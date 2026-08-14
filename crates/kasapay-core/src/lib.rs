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
//! # Giving money back
//!
//! [`Provider::refund`] is the only way money goes back. Capture has no
//! inverse: captured money is refunded, not un-captured, and the two are
//! different entries in a ledger. A [`Refund`] carries its own
//! [`RefundId`] because one capture is commonly refunded several times — three
//! returned items out of an order of five — and
//! [`Capabilities::repeated_refund`] is where a provider says whether it will
//! allow that.
//!
//! # Learning what happened without asking
//!
//! A payment that finishes out of band — the payer came back to the provider
//! rather than to us — is reported by a delivery to a callback address, and
//! [`Provider::verify_webhook`] is what turns one into an [`Event`]. Three
//! things about it are load-bearing:
//!
//! - **Verification comes before anything else.** There is no way to build an
//!   [`Event`] out of an unsigned body, because there is no constructor that
//!   does not check first.
//! - **An event type kasapay does not model is [`EventKind::Other`], not an
//!   error.** A provider adds types without asking, and answering `Err` puts
//!   the caller into a redelivery loop that lasts days.
//! - **[`EventId`] says whether the provider issued the id or kasapay composed
//!   it**, because a caller writes it into a unique index and the two are not
//!   the same promise.
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
mod refund;
mod secret;
mod webhook;

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
pub use crate::provider::{Capabilities, Provider, ProviderId, async_trait};
#[doc(inline)]
pub use crate::raw::Raw;
#[doc(inline)]
pub use crate::refund::{
    Refund, RefundId, RefundRequest, RefundRequestBuilder, RefundRequestError, RefundStatus,
};
#[doc(inline)]
pub use crate::secret::Secret;
#[doc(inline)]
pub use crate::webhook::{Event, EventId, EventKind, header};
