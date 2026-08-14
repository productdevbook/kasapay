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
//!   [`Client::create_user`] is where one comes from, and a user who exists is
//!   not yet a user who can charge — they have to be enrolled with a bank.
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
//! # Which version of `/crypt/decrypt` this is
//!
//! **v3.** iyzico documents the operation twice, and the two are not two
//! versions of one endpoint to choose between: they belong to two separate
//! integrations, `App2App V2` and `App2App V3`, each with its own documentation
//! section and its own paths. Only `/crypt/decrypt` is spelled the same in
//! both. Everything else differs — V2 lists users at
//! `/v2/in-store/user-info/list` and starts a payment at `/v2/in-store/payment`,
//! against V3's `/v3/in-store/user/list` and `/v3/in-store/payment/init` — so
//! pointing [`Config`] at the v2 base to reach v2's decrypt would break every
//! other call this client makes. This crate implements V3 throughout.
//!
//! `specs/iyzico/in-store/latest.yaml` carrying both is not evidence of a
//! choice either. The v2 entry comes from one OpenAPI fragment on the page
//! titled *In-Store API V3*, whose six sibling fragments all declare a
//! `/v3/in-store` server while that one declares `/v2/in-store` — and the
//! merge script derives each path from its own fragment's server. All seven
//! say `version: 3.0`. The v3 fragment on iyzico's dedicated decrypt page
//! declares `/v3/in-store` for both sandbox and production.
//!
//! iyzico nowhere says V2 is withdrawn, and its pages are still published.
//! What is settled is that v3 is this client's version and v2 is not reachable
//! by reconfiguring it.
//!
//! # `currencyCode` is a number
//!
//! The one response iyzico publishes in full answers `"currencyCode": "0949"`
//! — ISO 4217's numeric code for lira, zero-padded, where the rest of their
//! API writes `TRY`. Both are read here. Nothing else numeric is: this API
//! settles in lira only, so another number is a surprise worth an error rather
//! than a guess.
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
//! [`Client::refund`] therefore sends both. Whichever name is real carries the
//! amount, and a server strict enough to reject the other says so instead of
//! giving the money back.
//!
//! # Authentication
//!
//! Three plain headers — `x-api-key`, `x-secret-key`, `x-merchant-id` — rather
//! than the [`IYZWSv2`](crate::Credentials) signing the rest of iyzico uses.
//! That comes from prose on the overview page; the fragments themselves declare
//! only `x-api-key`.
//!
//! There is no OAuth2 alternative to them, whatever the specs look like.
//! `specs/iyzico/in-store/latest.yaml` carries `/in-store/oauth2/authorize`,
//! `/in-store/oauth2/token` and `/in-store/oauth2/token/refresh`, and they are
//! filed under In-Store because that file is grouped by path. All three come
//! from one page — Physical POS → Terminal API Integration → Login — and their
//! fragments are titled *Terminal API – Outside Flow*, as are the ones defining
//! the VUK 509 `/v2/terminal-host/*` services; every fragment of this API is
//! titled *iyzico In-Store API*. iyzico says of the `access_token` they issue
//! that it is "used as Bearer Token in Terminal Host services" — a cash
//! register driving a physical POS device, which is not this. No `CepPOS`
//! `App2App` page mentions OAuth in either language.
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
pub use crate::in_store::client::{Client, Config, Enrollment, User};
