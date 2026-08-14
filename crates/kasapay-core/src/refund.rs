//! Giving money back.

use std::collections::BTreeMap;
use std::fmt;

use url::Url;

use crate::charge::{IdempotencyKey, NextAction, PaymentId};
use crate::money::{Money, MoneyError};
use crate::provider::ProviderId;
use crate::raw::Raw;

/// A refund's own identifier, distinct from the payment's.
///
/// One capture can be refunded three times — three returned items — and each
/// of those is its own object with its own outcome. A caller that keyed a
/// ledger on the payment id alone would collapse them into one.
///
/// # Not every provider issues one
///
/// Stripe does (`re_…`). iyzico and PayTR do not, so their adapters put the
/// closest thing they have here and say so in their own documentation. A
/// caller writing this into a unique index should read that first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefundId(Box<str>);

impl RefundId {
    /// Wraps an identifier as the provider gave it, or as the adapter built it.
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefundId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a refund stands.
///
/// A refund is not instant anywhere: the money leaves the merchant when the
/// provider accepts it and reaches the payer days later, and two providers
/// here make the payer approve it first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RefundStatus {
    /// Accepted, not settled.
    Pending,
    /// Stalled until the payer does something — see [`Refund::next_action`].
    RequiresAction,
    /// The money is back.
    Succeeded,
    /// It did not go back.
    Failed,
    /// Withdrawn before it settled.
    Canceled,
    /// A state the provider has added since this was written.
    Other(Box<str>),
}

impl RefundStatus {
    /// Whether the refund can still change without a further request from us.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Pending | Self::RequiresAction)
    }
}

/// Money given back off a payment.
///
/// Every field is public and the struct is open, for the same reason
/// [`Charge`](crate::Charge) is: an adapter in someone else's repository has
/// to be able to build one.
#[derive(Debug, Clone)]
pub struct Refund {
    /// This refund's own identifier.
    ///
    /// Not the payment's: three refunds against one capture are three of
    /// these, and a caller replaying one needs to tell them apart.
    pub id: RefundId,
    /// The payment it came off.
    pub payment: PaymentId,
    /// How much went back.
    pub amount: Money,
    /// Where it stands.
    pub status: RefundStatus,
    /// What the payer must do before the money moves, if anything.
    ///
    /// iyzico's In-Store flow makes the payer approve a refund in the same app
    /// they approved the payment in, so a refund there stalls on a deep link
    /// exactly as a charge does.
    pub next_action: Option<NextAction>,
    /// The provider's or the bank's own reference for it, for reconciling
    /// against a statement.
    ///
    /// Distinct from [`Refund::id`]: an id names the refund to the provider's
    /// API, a reference names the movement to whoever produced the statement.
    /// `None` where the provider gives neither.
    pub reference: Option<Box<str>>,
    /// Which provider this came from.
    pub provider: ProviderId,
    /// The provider's own response, untouched.
    pub raw: Raw,
}

/// A refund to make.
///
/// Build one with [`RefundRequest::builder`].
///
/// # Why this is a struct and not three arguments
///
/// A retried refund is exactly where an idempotency key is needed, and a bare
/// `(payment, amount, reason)` has nowhere to put one. The optional fields
/// beyond that are the same ones [`ChargeRequest`](crate::ChargeRequest)
/// carries, and for the same reasons: iyzico wants a `userId` and a callback
/// address on a refund exactly as it does on a charge.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RefundRequest {
    /// The payment to take the money off.
    pub payment: PaymentId,
    /// How much to give back. `None` is all of it.
    ///
    /// A partial amount requires
    /// [`Capabilities::partial_refund`](crate::Capabilities), and a second
    /// refund against one payment requires
    /// [`Capabilities::repeated_refund`](crate::Capabilities).
    pub amount: Option<Money>,
    /// Why, where the provider has somewhere to put it.
    pub reason: Option<Box<str>>,
    /// A key that makes replaying this refund safe.
    ///
    /// A provider either sends it or refuses the request with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported), exactly as
    /// on a charge. Accepting a key and dropping it would read as a guarantee
    /// against a double refund where there is none.
    pub idempotency_key: Option<IdempotencyKey>,
    /// The payer, in the provider's own terms, where the provider needs one.
    ///
    /// iyzico's In-Store API needs its `userId` on a refund and refuses one
    /// without it.
    pub customer: Option<Box<str>>,
    /// Where the provider should send the payer to approve the refund.
    ///
    /// Required by any provider that makes the payer approve it — iyzico's
    /// In-Store flow does.
    pub return_url: Option<Url>,
    /// Key/value pairs handed to the provider and given back unchanged.
    pub metadata: BTreeMap<String, String>,
}

impl RefundRequest {
    /// Starts building a refund of the whole payment.
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
    reason: Option<Box<str>>,
    idempotency_key: Option<IdempotencyKey>,
    customer: Option<Box<str>>,
    return_url: Option<Url>,
    metadata: BTreeMap<String, String>,
}

impl RefundRequestBuilder {
    /// Gives back part of the payment rather than all of it.
    #[must_use]
    pub const fn amount(mut self, amount: Money) -> Self {
        self.amount = Some(amount);
        self
    }

    /// Says why the money is going back.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<Box<str>>) -> Self {
        self.reason = Some(reason.into());
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

    /// Sets where the provider should send the payer to approve the refund.
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
    /// The payment id was empty.
    #[error("payment id is empty")]
    EmptyPaymentId,
    /// The amount was not one that can be refunded.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use super::{RefundId, RefundRequest, RefundRequestError, RefundStatus};
    use crate::charge::{IdempotencyKey, PaymentId};
    use crate::money::{Currency, Money};

    #[test]
    fn a_whole_refund_names_no_amount() {
        let request = RefundRequest::builder(PaymentId::new("pi_1"))
            .reason("returned unopened")
            .build()
            .expect("valid request");
        assert!(request.amount.is_none());
        assert_eq!(request.reason.as_deref(), Some("returned unopened"));
        assert!(request.idempotency_key.is_none());
    }

    #[test]
    fn a_partial_refund_keeps_what_was_set() {
        let request = RefundRequest::builder(PaymentId::new("pi_1"))
            .amount(Money::from_minor_units(400, Currency::Try))
            .idempotency_key(IdempotencyKey::new("ref-1"))
            .customer("kasiyer-7")
            .metadata("item", "3")
            .build()
            .expect("valid request");
        assert_eq!(request.amount.map(Money::minor_units), Some(400));
        assert_eq!(
            request.idempotency_key.as_ref().map(IdempotencyKey::as_str),
            Some("ref-1")
        );
        assert_eq!(request.customer.as_deref(), Some("kasiyer-7"));
        assert_eq!(request.metadata.get("item").map(String::as_str), Some("3"));
    }

    #[test]
    fn build_rejects_a_zero_amount() {
        let err = RefundRequest::builder(PaymentId::new("pi_1"))
            .amount(Money::from_minor_units(0, Currency::Try))
            .build()
            .expect_err("zero is not refundable");
        assert!(matches!(err, RefundRequestError::Amount(_)));
    }

    #[test]
    fn build_rejects_an_empty_payment_id() {
        let err = RefundRequest::builder(PaymentId::new(""))
            .build()
            .expect_err("an empty id names no payment");
        assert_eq!(err, RefundRequestError::EmptyPaymentId);
    }

    #[test]
    fn three_refunds_of_one_payment_are_three_ids() {
        // The reason RefundId exists: a ledger keyed on the payment would
        // collapse three returned items into one entry.
        let ids = [RefundId::new("re_1"), RefundId::new("re_2")];
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[0].to_string(), "re_1");
    }

    #[test]
    fn a_settled_refund_is_not_open() {
        assert!(RefundStatus::Pending.is_open());
        assert!(RefundStatus::RequiresAction.is_open());
        assert!(!RefundStatus::Succeeded.is_open());
        assert!(!RefundStatus::Failed.is_open());
        assert!(!RefundStatus::Other("disputed".into()).is_open());
    }
}
