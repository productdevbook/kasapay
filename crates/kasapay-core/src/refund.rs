//! Money given back off a payment.

use std::collections::BTreeMap;

use url::Url;

use crate::charge::{IdempotencyKey, NextAction};
use crate::id::{PaymentId, RefundId};
use crate::money::{Money, MoneyError};
use crate::provider::ProviderId;
use crate::raw::Raw;

/// What a merchant tells the provider the money went back for.
///
/// Not a label for the shop's own use. Two of the five providers here have a
/// field for it that ends up in chargeback and reconciliation reporting —
/// Stripe's `reason`, iyzico's `reason` — and a fraudulent order refunded
/// without saying so has told them nothing, and cannot tell them later.
///
/// The three named ones are the intersection: Stripe documents exactly these
/// three, and iyzico's four are these three plus its own `OTHER`. What each
/// adapter does with them, and with [`RefundReason::Other`], is on that
/// adapter's `refund`.
///
/// # Exhaustive, for the same reason [`Currency`](crate::Currency) is
///
/// This is a value travelling *out* to a provider rather than one a caller
/// reads back, and an adapter that met a reason it had no word for would
/// quietly send none — which is the failure this type exists to prevent.
/// Adding a variant is a breaking change, and it forces every adapter to say
/// what it maps to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefundReason {
    /// The same money was taken twice.
    Duplicate,
    /// The payment was not the cardholder's.
    Fraudulent,
    /// The buyer asked for their money back.
    RequestedByCustomer,
    /// Something else, in the caller's own words.
    ///
    /// A provider with a free-text field takes this; one with an enumeration
    /// sends its own "other" value beside it, and one with neither drops it.
    /// Which of the three a provider is, is on its `refund`.
    Other(Box<str>),
}

/// Where a refund stands.
///
/// A refund is not instant anywhere. Every provider here answers a fresh one
/// as something other than "the money is back": Stripe's is `pending`,
/// Mollie's `queued`, and iyzico's In-Store refund has not even been agreed to
/// yet — the payer approves it through a deep link, which is why this shape
/// carries a [`Refund::next_action`] at all.
///
/// # Not every provider can produce every one of these
///
/// The same caution as [`Status`](crate::Status), and the same table shape.
/// From each adapter's mapping:
///
/// | | `Pending` | `RequiresAction` | `Succeeded` | `Failed` | `Canceled` |
/// |---|---|---|---|---|---|
/// | Stripe | yes | yes | yes | yes | yes |
/// | iyzico `classic` | no | no | yes | no | no |
/// | iyzico `in_store` | no | yes | no | no | no |
/// | PayTR | yes | no | no | no | no |
/// | Mollie | yes | no | yes | yes | yes |
/// | PayPal | yes | no | yes | yes | yes |
///
/// The two iyzico rows are the ones worth knowing about, and they are the two
/// halves of the same fact: **iyzico answers a refund once, and never again.**
/// `classic` answers a refund it has already accepted, so it is `Succeeded`
/// the moment it is read and there is no later state to poll for; `in_store`
/// answers a deep link and nothing else, so it is `RequiresAction` and what
/// became of it arrives on the callback rather than here.
///
/// **PayTR's is always `Pending`.** Its `/odeme/iade` answers that it took the
/// request, and the refund's own completion turns up later as a
/// `date_completed` on the payment's status query — `PayTr::refunds` is where
/// a caller reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefundStatus {
    /// Accepted, and the money is not back yet.
    Pending,
    /// Stalled until somebody acts — see [`Refund::next_action`].
    RequiresAction,
    /// The money is back with the payer.
    Succeeded,
    /// It will not be sent.
    Failed,
    /// Withdrawn before it was sent.
    Canceled,
}

impl RefundStatus {
    /// Whether the refund can still change without a further request from us.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::RequiresAction)
    }
}

/// Money given back off a payment, as the provider currently sees it.
///
/// Every field is public and the struct is open, for the same reason
/// [`Charge`](crate::Charge) is: an adapter in someone else's repository has
/// to be able to build one.
///
/// # There is no refunded status on the payment
///
/// [`Status`](crate::Status) has no variant for it and will not grow one — no
/// provider reports a refund as a payment status, so it would be a branch that
/// never runs for most of them. "Has all of this gone back" is these objects
/// summed with [`Money::checked_add`] against
/// [`Charge::amount`](crate::Charge::amount), and each adapter has a call that
/// lists them.
#[derive(Debug, Clone)]
pub struct Refund {
    /// How the provider names this refund, where it names it at all.
    ///
    /// `None` is not a missing field: **iyzico issues no identifier for a
    /// refund**. The nearest thing it has is the bank's `hostReference`, which
    /// exists only once the money has gone and so is no use for making the
    /// attempt idempotent. So this is `Option` for the same reason
    /// [`Charge::id`](crate::Charge::id) is — a composed key handed back in
    /// the field a real one lives in would read as a guarantee nobody made.
    ///
    /// Read [`Id::source`](crate::Id::source) before writing one into a unique
    /// index.
    pub id: Option<RefundId>,
    /// The payment the money came off.
    pub payment: PaymentId,
    /// How much went back.
    ///
    /// What the provider says it refunded, not what was asked for. A provider
    /// that refunds the remainder of a partly-refunded payment answers the
    /// remainder here, and it is smaller than
    /// [`RefundRequest::amount`] was.
    pub amount: Money,
    /// Where it stands.
    pub status: RefundStatus,
    /// What somebody must do before the money moves, if anything.
    ///
    /// iyzico's In-Store refund is the one that has one: it answers a deep
    /// link into iyzico's app, and the refund happens when the payer approves
    /// it there. Everywhere else this is `None` — the refund was accepted by
    /// the time the call answered.
    pub next_action: Option<NextAction>,
    /// Which provider this came from.
    pub provider: ProviderId,
    /// The provider's own response, untouched.
    pub raw: Raw,
}

/// A refund to make.
///
/// Build one with [`RefundRequest::builder`].
///
/// Two of these fields are here because one provider needs them and the others
/// ignore them, which is the same reason
/// [`ChargeRequest::customer`](crate::ChargeRequest::customer) exists:
/// iyzico's In-Store API wants a `userId` and a callback address on a refund
/// exactly as it does on a payment, and a refund request with nowhere to put
/// them would mean that adapter could never implement this trait method.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RefundRequest {
    /// The payment to take the money off.
    pub payment: PaymentId,
    /// How much to give back, or `None` for all of it.
    ///
    /// **`None` is not universally answerable.** Mollie and iyzico's classic
    /// API have no "refund what is left" request — both take an amount and
    /// only an amount — so their adapters say what they do with `None` on
    /// their own `refund`, and one of the two costs an extra request to find
    /// the figure out. See [`Provider::refund`](crate::Provider::refund) for
    /// the table.
    pub amount: Option<Money>,
    /// What to tell the provider the money went back for.
    pub reason: Option<RefundReason>,
    /// A key that makes replaying this refund safe.
    ///
    /// Worth more here than on a charge. A replayed charge opens a second
    /// payment somebody can see and cancel; a replayed refund gives the money
    /// back twice, and the second one is the shop's. A provider that cannot
    /// honour a key **refuses the refund** with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) rather than
    /// sending it without one, which is the rule
    /// [`Provider::refund`](crate::Provider::refund) and
    /// [`ChargeRequest::idempotency_key`](crate::ChargeRequest::idempotency_key)
    /// both state: a key accepted and dropped reads as a guarantee against
    /// giving the money back twice.
    pub idempotency_key: Option<IdempotencyKey>,
    /// The payer, in the provider's own terms, where the refund needs naming
    /// them again.
    ///
    /// iyzico's In-Store `userId`. Everywhere else this is ignored: the
    /// payment already knows whose it was.
    pub customer: Option<Box<str>>,
    /// Where the provider should send the payer back to, where a refund needs
    /// their agreement.
    ///
    /// iyzico's In-Store refund posts its outcome here, the same way its
    /// payment does. Everywhere else this is ignored.
    pub return_url: Option<Url>,
    /// Key/value pairs handed to the provider and given back unchanged.
    pub metadata: BTreeMap<String, String>,
}

impl RefundRequest {
    /// Starts building a refund.
    #[must_use]
    pub fn builder(payment: PaymentId) -> RefundRequestBuilder {
        RefundRequestBuilder {
            payment,
            amount: None,
            reason: None,
            idempotency_key: None,
            customer: None,
            return_url: None,
            metadata: BTreeMap::new(),
        }
    }
}

/// Collects the parts of a [`RefundRequest`] before it is checked.
#[derive(Debug, Clone)]
pub struct RefundRequestBuilder {
    payment: PaymentId,
    amount: Option<Money>,
    reason: Option<RefundReason>,
    idempotency_key: Option<IdempotencyKey>,
    customer: Option<Box<str>>,
    return_url: Option<Url>,
    metadata: BTreeMap<String, String>,
}

impl RefundRequestBuilder {
    /// Refunds part of the payment rather than all of it.
    #[must_use]
    pub fn amount(mut self, amount: Money) -> Self {
        self.amount = Some(amount);
        self
    }

    /// Says what the money went back for.
    #[must_use]
    pub fn reason(mut self, reason: RefundReason) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Sets the key that makes replaying this refund safe.
    #[must_use]
    pub fn idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Names the payer in the provider's own terms.
    #[must_use]
    pub fn customer(mut self, customer: impl Into<Box<str>>) -> Self {
        self.customer = Some(customer.into());
        self
    }

    /// Sets where the provider should send the payer back to.
    #[must_use]
    pub fn return_url(mut self, url: Url) -> Self {
        self.return_url = Some(url);
        self
    }

    /// Adds one key/value pair to hand to the provider.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Checks the request and produces it.
    pub fn build(self) -> Result<RefundRequest, RefundRequestError> {
        if self.payment.as_str().is_empty() {
            return Err(RefundRequestError::EmptyPaymentId);
        }
        if let Some(amount) = self.amount {
            amount.require_positive()?;
        }
        Ok(RefundRequest {
            payment: self.payment,
            amount: self.amount,
            reason: self.reason,
            idempotency_key: self.idempotency_key,
            customer: self.customer,
            return_url: self.return_url,
            metadata: self.metadata,
        })
    }
}

/// A [`RefundRequest`] was built out of parts that do not make a valid refund.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RefundRequestError {
    /// The payment identifier was empty.
    #[error("payment identifier is empty")]
    EmptyPaymentId,
    /// The amount was not one that can be refunded.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use super::{RefundRequest, RefundRequestError, RefundStatus};
    use crate::id::PaymentId;
    use crate::money::{Currency, Money};

    #[test]
    fn a_refund_with_no_amount_asks_for_all_of_it() {
        let request = RefundRequest::builder(PaymentId::issued("pi_1"))
            .build()
            .expect("valid request");
        assert!(request.amount.is_none());
    }

    #[test]
    fn build_rejects_a_zero_amount() {
        let err = RefundRequest::builder(PaymentId::issued("pi_1"))
            .amount(Money::from_minor_units(0, Currency::Try))
            .build()
            .expect_err("zero is not refundable");
        assert!(matches!(err, RefundRequestError::Amount(_)));
    }

    #[test]
    fn build_rejects_a_payment_nothing_names() {
        let err = RefundRequest::builder(PaymentId::issued(""))
            .build()
            .expect_err("an empty identifier is not usable");
        assert_eq!(err, RefundRequestError::EmptyPaymentId);
    }

    /// A refund nobody has to approve and nothing more will happen to is done.
    #[test]
    fn only_a_settled_refund_is_closed() {
        assert!(RefundStatus::Pending.is_open());
        assert!(RefundStatus::RequiresAction.is_open());
        assert!(!RefundStatus::Succeeded.is_open());
        assert!(!RefundStatus::Failed.is_open());
        assert!(!RefundStatus::Canceled.is_open());
    }
}
