//! PayTR, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! PayTR hosts the payment form, so no card data crosses this process.
//! [`PayTr::start_payment`] opens one and answers a
//! [`Charge`](kasapay_core::Charge) carrying the address to send the payer to;
//! [`charge_status`](kasapay_core::Provider::charge_status) reads it back.
//!
//! # PayTR has no payment id
//!
//! Every other provider here gives a payment its own identifier. PayTR names a
//! payment by **the merchant's own order reference** — the `merchant_oid` sent
//! when it was opened — and that is what reads it back, refunds it, and
//! arrives on the payment notice.
//!
//! So a [`PaymentId`](kasapay_core::PaymentId) from this crate says so:
//! [`payment_id`] builds one out of an [`OrderRef`](kasapay_core::OrderRef),
//! and its [`source`](kasapay_core::PaymentId::source) is
//! [`IdSource::Derived`](kasapay_core::IdSource::Derived) naming `merchant_oid`
//! rather than claiming PayTR issued anything.
//!
//! It follows that an order reference must never be reused, and that two
//! payments for one order need two references: nothing else keeps two payments
//! apart here, and a caller writing this identifier into a unique index is
//! relying on their own discipline rather than PayTR's guarantee.
//!
//! # A failed payment is an error, not a status
//!
//! PayTR's status query answers a payment that happened, or an error saying it
//! does not know one. So
//! [`charge_status`](kasapay_core::Provider::charge_status) here produces only
//! [`Status::Captured`](kasapay_core::Status::Captured) — never
//! [`Failed`](kasapay_core::Status::Failed) — and a payment that was refused
//! comes back as [`ErrorKind::NotFound`](kasapay_core::ErrorKind::NotFound).
//!
//! The payment notice is where a refusal is actually reported: it carries
//! `status=failed` with a reason. Poll the status query to confirm a success;
//! read the notice to learn about a failure.
//!
//! # The payment notice
//!
//! PayTR does not wait for the payer to come back. It posts the outcome to the
//! merchant's notification address, retries until the body of the reply is
//! exactly `OK`, and signs it with a hash whose salt sits in a different place
//! from every other call. [`Credentials::verify_callback`] checks it.
//!
//! **A notice that does not verify should still be answered `OK`** and then
//! ignored. Anything else makes PayTR retry it for days, and acting on it is
//! how a shop ships against a payment nobody made.
//!
//! # Looking a card up before charging it
//!
//! [`PayTr::bin_details`] answers what PayTR knows about the first 6 or 8
//! digits of a card number: its bank, its network, whether it is a company
//! card, whether it may go without 3-D Secure, and which instalment programme
//! it belongs to. A BIN PayTR has no record of — a card issued outside Turkey,
//! usually — is `Ok(None)` rather than an error.
//!
//! [`CardDetails::programme`] is the field worth reading twice. `None` means
//! the card is in no programme, and **a card in no programme cannot be paid in
//! instalments through PayTR** — so it is the answer to "may I offer this payer
//! instalments" as well as the value a Direkt API payment sends as `card_type`.
//!
//! # Instalment rates are not here
//!
//! PayTR's `/odeme/taksit-oranlari` is not wrapped, and deliberately: the
//! service exists to return the `oranlar` field, and PayTR documents that field
//! only as "the rates, by card family, in array format". It never says what one
//! entry looks like — not in the field table, not in the PDF, not in any of the
//! four sample programs, all of which print it and stop. There is nothing to
//! model that would not be a guess, and a guess here fails at parse time
//! against every real merchant.
//!
//! # Retrying a payment is not documented as safe
//!
//! PayTR documents no idempotency mechanism for opening a payment, and does
//! not say what happens if a `merchant_oid` is reused. It documents duplicate
//! rejection for the transfer service's `trans_id` and for nothing else, so
//! there is no basis for assuming an order reference behaves the same way.
//!
//! [`Error::is_retryable`](kasapay_core::Error::is_retryable) can therefore be
//! true for a failure whose retry might take the money twice. Read the payment
//! back with [`charge_status`](kasapay_core::Provider::charge_status) before
//! sending it again — that call is always safe, and PayTR answers an error for
//! a payment it does not know.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money, OrderRef};
//! use kasapay_paytr::{Config, Credentials, PayTr, payment};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let paytr = PayTr::new(Config::new(Credentials::new("merchant-id", "key", "salt")).test_mode())?;
//!
//! let payment = payment::Payment::builder(
//!     OrderRef::new("ord-2026-0001"),
//!     Money::parse("149.90", Currency::Try)?,
//!     payment::Payer {
//!         email: "ayse@example.test".into(),
//!         ip: "203.0.113.7".into(),
//!         name: "Ayse Yilmaz".into(),
//!         address: "Bagdat Cad. 1".into(),
//!         phone: "+905350000000".into(),
//!         success_url: "https://merchant.example/ok".parse()?,
//!         failure_url: "https://merchant.example/no".parse()?,
//!     },
//! )
//! .item(payment::BasketItem {
//!     name: "Kahve".into(),
//!     price: Money::parse("149.90", Currency::Try)?,
//!     quantity: 1,
//! })
//! .build()?;
//! # let _ = (paytr, payment);
//! # Ok(())
//! # }
//! ```

pub mod card;
mod client;
pub mod payment;
mod signing;
mod wire;

#[doc(inline)]
pub use crate::card::{CardDetails, CardKind, CardScheme};
#[doc(inline)]
pub use crate::client::{Config, PAYTR, PayTr, RefundRecord, payment_id};
#[doc(inline)]
pub use crate::signing::Credentials;
