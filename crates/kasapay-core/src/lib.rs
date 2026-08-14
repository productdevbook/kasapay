//! Provider-neutral payment types and the trait every kasapay adapter implements.
//!
//! Nothing here talks to a network. It defines what a charge is, what an
//! amount is, and what a failure is, so that [`kasapay-stripe`] and
//! [`kasapay-iyzico`] can disagree about everything else.
//!
//! # The shape of a payment
//!
//! [`Provider::charge`] does not return a completed payment. It returns a
//! [`Charge`] with a [`Status`], and — where the payer still has work to do —
//! a [`NextAction`]. Stripe answers a `client_secret` to confirm in the
//! browser; iyzico answers a deep link into its own app. Both are the same
//! shape here, and neither is a success yet.
//!
//! # Authorising and capturing
//!
//! [`Provider::capture`] takes funds an authorisation is only holding, and
//! [`Provider::cancel`] releases one that will never be taken. Not every
//! provider separates the two — iyzico's In-Store flow takes the money at
//! authorisation — so [`Provider::capabilities`] says which, and it says so
//! before there is a payment to ask about.
//!
//! # Identifiers
//!
//! [`OrderRef`] is the caller's own reference for an order, and [`PaymentId`]
//! is how the provider names the payment that came of it. They are not the same
//! string even where they carry the same characters: PayTR issues no identifier
//! at all and names a payment by the `merchant_oid` it was sent, so its
//! [`PaymentId::source`] is [`IdSource::Derived`] and says which field that is.
//! A caller relying on an identifier being unique — writing it into a unique
//! index, keying a retry on it — is relying on the provider's guarantee or on
//! their own, and `source` is what tells the two apart.
//!
//! [`Charge::id`] is an `Option` for the provider that has not named the
//! payment yet, and never an empty string.
//!
//! # Amounts
//!
//! [`Money`] counts minor units. There is no `f64` anywhere in this crate,
//! and [`Money::parse`] refuses precision a currency does not have rather than
//! rounding it away.
//!
//! [`kasapay-stripe`]: https://docs.rs/kasapay-stripe
//! [`kasapay-iyzico`]: https://docs.rs/kasapay-iyzico

mod charge;
mod error;
mod money;
mod provider;
mod raw;
mod secret;

#[doc(inline)]
pub use crate::charge::{
    Charge, ChargeRequest, ChargeRequestBuilder, ChargeRequestError, IdSource, IdempotencyKey,
    NextAction, OrderRef, PaymentId, Status,
};
#[doc(inline)]
pub use crate::error::{Error, ErrorKind};
#[doc(inline)]
pub use crate::money::{Currency, Money, MoneyError, UnknownCurrency};
#[doc(inline)]
pub use crate::provider::{Capabilities, Provider, ProviderId, async_trait};
#[doc(inline)]
pub use crate::raw::Raw;
#[doc(inline)]
pub use crate::secret::Secret;
