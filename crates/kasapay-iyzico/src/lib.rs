//! iyzico In-Store API v3, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! # What this API is
//!
//! In-Store is the counter-side flow: the merchant starts a payment, iyzico
//! answers a deep link, the payer completes it in iyzico's own app, and the
//! result arrives at a callback address the merchant supplied per request. So
//! [`Provider::charge`](kasapay_core::Provider::charge) here returns
//! [`Status::RequiresAction`](kasapay_core::Status::RequiresAction), never a
//! captured payment.
//!
//! Three things follow, and they are the ones that surprise callers:
//!
//! - `ChargeRequest::customer` is iyzico's `userId` and is **required**.
//! - `ChargeRequest::return_url` becomes the `x-callback-url` header and is
//!   **required**.
//! - The In-Store API settles in Turkish lira only; any other currency is
//!   [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported).
//!
//! # Where the types come from
//!
//! iyzico publishes no OpenAPI document. The ones in `specs/iyzico/` are
//! reassembled from the per-endpoint fragments embedded across their whole
//! documentation site, one file per product area, dated on the day they were
//! taken. They record what was documented, not a contract iyzico offers.
//!
//! This crate covers `urunler/ceppos-app2app` only. iyzico has fifteen other
//! product areas documented the same way — subscriptions, marketplace, mass
//! payout, card storage — and none of them are implemented here yet.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{ChargeRequest, Currency, Money, OrderRef, Provider};
//! use kasapay_iyzico::{Config, Iyzico};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzico = Iyzico::new(Config::sandbox("api-key", "secret-key", "merchant-id"))?;
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
pub use crate::client::{Config, Iyzico};
