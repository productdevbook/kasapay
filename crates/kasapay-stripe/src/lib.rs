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
//!   payment that was never captured. Both are on the shared trait, and the
//!   inherent methods are what it delegates to.
//! - A refund's `reason` travels as metadata under [`REASON_METADATA_KEY`]:
//!   Stripe's own `reason` takes three fixed values and free text is not one
//!   of them.
//! - **A declined card is not [`Status::Failed`](kasapay_core::Status::Failed).**
//!   Stripe puts such a PaymentIntent back to `requires_payment_method`, which
//!   arrives as [`Status::RequiresAction`](kasapay_core::Status::RequiresAction)
//!   — honest, because the payer can try another card, but a caller waiting
//!   for `Failed` from Stripe waits forever.
//! - Anything this crate does not model is reachable through
//!   [`Stripe::client`], which hands back the `async-stripe` client itself.
//!
//! # Webhooks
//!
//! [`Stripe::with_webhook_secret`] holds the endpoint's `whsec_…` — a
//! different secret from the API key — and
//! [`Provider::verify_webhook`](kasapay_core::Provider::verify_webhook) checks
//! the HMAC in constant time **and** the timestamp against
//! [`DEFAULT_TOLERANCE`], because a Stripe signature never expires on its own
//! and a body captured off the wire would otherwise verify a week later.
//!
//! # Retrying
//!
//! Safe. `ChargeRequest::idempotency_key` is sent as Stripe's
//! `Idempotency-Key`, so replaying a charge with the same key takes the money
//! once. Without a key, a retry is a second charge.
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
mod webhook;

#[doc(inline)]
pub use crate::client::{DEFAULT_TIMEOUT, ORDER_METADATA_KEY, REASON_METADATA_KEY, Stripe};
#[doc(inline)]
pub use crate::webhook::DEFAULT_TOLERANCE;
