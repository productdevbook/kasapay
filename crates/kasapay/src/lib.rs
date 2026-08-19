//! One payment API over any payment provider.
//!
//! Write against [`Provider`] and which provider takes the money becomes a
//! deployment decision rather than a rewrite. Five ship with this workspace —
//! Stripe, iyzico, PayTR, Mollie and PayPal — and a provider that lives
//! elsewhere is a first-class one: implement [`Provider`], name it with
//! [`ProviderId::new`]. Everything a caller needs is re-exported here; the
//! bundled adapters are behind features, one each.
//!
//! ```toml
//! kasapay = { version = "0.0.5", features = ["stripe", "iyzico"] }
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
//! # What the trait answers
//!
//! [`Provider`] is `charge`, `resume`, `charge_status`, `capture`, `cancel`,
//! `refund`, `lookup` and `instruments`, and [`Capabilities`] says which of
//! them a given provider actually does — before there is a payment to ask about. A
//! capability that says yes and a call that then fails is a bug in the
//! adapter.
//!
//! **Giving money back is [`Provider::refund`]**, which answers a [`Refund`]
//! with its own life: its own identifier where the provider issues one, its
//! own [`RefundStatus`], and at iyzico's counter its own [`NextAction`],
//! because there the payer approves the refund in an app. [`Status`] has no
//! `Refunded` and will not grow one — no provider reports a refund as a
//! payment's status.
//!
//! **The call whose answer never arrived is [`Provider::lookup`]**, keyed by
//! [`OrderRef`] — the caller's own reference, which they had before they sent
//! anything. `Ok(None)` means the provider has no record and the charge can
//! safely be sent again. Two of the five can answer it; the rest say what to
//! do instead.
//!
//! # What arrives without being asked for
//!
//! [`Webhook`] is the second trait: it takes the headers and bytes of a
//! [`Delivery`], shows they are the provider's, and says what they mean as an
//! [`Event`].
//!
//! ```no_run
//! use kasapay::{Delivery, EventKind, Webhook};
//!
//! # async fn handle(verifier: &dyn Webhook, headers: &[(&str, &str)], body: &[u8]) -> &'static str {
//! match verifier.verify(&Delivery::new(headers, body)).await {
//!     // The identifier goes into a unique index before anything ships: the
//!     // second delivery of an event must collide rather than ship twice.
//!     Ok(event) if event.kind == EventKind::Captured => "ship it",
//!     // Not an error. A provider adding an event type is normal, and
//!     // refusing one earns days of redeliveries for something nobody wanted.
//!     Ok(_) => "acknowledged",
//!     // Do not act on it — and still answer the provider what the provider
//!     // documents. Those are two different questions.
//!     Err(_) => "acknowledged",
//! }
//! # }
//! ```
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
    Address, BasketItem, Buyer, Capabilities, Charge, ChargeRequest, ChargeRequestBuilder,
    ChargeRequestError, Currency, Delivery, Error, ErrorKind, Event, EventId, EventKind, Id,
    IdKind, IdSource, IdempotencyKey, Instrument, InstrumentId, ItemKind, Money, MoneyError,
    NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, Refund, RefundId, RefundReason,
    RefundRequest, RefundRequestBuilder, RefundRequestError, RefundStatus, RepeatedHeader, Secret,
    Sequence, Status, UnknownCurrency, Webhook, async_trait, kind,
};

#[cfg(feature = "iyzico")]
#[doc(inline)]
pub use kasapay_iyzico as iyzico;
#[cfg(feature = "mollie")]
#[doc(inline)]
pub use kasapay_mollie as mollie;
#[cfg(feature = "paypal")]
#[doc(inline)]
pub use kasapay_paypal as paypal;
#[cfg(feature = "paytr")]
#[doc(inline)]
pub use kasapay_paytr as paytr;
#[cfg(feature = "stripe")]
#[doc(inline)]
pub use kasapay_stripe as stripe;
