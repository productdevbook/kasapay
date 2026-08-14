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
//! - **A declined card is not [`Status::Failed`](kasapay_core::Status::Failed).**
//!   Stripe puts such a PaymentIntent back to `requires_payment_method`, which
//!   arrives as [`Status::RequiresAction`](kasapay_core::Status::RequiresAction)
//!   — honest, because the payer can try another card, but a caller waiting
//!   for `Failed` from Stripe waits forever.
//! - Anything this crate does not model is reachable through
//!   [`Stripe::client`], which hands back the `async-stripe` client itself.
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
