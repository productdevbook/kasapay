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
//! [`Provider::refund`] is the only way money goes back: a capture has no
//! inverse. A [`Refund`] is its own object with its own life — its own
//! identifier where the provider issues one, its own [`RefundStatus`], and,
//! at one provider, its own [`NextAction`] for the payer to approve.
//!
//! [`Status`] has no `Refunded` and will not grow one. No provider reports a
//! refund as a payment's status — Stripe leaves a refunded PaymentIntent
//! `succeeded` — so the variant would be a branch that never runs for most of
//! them. "Has all of this gone back" is the refunds summed against
//! [`Charge::amount`].
//!
//! # What arrives without being asked for
//!
//! A payment that finishes out of band is not observable through
//! [`Provider`] at all: the payer goes away and comes back somewhere else.
//! [`Webhook::verify`] is the other half — it takes the bytes a provider
//! posted, shows they are the provider's, and says what they mean as an
//! [`Event`].
//!
//! It is a separate trait because it is a separate thing to hold: verifying
//! needs a webhook secret the API credentials do not carry. It is `async`
//! because verification is not one mechanism — Stripe signs the bytes, PayTR
//! signs three fields of them, and Mollie signs nothing at all and posts an
//! identifier to read back.
//!
//! An [`EventKind`] this crate does not model is [`EventKind::Other`] and
//! never an error. A provider retries a delivery until it is acknowledged, so
//! refusing an unknown type is how a shop earns a week of redeliveries for an
//! event nobody wanted.
//!
//! # Identifiers
//!
//! [`OrderRef`] is the caller's own reference for an order, and [`PaymentId`]
//! is how the provider names the payment that came of it. They are not the same
//! string even where they carry the same characters.
//!
//! Two questions are asked of every identifier the provider issues, and both
//! are answered by [`Id`], of which `PaymentId` is one kind.
//!
//! **What does it name?** The type says, and the compiler holds it: `PaymentId`
//! is [`Id<kind::Payment>`](Id), and an adapter that hands back a handle to
//! something else — iyzico's classic API names a hosted checkout form by a
//! token that is not a payment id — declares a kind of its own with [`IdKind`]
//! rather than lending this one out. Two identifiers the same provider issued
//! are alike enough to confuse, and the kind is what separates them.
//!
//! **Whose uniqueness does it rest on?** [`PaymentId::source`] says. PayTR
//! issues no identifier at all and names a payment by the `merchant_oid` it was
//! sent, so its source is [`IdSource::Derived`] and names that field. A caller
//! relying on an identifier being unique — writing it into a unique index,
//! keying a retry on it — is relying on the provider's guarantee or on their
//! own, and this is what tells the two apart.
//!
//! [`Charge::id`] is an `Option` for the provider that has not named the
//! payment yet, and never an empty string.
//!
//! # No type here holds a card number
//!
//! There is no field on [`ChargeRequest`] for one and there will not be. A
//! server that touches a card number is in PCI DSS scope on the merchant's
//! longest self-assessment rather than its shortest, and a library that makes
//! it easy to put one in a struct makes it easy to end up there without
//! noticing. Every provider kasapay ships has a way of taking a payment that
//! never sends a number through the caller's process — a page the provider
//! hosts, a token the payer's browser makes, a redirect — and those are the
//! ways kasapay implements.
//!
//! What a returning customer needs instead is [`InstrumentId`]: the provider
//! keeps the card and hands back a handle to it, and the handle is what a
//! payment carries. Charging one is not the same act as taking a card number,
//! and only the first of the two is here.
//!
//! [`Provider::instruments`] lists what a customer has on file — every
//! adapter answers the same shape, an [`Instrument`] carrying the identity and
//! something to show a person choosing between them. Charging one is not:
//! iyzico wants a buyer and a basket beside the token, Stripe an
//! `off_session` flag, Mollie a `sequenceType`, so that call stays each
//! adapter's own.
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
mod id;
mod instrument;
mod money;
mod provider;
mod raw;
mod refund;
mod secret;
mod webhook;

#[doc(inline)]
pub use crate::charge::{
    Charge, ChargeRequest, ChargeRequestBuilder, ChargeRequestError, IdempotencyKey, NextAction,
    OrderRef, Status,
};
#[doc(inline)]
pub use crate::error::{Error, ErrorKind};
#[doc(inline)]
pub use crate::id::{EventId, Id, IdKind, IdSource, InstrumentId, PaymentId, RefundId, kind};
#[doc(inline)]
pub use crate::instrument::Instrument;
#[doc(inline)]
pub use crate::money::{Currency, Money, MoneyError, UnknownCurrency};
#[doc(inline)]
pub use crate::provider::{Capabilities, Provider, ProviderId, async_trait};
#[doc(inline)]
pub use crate::raw::Raw;
#[doc(inline)]
pub use crate::refund::{
    Refund, RefundReason, RefundRequest, RefundRequestBuilder, RefundRequestError, RefundStatus,
};
#[doc(inline)]
pub use crate::secret::Secret;
#[doc(inline)]
pub use crate::webhook::{Delivery, Event, EventKind, Webhook};
