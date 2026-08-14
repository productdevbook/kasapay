//! What is asked of a provider, and what comes back.

use std::collections::BTreeMap;
use std::fmt;

use url::Url;

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

/// The provider's own identifier for a payment.
///
/// Opaque on purpose: Stripe issues `pi_…`, iyzico a 64-bit integer, and
/// nothing outside the adapter should read either.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaymentId(Box<str>);

impl PaymentId {
    /// Wraps an identifier as the provider gave it.
    pub fn new(value: impl Into<Box<str>>) -> Self {
        Self(value.into())
    }

    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaymentId {
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
    /// The provider's identifier for the payment.
    pub id: PaymentId,
    /// The order reference the charge was created against, when the provider kept it.
    pub order: Option<OrderRef>,
    /// The amount.
    pub amount: Money,
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
