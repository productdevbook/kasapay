//! PayPal, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! Over PayPal's [Orders v2 API][orders], from
//! [their own OpenAPI document](https://github.com/paypal/paypal-rest-api-specifications),
//! which `specs/paypal/` records a subset of. PayPal hosts the checkout, so no
//! card data crosses this process:
//! [`PayPal::create_order`] opens an order and
//! answers a [`Charge`](kasapay_core::Charge) carrying the address to send the
//! payer to; [`charge_status`](kasapay_core::Provider::charge_status) reads it
//! back by the order id PayPal issued.
//!
//! # This crate is the spine, not the whole API
//!
//! Three calls: create an order, read it back, capture it. Deliberately
//! smaller than PayPal's Orders v2 surface, because a smaller adapter that is
//! correct beats a complete one that guesses:
//!
//! - **Only `intent: CAPTURE`.** PayPal decides at order creation whether a
//!   later capture takes the whole order (`CAPTURE`) or first places a hold
//!   through a separate Authorizations resource that is captured after
//!   (`AUTHORIZE`) — the same choice Mollie's `captureMode` makes, and
//!   `ChargeRequest` has no field for either. Unlike Mollie, though,
//!   **neither intent captures on its own**: every PayPal order needs an
//!   explicit follow-up call after the payer approves regardless, so
//!   `AUTHORIZE` buys a longer hold rather than skipping a step this crate
//!   would otherwise need. Its Authorizations resource
//!   (`/v2/payments/authorizations/{id}/*`) is not here.
//! - **No refunds, no shipping/items, no webhooks, no PATCH.** The Orders API
//!   is more than sixty operations across create/read/update, confirm,
//!   authorize, capture, and package tracking; this crate maps three.
//! - **No Vault.** PayPal's saved-instrument story is a separate, versioned
//!   API (`/v3/vault/*`). [`Provider::instruments`](kasapay_core::Provider::instruments)
//!   answers [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported)
//!   because of it, and so does [`Provider::cancel`](kasapay_core::Provider::cancel)
//!   for an unrelated reason — see below.
//!
//! # `Provider::cancel` is always refused
//!
//! **PayPal's Orders v2 API has no cancel or void operation.** Its paths are
//! create, read, `PATCH` to edit, confirm-payment-source, authorize, capture,
//! and shipment tracking — nothing that withdraws an order. An order the
//! payer never approves is simply left; PayPal's own prose says it becomes
//! eligible for deletion once it has aged, without this crate's sources
//! naming a fixed duration, and there is no call to hurry that along. Four
//! card-first providers in this workspace all answer a real cancel; PayPal is
//! the one that shows `Provider::cancel` assuming every provider has
//! something to call.
//!
//! # OAuth2, and why the client renews its own token
//!
//! PayPal authenticates with client-credentials: `client_id` and
//! `client_secret` buy a bearer token at `POST /v1/oauth2/token`, and this
//! client fetches and caches one itself rather than taking it from the
//! caller — a deliberately different choice from `kasapay_iyzico::terminal`,
//! whose token the caller owns and which is never renewed automatically. Both
//! are reasoned choices about the same question, for opposite reasons: read
//! [`PayPal`]'s own documentation for why getting a stateless
//! bearer token has none of the replay ambiguity a physical card-present sale
//! does, and why this client still does not retry a failed *business* call on
//! its own.
//!
//! # Currencies
//!
//! PayPal's [currency codes reference][codes] lists twenty-five. kasapay
//! names nine, and the overlap is seven — **PayPal takes neither Turkish lira
//! nor Kuwaiti dinar**, the same two Mollie refuses, for the same reason: both
//! are simply absent from PayPal's list. Refused in
//! `convert::currency` before a socket opens.
//!
//! # `ChargeRequest` fields PayPal has nowhere to put
//!
//! - **`customer`** is not read. Orders v2 names no payer identity outside
//!   Vault, which is out of scope.
//! - **`metadata`** is not sent. PayPal's `purchase_unit` has no free-form
//!   key/value bag; `order` — kasapay's own reference — goes in the one field
//!   that comes closest, `custom_id`, and there is nowhere left for more.
//! - **`return_url`**, when set, is sent as both PayPal's `return_url` **and**
//!   `cancel_url` — `ChargeRequest` has a field for where the payer goes back
//!   to and none for where they go back to on giving up, so this crate reuses
//!   the one URL for both rather than sending a mismatched pair. Which
//!   happened is what [`Provider::charge_status`](kasapay_core::Provider::charge_status)
//!   is for.
//!
//! # Where an order's status actually comes from
//!
//! **Every documented example response in PayPal's own OpenAPI document,
//! across creating, reading and capturing an order, omits the order's own
//! top-level `status` field** — even though the schema declares one and the
//! `Prefer` header's prose promises a minimal response includes it. So this
//! crate reads a status from whichever of four places actually carries one on
//! a given answer: a capture's own status, an authorization's own status, the
//! order's top-level status when an example finally does carry it, or —
//! failing all three — an `approve`/`payer-action` link with nothing captured
//! yet. `client::into_charge`'s own documentation has the full order and
//! reasoning. **This has not been checked against a live sandbox account**;
//! see "Unverified" below.
//!
//! # PayPal's capture status folds in refunds
//!
//! `capture_status` names `PARTIALLY_REFUNDED` and `REFUNDED` alongside
//! `COMPLETED`, `DECLINED`, `PENDING` and `FAILED` — PayPal is the first
//! provider in this workspace whose capture carries a refund outcome inside
//! the same enum as its own status, rather than as a separate figure the way
//! Mollie's `amountRefunded` or Stripe's refund list are. Both refunded
//! states still read [`Status::Captured`](kasapay_core::Status::Captured)
//! here — the money was taken, which is the fact `Status` has a word for —
//! and the more specific answer is on [`Charge::raw`](kasapay_core::Charge::raw).
//!
//! # Retrying
//!
//! **Creating an order is safe with a key.**
//! [`ChargeRequest::idempotency_key`](kasapay_core::ChargeRequest::idempotency_key)
//! is sent as PayPal's `PayPal-Request-Id`; PayPal documents returning the
//! cached first answer for a repeated key rather than creating twice.
//!
//! **Capturing is not safe through [`Provider::capture`](kasapay_core::Provider::capture)
//! at all.** PayPal takes the same `PayPal-Request-Id` on the capture
//! endpoint, and replaying a capture without one can capture twice — but
//! `Provider::capture`'s signature carries no idempotency key for the trait
//! to send. [`PayPal::capture_order`] is the
//! one place a caller working directly against this crate can pass one; going
//! through [`Provider`](kasapay_core::Provider) alone, the only safe answer
//! after a failure whose outcome is unknown is
//! [`Provider::charge_status`](kasapay_core::Provider::charge_status), never
//! a second [`Provider::capture`](kasapay_core::Provider::capture).
//!
//! # Unverified against a live account
//!
//! Everything above that is read off PayPal's prose or their OpenAPI
//! document's schema rather than a documented example: the status-resolution
//! order, `403` reading as [`ErrorKind::Auth`](kasapay_core::ErrorKind::Auth)
//! rather than a refusal of its own kind, and the account-level default
//! `return_url`/`cancel_url` a caller gets by leaving `ChargeRequest::return_url`
//! unset. Check these first against a sandbox account.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{ChargeRequest, Currency, Money, NextAction, OrderRef, Provider, Secret};
//! use kasapay_paypal::{Config, PayPal};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let paypal = PayPal::new(Config::sandbox(
//!     Secret::new(std::env::var("PAYPAL_CLIENT_ID")?),
//!     Secret::new(std::env::var("PAYPAL_CLIENT_SECRET")?),
//! ))?;
//!
//! let request = ChargeRequest::builder(
//!     OrderRef::new("ord-2026-0001"),
//!     Money::parse("10.00", Currency::Usd)?,
//! )
//! .return_url("https://webshop.example/order/12345".parse()?)
//! .build()?;
//!
//! let charge = paypal.charge(&request).await?;
//! if let Some(NextAction::Redirect { url, .. }) = &charge.next_action {
//!     println!("send the payer to {url}");
//! }
//!
//! // Only after the payer has approved it at PayPal.
//! let id = charge.id.as_ref().ok_or("PayPal names every order")?;
//! let captured = paypal.capture(id, None).await?;
//! println!("{:?}", captured.status);
//! # Ok(())
//! # }
//! ```
//!
//! [orders]: https://developer.paypal.com/docs/api/orders/v2/
//! [codes]: https://developer.paypal.com/api/rest/reference/currency-codes/

mod client;
mod convert;
mod wire;

use kasapay_core::ProviderId;

/// PayPal.
pub const PAYPAL: ProviderId = ProviderId::new("paypal");

#[doc(inline)]
pub use crate::client::{Config, PayPal};
