//! What is asked of a provider, and what comes back.

use std::collections::BTreeMap;
use std::fmt;

use url::Url;

use crate::id::PaymentId;
use crate::money::{Money, MoneyError};
use crate::provider::ProviderId;
use crate::raw::Raw;

/// Our own reference for an order, chosen by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderRef(Box<str>);

impl OrderRef {
    /// Wraps a caller-chosen order reference.
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// The reference as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrderRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A key that makes replaying a charge safe.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(Box<str>);

impl IdempotencyKey {
    /// Wraps a caller-chosen key.
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// The key as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Where a payment stands.
///
/// # Not every provider can produce every one of these
///
/// A caller that branches on a status a provider never sends has written a
/// branch that never runs, and the compiler cannot say so. What each adapter
/// can actually produce, from reading their mappings:
///
/// | | `Pending` | `RequiresAction` | `Authorized` | `Captured` | `Failed` | `Canceled` |
/// |---|---|---|---|---|---|---|
/// | Stripe | yes | yes | yes | yes | **no** | yes |
/// | iyzico `in_store` | yes | yes | no | yes | yes | yes |
/// | iyzico `classic` | yes | yes | no | yes | yes | no |
/// | PayTR | no | yes | no | yes | notice only | no |
/// | Mollie | yes | yes | yes | yes | yes | yes |
///
/// Three of those cells are worth knowing about.
///
/// **Stripe never reports `Failed`.** A PaymentIntent whose card was declined
/// goes back to `requires_payment_method`, which arrives here as
/// [`Status::RequiresAction`] — and that is honest, because the payer can try
/// another card. A caller waiting for `Failed` from Stripe waits forever.
///
/// **PayTR reports a refusal only on the payment notice.** Its status query
/// answers a payment that succeeded or an error, so `Failed` comes from
/// `Notice::charge` and never from `charge_status`. Worse, that error is the
/// same for a payment PayTR refused and an order it has never heard of —
/// `ErrorKind::NotFound` either way, because PayTR sends nothing that
/// separates them.
///
/// **Mollie's `Failed` is two of its own states.** A payment it refused is
/// `failed`; one the payer abandoned until it could no longer be paid is
/// `expired`, which is neither a refusal nor a withdrawal and has no word
/// here. Both arrive as [`Status::Failed`], and which it was is in
/// [`Charge::raw`]. A caller counting declines separately from abandoned
/// checkouts reads it there.
///
/// Each adapter's own documentation says the same thing where a reader will
/// meet it.
///
/// # Nothing here says a payment was refunded
///
/// No provider reports one as a status. Stripe's PaymentIntent stays
/// `succeeded` with the refunds beside it, PayTR lists them on the payment,
/// and iyzico's In-Store receipt sets a flag on a payment that is still
/// captured. A variant only one of them could ever produce would be a branch
/// that never runs for the others.
///
/// So "how much of this has gone back" is the adapter's own refunds — Stripe's
/// and PayTR's both answer a list — summed with [`Money::checked_add`] and
/// compared against [`Charge::amount`]. Mollie is the one that answers the
/// figure outright, as `amountRefunded` on a payment that still reads `paid`,
/// and it is read off [`Charge::raw`] rather than off a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// Accepted, nothing more required yet, not settled.
    Pending,
    /// Stalled until the payer does something — see [`Charge::next_action`].
    RequiresAction,
    /// Funds are held but not taken.
    Authorized,
    /// Funds are taken.
    Captured,
    /// Refused, and will not proceed.
    Failed,
    /// Withdrawn before it completed.
    Canceled,
}

impl Status {
    /// Whether the payment can still change without a further request from us.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::RequiresAction | Self::Authorized
        )
    }
}

/// What the payer has to do before the payment can go on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NextAction {
    /// Send the payer to this address — a hosted page, or an app deep link.
    Redirect {
        /// Where to send them.
        url: Url,
        /// A token the provider will want back when the payer returns.
        ///
        /// iyzico's `paymentSessionToken` is this; it is required to read the
        /// callback it later posts.
        continuation: Option<Box<str>>,
    },
    /// Hand this to the provider's client-side SDK and let it finish there.
    ConfirmOnClient {
        /// The provider's client-side handle for the payment.
        client_secret: Box<str>,
    },
}

/// A charge, as the provider currently sees it.
///
/// Every field is public and the struct is open: a provider adapter living
/// outside this workspace has to be able to build one.
#[derive(Debug, Clone)]
pub struct Charge {
    /// How the provider names this payment, where it names it at all.
    ///
    /// `None` is a payment nothing identifies yet — an iyzico checkout form the
    /// payer has not finished has no `paymentId` — and it is `None` rather than
    /// an empty string so that it cannot be handed back as a handle and quietly
    /// read as a payment nobody made. A provider that never issues one and has
    /// nothing to compose one from answers `None` here always, and
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) to
    /// [`Provider::charge_status`](crate::Provider::charge_status).
    ///
    /// Read [`PaymentId::source`] before writing one into a unique index.
    pub id: Option<PaymentId>,
    /// The order reference the charge was created against, when the provider kept it.
    pub order: Option<OrderRef>,
    /// What the payer is charged. The money that moves.
    ///
    /// Not always what the goods came to: an instalment surcharge lands here
    /// and not on the basket. This is the figure that reconciles against a
    /// bank statement.
    pub amount: Money,
    /// What the goods came to, when the provider reports it separately and it
    /// differs from [`Charge::amount`].
    ///
    /// `None` does not mean the two are equal — it means this provider does
    /// not say. Stripe has no basket at all at the payment level, so it never
    /// answers; iyzico's `price` and PayTR's `payment_amount` do.
    pub order_amount: Option<Money>,
    /// Where it stands.
    pub status: Status,
    /// What the payer must do next, if anything.
    pub next_action: Option<NextAction>,
    /// Which provider this came from.
    pub provider: ProviderId,
    /// The provider's own response, untouched.
    ///
    /// The escape hatch: everything kasapay does not model is still here.
    pub raw: Raw,
}

/// A charge to create.
///
/// Build one with [`ChargeRequest::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChargeRequest {
    /// The caller's reference for the order.
    pub order: OrderRef,
    /// The amount to take.
    pub amount: Money,
    /// The payer, in the provider's own terms, when there is one on file.
    pub customer: Option<Box<str>>,
    /// Free text shown on statements or in the provider's dashboard.
    pub description: Option<Box<str>>,
    /// Where the provider should send the payer back to.
    pub return_url: Option<Url>,
    /// A key that makes replaying this request safe.
    ///
    /// A provider either sends it or refuses the request with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported). Accepting a
    /// key and dropping it would read as a guarantee against double charges
    /// where there is none.
    pub idempotency_key: Option<IdempotencyKey>,
    /// Key/value pairs handed to the provider and given back unchanged.
    pub metadata: BTreeMap<String, String>,
}

impl ChargeRequest {
    /// Starts building a charge.
    #[must_use]
    pub fn builder(order: OrderRef, amount: Money) -> ChargeRequestBuilder {
        ChargeRequestBuilder {
            order,
            amount,
            customer: None,
            description: None,
            return_url: None,
            idempotency_key: None,
            metadata: BTreeMap::new(),
        }
    }
}

/// Collects the parts of a [`ChargeRequest`] before it is checked.
#[derive(Debug, Clone)]
pub struct ChargeRequestBuilder {
    order: OrderRef,
    amount: Money,
    customer: Option<Box<str>>,
    description: Option<Box<str>>,
    return_url: Option<Url>,
    idempotency_key: Option<IdempotencyKey>,
    metadata: BTreeMap<String, String>,
}

impl ChargeRequestBuilder {
    /// Names the payer in the provider's own terms.
    #[must_use]
    pub fn customer(mut self, customer: impl Into<Box<str>>) -> Self {
        self.customer = Some(customer.into());
        self
    }

    /// Sets the free text shown on statements or in the provider's dashboard.
    #[must_use]
    pub fn description(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets where the provider should send the payer back to.
    #[must_use]
    pub fn return_url(mut self, url: Url) -> Self {
        self.return_url = Some(url);
        self
    }

    /// Sets the key that makes replaying this request safe.
    #[must_use]
    pub fn idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Adds one key/value pair to hand to the provider.
    #[must_use]
    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Checks the request and produces it.
    pub fn build(self) -> Result<ChargeRequest, ChargeRequestError> {
        if self.order.as_str().is_empty() {
            return Err(ChargeRequestError::EmptyOrderRef);
        }
        self.amount.require_positive()?;
        Ok(ChargeRequest {
            order: self.order,
            amount: self.amount,
            customer: self.customer,
            description: self.description,
            return_url: self.return_url,
            idempotency_key: self.idempotency_key,
            metadata: self.metadata,
        })
    }
}

/// A [`ChargeRequest`] was built out of parts that do not make a valid charge.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ChargeRequestError {
    /// The order reference was empty.
    #[error("order reference is empty")]
    EmptyOrderRef,
    /// The amount was not one that can be charged.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use super::{ChargeRequest, ChargeRequestError, OrderRef};
    use crate::money::{Currency, Money};

    fn ten_lira() -> Money {
        Money::from_minor_units(1000, Currency::Try)
    }

    #[test]
    fn a_built_request_keeps_what_was_set() {
        let request = ChargeRequest::builder(OrderRef::new("ord-1"), ten_lira())
            .description("bir kahve")
            .metadata("site", "vucod")
            .build()
            .expect("valid request");
        assert_eq!(request.order.as_str(), "ord-1");
        assert_eq!(request.description.as_deref(), Some("bir kahve"));
        assert_eq!(
            request.metadata.get("site").map(String::as_str),
            Some("vucod")
        );
        assert!(request.customer.is_none());
    }

    #[test]
    fn build_rejects_a_zero_amount() {
        let err = ChargeRequest::builder(
            OrderRef::new("ord-1"),
            Money::from_minor_units(0, Currency::Try),
        )
        .build()
        .expect_err("zero is not chargeable");
        assert!(matches!(err, ChargeRequestError::Amount(_)));
    }

    #[test]
    fn build_rejects_an_empty_order_reference() {
        let err = ChargeRequest::builder(OrderRef::new(""), ten_lira())
            .build()
            .expect_err("an empty reference is not usable");
        assert_eq!(err, ChargeRequestError::EmptyOrderRef);
    }
}
