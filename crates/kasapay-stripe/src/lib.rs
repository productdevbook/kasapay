//! Stripe, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! A thin adapter over [`async-stripe`], which is generated from Stripe's own
//! OpenAPI document and regenerated weekly. This crate does not re-derive that
//! work; it maps PaymentIntents onto kasapay's [`Charge`](kasapay_core::Charge)
//! and gets out of the way.
//!
//! # What maps onto what
//!
//! - A charge is a PaymentIntent. `ChargeRequest::order` has no Stripe field of
//!   its own, so it travels as metadata under [`ORDER_METADATA_KEY`].
//! - A PaymentIntent that still needs the payer comes back as
//!   [`NextAction::ConfirmOnClient`](kasapay_core::NextAction::ConfirmOnClient)
//!   carrying the `client_secret` for Stripe.js.
//! - [`Stripe::refund`] gives money back, and [`Stripe::cancel`] withdraws a
//!   payment that was never captured. Neither is on the shared trait yet.
//! - A refunded PaymentIntent still reads `succeeded`, so how much of a
//!   payment has been given back is [`Stripe::refunds`] summed rather than a
//!   status to read.
//! - **A declined card is not [`Status::Failed`](kasapay_core::Status::Failed).**
//!   Stripe puts such a PaymentIntent back to `requires_payment_method`, which
//!   arrives as [`Status::RequiresAction`](kasapay_core::Status::RequiresAction)
//!   — honest, because the payer can try another card, but a caller waiting
//!   for `Failed` from Stripe waits forever.
//! - Anything this crate does not model is reachable through
//!   [`Stripe::client`], which hands back the `async-stripe` client itself.
//!
//! # Retrying
//!
//! Safe. `ChargeRequest::idempotency_key` is sent as Stripe's
//! `Idempotency-Key`, so replaying a charge with the same key takes the money
//! once. Without a key, a retry is a second charge.
//!
//! # Which `async-stripe`
//!
//! Exactly `1.0.0-rc.8`, pinned rather than ranged. The 1.0 line is a
//! prerelease and a candidate has changed generated types before —
//! `PaymentIntent` gained a public field between rc.6 and rc.7 — so which one
//! this is built against is not left to resolution.
//!
//! It costs a caller one thing: a crate that depends on `async-stripe` itself
//! has to be on the same candidate, because two exact pins at different
//! candidates cannot resolve together. [`Stripe::client`] hands the client back
//! so that reaching what this crate does not model needs no second dependency.
//! The pin becomes a range the day 1.0.0 ships.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{ChargeRequest, Currency, Money, OrderRef, Provider, Secret};
//! use kasapay_stripe::Stripe;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let stripe = Stripe::new(&Secret::new(std::env::var("STRIPE_SECRET_KEY")?));
//!
//! let request = ChargeRequest::builder(
//!     OrderRef::new("ord-2026-0001"),
//!     Money::parse("19.99", Currency::Usd)?,
//! )
//! .description("one coffee")
//! .build()?;
//!
//! let charge = stripe.charge(&request).await?;
//! println!("{:?} {:?}", charge.status, charge.next_action);
//! # Ok(())
//! # }
//! ```
//!
//! [`async-stripe`]: https://docs.rs/async-stripe

mod client;
mod convert;

#[doc(inline)]
pub use crate::client::{DEFAULT_TIMEOUT, ORDER_METADATA_KEY, Refund, RefundState, Stripe};
