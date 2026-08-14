//! One payment API over Stripe and iyzico.
//!
//! Write against [`Provider`] and the provider becomes a deployment decision
//! rather than a rewrite. Everything a caller needs is re-exported here;
//! the adapters are behind features, one per provider.
//!
//! ```toml
//! kasapay = { version = "0.1", features = ["stripe", "iyzico"] }
//! ```
//!
//! # The one thing to understand first
//!
//! [`Provider::charge`] does not mean the money moved. It returns a [`Charge`]
//! whose [`Status`] is often [`Status::RequiresAction`], with a [`NextAction`]
//! saying what the payer must do — confirm in the browser for Stripe, follow a
//! deep link into iyzico's app for iyzico. Treating a returned `Charge` as a
//! completed payment is the mistake this crate is shaped to prevent.
//!
//! # Choosing a provider at runtime
//!
//! ```no_run
//! use std::sync::Arc;
//! use kasapay::{Provider, ProviderId};
//!
//! # #[cfg(all(feature = "stripe", feature = "iyzico"))]
//! # fn pick(id: ProviderId, stripe: kasapay::stripe::Stripe, iyzico: kasapay::iyzico::Iyzico)
//! # -> Option<Arc<dyn Provider>> {
//! match id {
//!     ProviderId::STRIPE => Some(Arc::new(stripe)),
//!     ProviderId::IYZICO => Some(Arc::new(iyzico)),
//!     _ => None,
//! }
//! # }
//! ```

#[doc(inline)]
pub use kasapay_core::{
    Charge, ChargeRequest, ChargeRequestBuilder, ChargeRequestError, Currency, Error, ErrorKind,
    IdempotencyKey, Money, MoneyError, NextAction, OrderRef, PaymentId, Provider, ProviderId,
    Secret, Status, UnknownCurrency,
};

#[cfg(feature = "iyzico")]
#[doc(inline)]
pub use kasapay_iyzico as iyzico;
#[cfg(feature = "stripe")]
#[doc(inline)]
pub use kasapay_stripe as stripe;
