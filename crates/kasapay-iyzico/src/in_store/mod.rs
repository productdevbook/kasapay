//! The In-Store API v3 — iyzico's counter-side flow.
//!
//! The merchant starts a payment, iyzico answers a deep link, the payer
//! completes it in iyzico's own app, and the result arrives at a callback
//! address the merchant supplied per request. So
//! [`Provider::charge`](kasapay_core::Provider::charge) here returns
//! [`Status::RequiresAction`](kasapay_core::Status::RequiresAction), never a
//! captured payment.
//!
//! Three things follow, and they are the ones that surprise callers:
//!
//! - `ChargeRequest::customer` is iyzico's `userId` and is **required**.
//! - `ChargeRequest::return_url` becomes the `x-callback-url` header and is
//!   **required**.
//! - This API settles in Turkish lira only; any other currency is
//!   [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported).
//!
//! # Finishing a payment
//!
//! The redirect is not the end. When the payer is done, iyzico posts an
//! encrypted `data` blob to the `x-callback-url` the charge carried, and
//! [`Client::decrypt_callback`] is what opens it — the `continuation` on the
//! returned [`NextAction::Redirect`](kasapay_core::NextAction::Redirect) is the
//! `paymentSessionToken` that call needs. Polling
//! [`charge_status`](kasapay_core::Provider::charge_status) works, but the
//! callback is what the flow is built around.
//!
//! iyzico documents `/crypt/decrypt` under both `/v3/in-store` and
//! `/v2/in-store` with identical bodies, and does not say which is current.
//! This crate calls whichever the configured base points at.
//!
//! # What a status query can say
//!
//! `/payment/query` documents no status field, so
//! [`charge_status`](kasapay_core::Provider::charge_status) reads the
//! receipt's approval flags: an approved or refundable payment is
//! [`Captured`](kasapay_core::Status::Captured) and anything else is
//! [`Pending`](kasapay_core::Status::Pending). **A refused payment is
//! therefore indistinguishable from one the payer has not finished.** The
//! decrypted callback does distinguish them, and is the better source.
//!
//! This mapping has not been checked against a live account.
//!
//! # A partial refund carries its amount twice
//!
//! iyzico documents the field two ways and contradicts itself on one page: the
//! prose says `refundAmount`, the OpenAPI fragment beside it says
//! `refundPrice`. The field is optional, so sending only the wrong name is not
//! an error — it is a **full refund where a partial one was asked for**.
//!
//! [`Client::start_refund`] therefore sends both. Whichever name is real carries the
//! amount, and a server strict enough to reject the other says so instead of
//! giving the money back.
//!
//! # Authentication
//!
//! Three plain headers — `x-api-key`, `x-secret-key`, `x-merchant-id` — rather
//! than the [`IYZWSv2`](crate::Credentials) signing the rest of iyzico uses.
//! That comes from prose on the overview page; the fragments themselves declare
//! only `x-api-key`, and also document an OAuth2 flow this crate does not use.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{ChargeRequest, Currency, Money, OrderRef, Provider};
//! use kasapay_iyzico::in_store;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzico = in_store::Client::new(in_store::Config::sandbox(
//!     "api-key",
//!     "secret-key",
//!     "merchant-id",
//! ))?;
//!
//! let request = ChargeRequest::builder(
//!     OrderRef::new("ord-2026-0001"),
//!     Money::parse("149.90", Currency::Try)?,
//! )
//! .customer("kasiyer-7")
//! .return_url("https://example.test/iyzico/callback".parse()?)
//! .build()?;
//!
//! let charge = iyzico.charge(&request).await?;
//! println!("send the payer to {:?}", charge.next_action);
//! # Ok(())
//! # }
//! ```

mod client;
mod wire;

#[doc(inline)]
pub use crate::in_store::client::{Client, Config};
