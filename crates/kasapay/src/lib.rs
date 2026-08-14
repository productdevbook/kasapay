//! One payment API over any payment provider.
//!
//! Write against [`Provider`] and which provider takes the money becomes a
//! deployment decision rather than a rewrite. Stripe and iyzico ship with this
//! workspace; a provider that lives elsewhere is a first-class one — implement
//! [`Provider`], name it with [`ProviderId::new`]. Everything a caller needs is
//! re-exported here; the bundled adapters are behind features, one each.
//!
//! ```toml
//! kasapay = { version = "0.0.1", features = ["stripe", "iyzico"] }
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
//! # fn pick(
//! #     id: ProviderId,
//! #     stripe: kasapay::stripe::Stripe,
//! #     iyzico: kasapay::iyzico::in_store::Client,
//! # )
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
    Capabilities, Charge, ChargeRequest, ChargeRequestBuilder, ChargeRequestError, Currency, Error,
    ErrorKind, Id, IdKind, IdSource, IdempotencyKey, Money, MoneyError, NextAction, OrderRef,
    PaymentId, Provider, ProviderId, Raw, Secret, Status, UnknownCurrency, async_trait, kind,
};

#[cfg(feature = "iyzico")]
#[doc(inline)]
pub use kasapay_iyzico as iyzico;
#[cfg(feature = "paytr")]
#[doc(inline)]
pub use kasapay_paytr as paytr;
#[cfg(feature = "stripe")]
#[doc(inline)]
pub use kasapay_stripe as stripe;
