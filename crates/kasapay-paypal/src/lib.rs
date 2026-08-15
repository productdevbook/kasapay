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
//! # This crate is the spine, plus what a refund and a hold need
//!
//! Create an order, read it back, capture it, refund a capture, place and
//! capture a hold. Still deliberately smaller than PayPal's Orders and
//! Payments surface, because a smaller adapter that is correct beats a
//! complete one that guesses:
//!
//! - **Both intents now.** PayPal decides at order creation whether a later
//!   capture takes the whole order (`CAPTURE`, [`PayPal::create_order`]) or
//!   first places a hold through the Authorizations resource that is
//!   captured after (`AUTHORIZE`, [`PayPal::authorize`]) — the same choice
//!   Mollie's `captureMode` makes, and `ChargeRequest` has no field for
//!   either, so each is its own method. Unlike Mollie, though, **neither
//!   intent captures on its own**: every PayPal order needs an explicit
//!   follow-up call after the payer approves regardless — either
//!   [`PayPal::capture_order`] or [`PayPal::authorize_order`] then
//!   [`PayPal::capture_authorization`] — so `AUTHORIZE` buys a longer hold
//!   rather than skipping a step this crate would otherwise need. What is
//!   still not here: releasing a hold without capturing it. PayPal documents
//!   `POST /v2/payments/authorizations/{id}/void` for that, and it is keyed
//!   by the authorization's own id rather than the order's — the same split
//!   [`PayPal::capture_authorization`] has — but this crate does not call
//!   it. See [`Provider::cancel`](kasapay_core::Provider::cancel)'s own
//!   documentation on [`PayPal`] for what that leaves a caller with.
//! - **Refunds are against a capture, not an order.** [`PayPal::refund`]
//!   is `POST /v2/payments/captures/{id}/refund`, keyed by [`CaptureId`] —
//!   the same split `kasapay_mollie::Mollie::refund` draws between a
//!   payment and its capture. Whole or partial, and repeatable up to what
//!   was captured.
//! - **No shipping/items, no webhooks, no PATCH, no listing refunds.** The
//!   Orders and Payments APIs together are well over sixty operations across
//!   create/read/update, confirm, capture, authorize, void, reauthorize,
//!   refund and package tracking; this crate maps eight.
//! - **No Vault.** PayPal's saved-instrument story is a separate, versioned
//!   API (`/v3/vault/*`). [`Provider::instruments`](kasapay_core::Provider::instruments)
//!   answers [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported)
//!   because of it.
//!
//! # `Provider::cancel` is always refused
//!
//! **`/v2/checkout/orders` itself has no cancel or void operation** — create,
//! read, `PATCH` to edit, confirm-payment-source, authorize, capture, and
//! shipment tracking, nothing that withdraws an order by its own id. An order
//! the payer never approves is simply left; PayPal's own prose says it
//! becomes eligible for deletion once it has aged, without this crate's
//! sources naming a fixed duration, and there is no call to hurry that
//! along. Four card-first providers in this workspace all answer a real
//! cancel; PayPal is the one that shows `Provider::cancel` assuming every
//! provider has something to call.
//!
//! This is still true with `intent: AUTHORIZE` in the picture: the
//! Authorizations resource does document a void, but it releases a hold by
//! the hold's own id, and [`Provider::cancel`](kasapay_core::Provider::cancel)
//! takes only an order id. This crate does not implement it, so a caller who
//! places a hold with [`PayPal::authorize_order`] and decides not to take it
//! has no call here to release it — the same honest gap `Provider::cancel`
//! already had for a plain `intent: CAPTURE` order.
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
//! This enum is what proved PayPal's API always supported partial and
//! repeated refunds even while this crate had no call that made one;
//! [`PayPal::refund`] is that call now, and
//! [`Capabilities::partial_refund`](kasapay_core::Capabilities::partial_refund)
//! and [`Capabilities::repeated_refund`](kasapay_core::Capabilities::repeated_refund)
//! read `true`.
//!
//! [`PayPal::refund`]'s own answer carries a different, smaller enum —
//! `refund_status`: `PENDING`, `COMPLETED`, `FAILED`, `CANCELLED` — modelled
//! as [`RefundState`] rather than folded into
//! [`Status`](kasapay_core::Status), because nothing calls
//! [`Provider::charge_status`](kasapay_core::Provider::charge_status) on a
//! refund id.
//!
//! # Retrying
//!
//! **Creating an order is safe with a key.**
//! [`ChargeRequest::idempotency_key`](kasapay_core::ChargeRequest::idempotency_key)
//! is sent as PayPal's `PayPal-Request-Id`; PayPal documents returning the
//! cached first answer for a repeated key rather than creating twice.
//!
//! **Capturing is safe with a key, through [`Provider::capture`](kasapay_core::Provider::capture)
//! now too.** PayPal takes the same `PayPal-Request-Id` on the capture
//! endpoint that it does on opening an order, and `Provider::capture`'s own
//! `idempotency` argument feeds straight into [`PayPal::capture_order`]'s
//! `request_id` — [`PayPal::capture_order`] stays exposed for a caller who
//! wants that parameter without going through
//! [`ChargeRequest`](kasapay_core::ChargeRequest)'s shape at all. Without a
//! key, replaying a capture — through either call — can capture the same
//! order twice, and the only safe answer after a failure whose outcome is
//! unknown is
//! [`Provider::charge_status`](kasapay_core::Provider::charge_status), never
//! a second capture sent without one.
//!
//! **[`PayPal::authorize_order`], [`PayPal::capture_authorization`] and
//! [`PayPal::refund`] all take a `request_id` of their own**, sent the same
//! way — none of them is reachable through [`Provider`](kasapay_core::Provider)
//! at all, so there is no trait signature to work around for them. Money
//! leaving is why [`PayPal::refund`]'s matters most: PayPal takes
//! `PayPal-Request-Id` on the refund endpoint exactly as it does on capture,
//! and a retried refund with none can give the money back twice.
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
//! [`PayPal::authorize`], [`PayPal::authorize_order`],
//! [`PayPal::capture_authorization`] and [`PayPal::refund`] add to that list
//! rather than replacing it — every request and response shape they use is a
//! documented example, but none of the four has been called against a
//! sandbox account either.
//!
//! **One combination PayPal documents each half of and never together: a
//! retried refund, with an omitted amount, against a capture that is itself
//! partially refunded.** Their `refund_request` schema says an omitted
//! `amount` refunds `captured amount - previous refunds` — computed at the
//! time the request is processed — and their `PayPal-Request-Id` prose says
//! a repeated key answers the same cached response rather than processing
//! again. Both are documented; **which one wins when a caller retries an
//! omitted-amount refund after some other refund has landed in between is
//! not** — whether the retry replays the first request's own computed
//! amount, or is treated as a fresh request and recomputes against a
//! smaller remainder. [`PayPal::refund`] passes `request_id` straight
//! through either way and does not guess at the answer.
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
//! let captured = paypal.capture(id, None, None).await?;
//! println!("{:?}", captured.status);
//!
//! // Giving some of it back later — against the capture, not the order.
//! let capture = kasapay_paypal::capture_id(&captured)?;
//! let refund = paypal.refund(&capture, None, None).await?;
//! println!("{:?}", refund.state);
//! # Ok(())
//! # }
//! ```
//!
//! [orders]: https://developer.paypal.com/docs/api/orders/v2/
//! [codes]: https://developer.paypal.com/api/rest/reference/currency-codes/

mod client;
mod convert;
pub mod id;
mod wire;

use kasapay_core::ProviderId;

/// PayPal.
pub const PAYPAL: ProviderId = ProviderId::new("paypal");

#[doc(inline)]
pub use crate::client::{
    Capture, Config, PayPal, Refund, RefundState, authorization_id, capture_id,
};
#[doc(inline)]
pub use crate::id::{AuthorizationId, CaptureId, RefundId};
