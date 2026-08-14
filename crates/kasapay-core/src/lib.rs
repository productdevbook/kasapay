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
mod secret;

#[doc(inline)]
pub use crate::charge::{
    Charge, ChargeRequest, ChargeRequestBuilder, ChargeRequestError, IdempotencyKey, NextAction,
    OrderRef, PaymentId, Status,
};
#[doc(inline)]
pub use crate::error::{Error, ErrorKind};
#[doc(inline)]
pub use crate::money::{Currency, Money, MoneyError, UnknownCurrency};
#[doc(inline)]
pub use crate::provider::{Provider, ProviderId};
#[doc(inline)]
pub use crate::secret::Secret;
