//! Mollie, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! Mollie hosts the checkout, so no card data crosses this process.
//! [`Provider::charge`](kasapay_core::Provider::charge) opens a payment and
//! answers a [`Charge`](kasapay_core::Charge) carrying the address to send the
//! payer to; [`charge_status`](kasapay_core::Provider::charge_status) reads it
//! back by the `tr_…` identifier Mollie issued.
//!
//! Over Mollie's [Payments API], from
//! [their own OpenAPI document](https://github.com/mollie/openapi), which
//! `specs/mollie/` dates and records without keeping a copy of — theirs is
//! licensed CC-BY-NC-SA and this workspace is MIT.
//!
//! # Amounts are decimal strings, and are still not floats
//!
//! Mollie writes money as `{"currency": "EUR", "value": "10.00"}`. The value is
//! a string on purpose — their own document says to send it "as a string and
//! not as a float, to ensure the correct number of decimals are passed" — and
//! it is written here from [`Money`](kasapay_core::Money)'s integer count of
//! minor units. `10.00` and never `10.0`; `1200` and never `1200.00` for yen,
//! which Mollie documents as having no decimal places at all.
//!
//! # Mollie takes neither lira nor Kuwaiti dinar
//!
//! Mollie's [multicurrency table][] is where their list comes from, and this
//! crate sends seven of it: USD, EUR, GBP, JPY, RUB, CHF and NOK. Anything
//! else kasapay names — Turkish lira and the Kuwaiti dinar included, which is
//! to say the home currency of the two Turkish providers here and the one
//! currency with three decimal places — is
//! [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported) **before a
//! socket opens**, rather than a request with a currency Mollie will not read.
//!
//! [multicurrency table]: https://docs.mollie.com/docs/multicurrency
//!
//! Mollie's OpenAPI document enumerates no currencies at all — `currency` is
//! described there as "a three-character ISO 4217 currency code" and nothing
//! more — so the list comes from their
//! [multicurrency page](https://docs.mollie.com/docs/multicurrency), which is
//! also where the decimal places come from.
//!
//! # `charge` needs two fields `ChargeRequest` calls optional
//!
//! Mollie requires a `description` and a `redirectUrl` on every payment. Both
//! are `Option` on [`ChargeRequest`](kasapay_core::ChargeRequest), and a
//! request missing either is
//! [`ErrorKind::InvalidRequest`](kasapay_core::ErrorKind::InvalidRequest)
//! here, before anything is sent.
//!
//! # A hold is [`Mollie::authorize`], not `charge`
//!
//! Mollie decides at creation whether a payment is captured automatically or
//! held, and `ChargeRequest` has no field that says which. So
//! [`Provider::charge`](kasapay_core::Provider::charge) opens a payment that is
//! captured the moment the payer finishes, and [`Mollie::authorize`] opens the
//! same payment with `captureMode: manual` for a later
//! [`capture`](kasapay_core::Provider::capture).
//!
//! # Cancelling and releasing are two different calls
//!
//! [`Provider::cancel`](kasapay_core::Provider::cancel) releases a hold —
//! Mollie's `POST /v2/payments/{id}/release-authorization`, which answers
//! `202 Accepted` with no body. It says Mollie will try; whether the hold
//! actually lifts is the issuing bank's decision, which is why the trait
//! answers [`ReleaseState::Accepted`](kasapay_core::ReleaseState) rather than
//! `Released` and why the payment is read back afterwards.
//!
//! Withdrawing a payment the payer never finished is the other call:
//! [`Mollie::cancel_payment`], `DELETE /v2/payments/{id}`, which answers the
//! cancelled payment. Whether a payment can be withdrawn at all depends on the
//! method the payer chose, and Mollie says which on `isCancelable`. Asking it
//! of a payment that is already authorised, or already paid, is Mollie's 422.
//!
//! # The webhook carries no signature, so the id must be fetched
//!
//! Mollie does not post the payment. It posts one form field — `id=tr_…` — to
//! the `webhookUrl` the payment was created with, and **nothing that proves the
//! request came from Mollie**: no signature, no shared secret, no timestamp.
//!
//! So the body of a webhook is not information. It is a prompt to call
//! [`charge_status`](kasapay_core::Provider::charge_status) with that id and
//! believe the answer, which arrives over TLS from Mollie's own host. Two
//! things follow for whoever writes the handler, the first of which
//! [`Mollie`]'s own [`Webhook::verify`](kasapay_core::Webhook::verify) does
//! for them:
//!
//! - **Act on nothing in the request but the id.** An amount, a status or an
//!   order reference posted to that address is whatever the sender typed.
//! - **A `tr_…` this merchant does not know is not an error to shout about.**
//!   Anybody can post any id, and Mollie itself answers
//!   [`ErrorKind::NotFound`](kasapay_core::ErrorKind::NotFound) for one that is
//!   not on this account.
//!
//! Answer 200 for both of those. Mollie retries a webhook that does not, and a
//! handler that returns 500 while it works out what to do is one Mollie will
//! call again.
//!
//! # An `Err` here has two meanings, and only one of them is a 200
//!
//! [`Webhook::verify`](kasapay_core::Webhook::verify) is the only one in this
//! workspace that makes a network call, because Mollie signs nothing and the
//! answer has to come from them. So its `Err` says either *this delivery is
//! not worth acting on* — an id Mollie does not know, a body that is not the
//! form they post — or *the check did not finish*, which is Mollie answering
//! `429` or `503` to the read-back.
//!
//! Answering 200 to the second acknowledges a delivery nobody read, and Mollie
//! has no reason to send it again. On a card payment the payer comes back to
//! the `redirectUrl` and the shop reads the payment anyway; on a bank transfer
//! or a direct debit nobody comes back, and that is a payment taken and never
//! learned about.
//!
//! [`Error::is_retryable`](kasapay_core::Error::is_retryable) is the
//! discrimination, and it is already exactly the right set: true for
//! `RateLimited`, `Transport` and `Provider`, false for `NotFound`,
//! `Malformed` and `Untrusted`. **Answer non-2xx where it is true** and let
//! Mollie redeliver; answer 200 for everything else.
//!
//! What Mollie does with a 200 is read off their prose rather than observed —
//! see `UNVERIFIED.md`.
//!
//! So [`Webhook::verify`](kasapay_core::Webhook::verify) here does not check a
//! signature — there is none to check. It reads the id out of the form body
//! and asks Mollie what that payment is, which is the only thing about a
//! Mollie delivery that can be trusted, and answers an
//! [`Event`](kasapay_core::Event) built from the answer rather than from the
//! request.
//!
//! # Nothing here says a payment was refunded
//!
//! Mollie's payment statuses have no refunded state: a fully refunded payment
//! still reads `paid`. [`Mollie::refunds`] is the list to sum, and Mollie
//! answers the figures directly as well, which is more than the other adapters
//! in this workspace get — `amountRefunded`,
//! `amountRemaining` and `amountCaptured` sit on the payment, and
//! [`Charge::raw`](kasapay_core::Charge::raw) is where they are read:
//!
//! ```
//! use kasapay_core::{Currency, Money, Raw};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // As Mollie answers a payment of 10.00 EUR with 5.95 of it given back.
//! let raw = Raw::from_text(
//!     r#"{"amount":{"currency":"EUR","value":"10.00"},
//!         "amountRefunded":{"currency":"EUR","value":"5.95"},
//!         "amountRemaining":{"currency":"EUR","value":"4.05"}}"#,
//! );
//!
//! let refunded = raw
//!     .text_at("/amountRefunded/value")
//!     .ok_or("no refunds on this payment")?;
//! assert_eq!(Money::parse(&refunded, Currency::Eur)?.minor_units(), 595);
//! # Ok(())
//! # }
//! ```
//!
//! # Retrying
//!
//! Safe with a key.
//! [`ChargeRequest::idempotency_key`](kasapay_core::ChargeRequest::idempotency_key)
//! is sent as Mollie's `Idempotency-Key` header, and Mollie answers a cached
//! copy of the first response to the same key for an hour. Mollie's own advice
//! is that retrying most requests is harmless anyway — a payment set up twice
//! leaves one of the two to expire — and names partial refunds as one of the
//! three cases where it is not.
//!
//! [`Mollie::refund`] takes no key, so a refund that timed out is read back
//! rather than replayed.
//!
//! **The captures endpoint takes a key too.**
//! [`Provider::capture`](kasapay_core::Provider::capture)'s own `idempotency`
//! — and [`Mollie::capture_payment`]'s — travels as the same
//! `Idempotency-Key` header, answered from the same hour-long cache. Without
//! one, replaying a capture can take the funds twice, the same way a
//! replayed partial refund can give them back twice.
//!
//! # An error from Mollie has no code
//!
//! Their error object is `status`, `title`, `detail` and sometimes `field`, and
//! nothing machine-readable beyond the HTTP status. So
//! [`Error::code`](kasapay_core::Error::code) is `None` for every failure this
//! crate reports, where PayTR's `err_no` and iyzico's `errorCode` are not, and
//! the offending field is named in the message instead.
//!
//! # Customers and mandates
//!
//! Mollie's saved instrument is a mandate — `mdt_…`, held against a customer
//! and read back as an [`InstrumentId`](kasapay_core::InstrumentId), the same
//! kind Stripe's `pm_…` and iyzico's `cardToken` are.
//! [`Capabilities::saved_instruments`](kasapay_core::Capabilities::saved_instruments)
//! is true because of it.
//!
//! [`Mollie::create_customer`] registers a customer to hang one on.
//! [`Mollie::charge_first`] opens a payment with `sequenceType: first`, which
//! establishes a mandate as a side effect — Mollie's own documented example
//! carries the new `mdt_…` on the payment while it is still `open`, before
//! the payer has done anything. [`Mollie::mandates`] and [`Mollie::mandate`]
//! list a customer's mandates and read one back; [`MandateStatus`] is
//! `valid`, `pending` or `invalid`, and Mollie's own enum has no fourth value
//! for "revoked" — see its documentation for what that means for telling the
//! two apart.
//! [`Provider::instruments`](kasapay_core::Provider::instruments) is
//! [`Mollie::mandates`] behind the shared trait, with `method` — `directdebit`,
//! `creditcard`, `paypal` — as the label, because this crate models no IBAN
//! and no card for a mandate to show more than that.
//!
//! [`Mollie::charge_with_mandate`] is the charge itself: `sequenceType:
//! recurring` with the `mandateId`, and
//! [`ChargeRequest::customer`](kasapay_core::ChargeRequest::customer)
//! carrying the customer, the other half of the name. **It answers no
//! redirect.** Mollie's own documented example for a recurring payment
//! carries `redirectUrl: null` and no `checkout` link, so
//! [`Charge::next_action`](kasapay_core::Charge::next_action) reads `None`
//! here where [`Provider::charge`](kasapay_core::Provider::charge) would
//! carry a [`NextAction::Redirect`](kasapay_core::NextAction::Redirect).
//!
//! Revoking a mandate is not here, and neither is creating a subscription off
//! one.
//!
//! # What is not here
//!
//! Mollie's API is eighty-seven paths and this crate maps eight of them. Not
//! modelled, and reachable through [`Charge::raw`](kasapay_core::Charge::raw)
//! or not at all: payment methods and the `method` field, `lines` and the
//! addresses that go with them, `statusReason` — which Mollie documents as
//! point-of-sale only — subscriptions, payment links, chargebacks,
//! settlements, and Mollie Connect. Revoking a mandate is a gap of the same
//! kind on the customer side.
//!
//! Listing refunds **is** here now — [`Mollie::refunds`] — and it walks
//! Mollie's pages to the end. That was the reason it was left out: half a list
//! of refunds undercounts what has gone back, and a shop acting on the total
//! refunds money it has already returned. [`Mollie::mandates`] walks them too,
//! which matters because
//! [`Provider::instruments`](kasapay_core::Provider::instruments) is that call
//! and a customer with more than fifty mandates was quietly getting fifty.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{ChargeRequest, Currency, Money, NextAction, OrderRef, Provider, Secret};
//! use kasapay_mollie::{Config, Mollie};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let mollie = Mollie::new(Config::new(Secret::new(std::env::var("MOLLIE_API_KEY")?)))?;
//!
//! let request = ChargeRequest::builder(
//!     OrderRef::new("ord-2026-0001"),
//!     Money::parse("10.00", Currency::Eur)?,
//! )
//! .description("Order #12345")
//! .return_url("https://webshop.example/order/12345".parse()?)
//! .build()?;
//!
//! let charge = mollie.charge(&request).await?;
//! if let Some(NextAction::Redirect { url, .. }) = &charge.next_action {
//!     println!("send the payer to {url}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [Payments API]: https://docs.mollie.com/reference/create-payment

mod client;
mod convert;
pub mod id;
mod wire;

use kasapay_core::ProviderId;

/// Mollie.
pub const MOLLIE: ProviderId = ProviderId::new("mollie");

#[doc(inline)]
pub use crate::client::{
    Capture, CaptureState, Config, Customer, Mandate, MandateStatus, Mollie, ORDER_METADATA_KEY,
    Refund, RefundState,
};
#[doc(inline)]
pub use crate::id::{CaptureId, CustomerId, RefundId};
