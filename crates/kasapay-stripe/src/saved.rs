//! Charging a card Stripe already holds, and what a listing says about one.
//!
//! Stripe names a saved instrument by a `pm_…` `PaymentMethod` id, made by
//! Stripe.js or Elements in the payer's own browser — never by a server-side
//! call, and never by this crate. That is also why there is no equivalent here
//! of iyzico's `cardUserKey`/`cardToken` pair: a `pm_…` stands alone rather
//! than needing the payer's half beside it, which is the one case
//! [`InstrumentId`]'s own doc comment says is not the rule.
//!
//! # Charging one
//!
//! [`Stripe::charge_saved_card`](crate::Stripe::charge_saved_card) confirms a
//! PaymentIntent with `customer` and `payment_method` set — the same call
//! Stripe documents for [charging a saved card without the payer
//! present](https://docs.stripe.com/payments/save-and-reuse). [`Payment::builder`]
//! is how one is built.
//!
//! # Authentication, and who is liable if there was none
//!
//! [`PaymentBuilder::off_session`] marks the payer as absent, which changes
//! what Stripe will try. Left off, a card that needs 3-D Secure comes back as
//! an ordinary [`Status::RequiresAction`](kasapay_core::Status::RequiresAction)
//! for the payer to complete. Set, Stripe skips the challenge when the card's
//! rules allow it — and when they do not, it answers an error carrying
//! `authentication_required` rather than a stalled [`Charge`](kasapay_core::Charge),
//! because there is nobody present to complete one. Either way, a payment that
//! goes through without a challenge is one where the chargeback liability sits
//! with the merchant rather than the card's issuer — that is what the
//! challenge is for.
//!
//! # No card number anywhere
//!
//! Neither [`StoredCard`] nor [`Payment`] has a field a card number could go
//! in. [`PaymentBuilder::build`] also refuses a `customer` or an instrument
//! that is a card number by shape — the same defence
//! `kasapay_iyzico::classic::saved` uses, against a field wired to the wrong
//! source rather than against an attacker.

use std::fmt;

use kasapay_core::{Error, ErrorKind, IdempotencyKey, InstrumentId, Money, MoneyError, OrderRef};
use url::Url;

use crate::convert::PROVIDER;

/// A card Stripe holds against a customer, safe to show somebody so they can
/// pick one.
///
/// Comes back from [`Stripe::stored_cards`](crate::Stripe::stored_cards).
/// [`StoredCard::token`] is what [`Payment::builder`] takes to charge it.
#[derive(Debug, Clone)]
pub struct StoredCard {
    /// Stripe's `pm_…`, which stands alone — see the module doc.
    pub token: InstrumentId,
    /// The scheme the card runs on.
    pub brand: Brand,
    /// The last four digits, for showing somebody which card this is.
    pub last_four: Box<str>,
    /// Credit, debit or prepaid.
    pub funding: Funding,
    /// The card's expiry month, one to twelve.
    pub exp_month: i64,
    /// The card's expiry year, four digits.
    pub exp_year: i64,
    /// The two-letter country the card was issued in, where Stripe knows it.
    pub country: Option<Box<str>>,
}

impl TryFrom<stripe_shared::PaymentMethod> for StoredCard {
    type Error = Error;

    /// Reads a `PaymentMethod` Stripe listed for a customer.
    ///
    /// Fails only if `card` is missing, which should not happen: the listing
    /// this is built from is filtered to `type=card`. Kept as a checked
    /// conversion rather than a panic because a filter is Stripe's promise,
    /// not this crate's.
    fn try_from(method: stripe_shared::PaymentMethod) -> Result<Self, Self::Error> {
        let card = method.card.ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a card-type PaymentMethod carried no card details",
            )
        })?;
        Ok(Self {
            token: InstrumentId::issued(method.id.as_str()),
            brand: Brand::from(card.brand.as_str()),
            last_four: card.last4.into_boxed_str(),
            funding: Funding::from(card.funding.as_str()),
            exp_month: card.exp_month,
            exp_year: card.exp_year,
            country: card.country.map(String::into_boxed_str),
        })
    }
}

/// The scheme a card runs on, as Stripe's `PaymentMethod` documents
/// `card.brand`: `amex`, `cartes_bancaires`, `diners`, `discover`,
/// `eftpos_au`, `jcb`, `link`, `mastercard`, `unionpay`, `visa` or `unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Brand {
    /// American Express.
    Amex,
    /// Cartes Bancaires.
    CartesBancaires,
    /// Diners Club.
    Diners,
    /// Discover.
    Discover,
    /// Eftpos Australia.
    EftposAu,
    /// JCB.
    Jcb,
    /// Link, Stripe's own wallet.
    Link,
    /// Mastercard.
    Mastercard,
    /// `UnionPay`.
    UnionPay,
    /// Visa.
    Visa,
    /// Stripe's own documented value for a brand it could not identify.
    Unknown,
    /// A brand Stripe has started returning since this was written.
    Other(Box<str>),
}

impl fmt::Display for Brand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Amex => f.write_str("amex"),
            Self::CartesBancaires => f.write_str("cartes_bancaires"),
            Self::Diners => f.write_str("diners"),
            Self::Discover => f.write_str("discover"),
            Self::EftposAu => f.write_str("eftpos_au"),
            Self::Jcb => f.write_str("jcb"),
            Self::Link => f.write_str("link"),
            Self::Mastercard => f.write_str("mastercard"),
            Self::UnionPay => f.write_str("unionpay"),
            Self::Visa => f.write_str("visa"),
            Self::Unknown => f.write_str("unknown"),
            Self::Other(name) => f.write_str(name),
        }
    }
}

impl From<&str> for Brand {
    fn from(value: &str) -> Self {
        match value {
            "amex" => Self::Amex,
            "cartes_bancaires" => Self::CartesBancaires,
            "diners" => Self::Diners,
            "discover" => Self::Discover,
            "eftpos_au" => Self::EftposAu,
            "jcb" => Self::Jcb,
            "link" => Self::Link,
            "mastercard" => Self::Mastercard,
            "unionpay" => Self::UnionPay,
            "visa" => Self::Visa,
            "unknown" => Self::Unknown,
            other => Self::Other(other.into()),
        }
    }
}

/// How a card is funded, as Stripe's `PaymentMethod` documents
/// `card.funding`: `credit`, `debit`, `prepaid`, or `unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Funding {
    /// A credit card.
    Credit,
    /// A debit card.
    Debit,
    /// A prepaid card.
    Prepaid,
    /// Stripe's own documented value for funding it could not identify.
    Unknown,
    /// A value Stripe has started returning since this was written.
    Other(Box<str>),
}

impl fmt::Display for Funding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Credit => f.write_str("credit"),
            Self::Debit => f.write_str("debit"),
            Self::Prepaid => f.write_str("prepaid"),
            Self::Unknown => f.write_str("unknown"),
            Self::Other(name) => f.write_str(name),
        }
    }
}

impl From<&str> for Funding {
    fn from(value: &str) -> Self {
        match value {
            "credit" => Self::Credit,
            "debit" => Self::Debit,
            "prepaid" => Self::Prepaid,
            "unknown" => Self::Unknown,
            other => Self::Other(other.into()),
        }
    }
}

/// A payment to take against a card Stripe already holds.
///
/// Build one with [`Payment::builder`].
#[derive(Debug, Clone)]
pub struct Payment {
    /// The caller's reference for the order.
    pub order: OrderRef,
    /// The amount to take.
    pub amount: Money,
    /// The payer, in Stripe's own terms — the `cus_…` the instrument is saved
    /// under. Stripe refuses a `payment_method` attached to a different
    /// customer than the one named here.
    pub customer: Box<str>,
    /// The card to charge.
    pub instrument: InstrumentId,
    /// Whether the payer is absent from this attempt — see the module doc.
    pub off_session: bool,
    /// Free text shown on statements or in Stripe's dashboard.
    pub description: Option<Box<str>>,
    /// Where Stripe should send the payer back to, if it has to send them
    /// anywhere.
    ///
    /// This call confirms as it creates, and Stripe documents `return_url` as
    /// usable **only** with `confirm: true` — so this is the path it belongs
    /// on. It matters where the confirmation cannot finish without the payer:
    /// a redirect-based payment method, or a card whose issuer asks for 3-D
    /// Secure on an `off_session` charge.
    ///
    /// `None` is the ordinary case, a card that authenticates without them.
    pub return_url: Option<Url>,
    /// A key that makes replaying this request safe.
    pub idempotency_key: Option<IdempotencyKey>,
}

impl Payment {
    /// Starts building a payment against a stored card.
    #[must_use]
    pub fn builder(
        order: OrderRef,
        amount: Money,
        customer: impl Into<Box<str>>,
        instrument: InstrumentId,
    ) -> PaymentBuilder {
        PaymentBuilder {
            order,
            amount,
            customer: customer.into(),
            instrument,
            off_session: false,
            description: None,
            return_url: None,
            idempotency_key: None,
        }
    }
}

/// Collects the parts of a [`Payment`] before it is checked.
#[derive(Debug, Clone)]
pub struct PaymentBuilder {
    order: OrderRef,
    amount: Money,
    customer: Box<str>,
    instrument: InstrumentId,
    off_session: bool,
    description: Option<Box<str>>,
    return_url: Option<Url>,
    idempotency_key: Option<IdempotencyKey>,
}

impl PaymentBuilder {
    /// Marks the payer as absent from this attempt.
    ///
    /// Sent as Stripe's `off_session: true`. See the module doc for what this
    /// changes about authentication and who is liable if none happens.
    #[must_use]
    pub const fn off_session(mut self) -> Self {
        self.off_session = true;
        self
    }

    /// Sets where Stripe should send the payer back to.
    ///
    /// Only reached when the confirmation cannot finish without the payer.
    /// Stripe's own document gives `return_url` as usable only with
    /// `confirm: true`, which this call sends and the hosted-form create does
    /// not — so this is where `Provider::charge` carries the field to.
    #[must_use]
    pub fn return_url(mut self, return_url: Url) -> Self {
        self.return_url = Some(return_url);
        self
    }

    /// Sets the free text shown on statements or in Stripe's dashboard.
    #[must_use]
    pub fn description(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the key that makes replaying this request safe.
    #[must_use]
    pub fn idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Checks the request and produces it.
    pub fn build(self) -> Result<Payment, PaymentError> {
        if self.order.as_str().is_empty() {
            return Err(PaymentError::EmptyOrderRef);
        }
        if self.customer.is_empty() {
            return Err(PaymentError::EmptyCustomer);
        }
        if self.instrument.as_str().is_empty() {
            return Err(PaymentError::EmptyInstrument);
        }
        for (field, value) in [
            ("customer", &*self.customer),
            ("payment_method", self.instrument.as_str()),
        ] {
            if kasapay_core::looks_like_a_card_number(value) {
                return Err(PaymentError::CardNumber { field });
            }
        }
        self.amount.require_positive()?;
        Ok(Payment {
            order: self.order,
            amount: self.amount,
            customer: self.customer,
            instrument: self.instrument,
            off_session: self.off_session,
            description: self.description,
            return_url: self.return_url,
            idempotency_key: self.idempotency_key,
        })
    }
}

/// A [`Payment`] was built out of parts Stripe will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PaymentError {
    /// No order reference was set.
    #[error("order reference is empty")]
    EmptyOrderRef,
    /// No customer was set, and a saved instrument means nothing without one.
    #[error("a saved-card payment needs the customer it is saved under")]
    EmptyCustomer,
    /// No instrument was set.
    #[error("a saved-card payment needs the PaymentMethod id to charge")]
    EmptyInstrument,
    /// The customer or the instrument was a card number by shape.
    #[error(
        "{field} was given what is a card number by shape; a saved instrument is named by \
         Stripe's own handles, and a card number reaching this process is the thing this refuses"
    )]
    CardNumber {
        /// Which field, named as Stripe names it on the wire.
        field: &'static str,
    },
    /// An amount was not one Stripe will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use kasapay_core::{Currency, InstrumentId, Money, OrderRef};

    use super::{Brand, Funding, Payment, PaymentError};

    fn ten_dollars() -> Money {
        Money::parse("10.00", Currency::Usd).expect("valid amount")
    }

    #[test]
    fn every_documented_brand_renders_back_to_what_stripe_sent() {
        for name in [
            "amex",
            "cartes_bancaires",
            "diners",
            "discover",
            "eftpos_au",
            "jcb",
            "link",
            "mastercard",
            "unionpay",
            "visa",
            "unknown",
        ] {
            assert_eq!(Brand::from(name).to_string(), name);
        }
        let unknown = Brand::from("some_future_brand");
        assert_eq!(unknown, Brand::Other("some_future_brand".into()));
        assert_eq!(unknown.to_string(), "some_future_brand");
    }

    #[test]
    fn every_documented_funding_renders_back_to_what_stripe_sent() {
        for name in ["credit", "debit", "prepaid", "unknown"] {
            assert_eq!(Funding::from(name).to_string(), name);
        }
        assert_eq!(Funding::from("giro"), Funding::Other("giro".into()));
    }

    #[test]
    fn a_card_number_wired_into_the_instrument_is_refused() {
        let err = Payment::builder(
            OrderRef::new("ord-1"),
            ten_dollars(),
            "cus_kasapay1",
            InstrumentId::issued("4242424242424242"),
        )
        .build()
        .expect_err("a card number does not name a saved instrument");
        assert_eq!(
            err,
            PaymentError::CardNumber {
                field: "payment_method"
            }
        );
        assert!(!err.to_string().contains("4242424242424242"));
    }

    #[test]
    fn an_empty_customer_or_instrument_is_refused_before_a_request_is_built() {
        assert_eq!(
            Payment::builder(
                OrderRef::new("ord-1"),
                ten_dollars(),
                "",
                InstrumentId::issued("pm_1")
            )
            .build()
            .expect_err("no customer"),
            PaymentError::EmptyCustomer
        );
        assert_eq!(
            Payment::builder(
                OrderRef::new("ord-1"),
                ten_dollars(),
                "cus_kasapay1",
                InstrumentId::issued("")
            )
            .build()
            .expect_err("no instrument"),
            PaymentError::EmptyInstrument
        );
    }

    #[test]
    fn off_session_defaults_to_false_and_the_builder_can_set_it() {
        let on_session = Payment::builder(
            OrderRef::new("ord-1"),
            ten_dollars(),
            "cus_kasapay1",
            InstrumentId::issued("pm_1"),
        )
        .build()
        .expect("valid payment");
        assert!(!on_session.off_session);

        let off_session = Payment::builder(
            OrderRef::new("ord-1"),
            ten_dollars(),
            "cus_kasapay1",
            InstrumentId::issued("pm_1"),
        )
        .off_session()
        .build()
        .expect("valid payment");
        assert!(off_session.off_session);
    }
}
