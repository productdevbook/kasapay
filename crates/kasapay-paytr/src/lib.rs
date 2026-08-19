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
//! # A refusal arrives on the notice
//!
//! PayTR does not wait for the payer to come back. It posts the outcome to the
//! merchant's notification address, retries until the body of the reply is
//! exactly `OK`, and signs it with a hash whose salt sits in a different place
//! from every other call. [`Notice::charge`] checks that hash and answers the
//! [`Charge`](kasapay_core::Charge) it reports: a refused payment is
//! [`Status::Failed`](kasapay_core::Status::Failed) with the amount that was
//! attempted, which is the only place this crate produces that status.
//!
//! **A notice that does not verify should still be answered `OK`** and then
//! ignored. Anything else makes PayTR retry it for days, and acting on it is
//! how a shop ships against a payment nobody made.
//!
//! # The status query cannot tell a refusal from an unknown order
//!
//! It answers a payment that succeeded, or an error. So
//! [`charge_status`](kasapay_core::Provider::charge_status) here produces only
//! [`Status::Captured`](kasapay_core::Status::Captured), and a payment PayTR
//! refused and an order it has never heard of are the same
//! [`ErrorKind::NotFound`](kasapay_core::ErrorKind::NotFound) with PayTR's own
//! `err_no` on it. Nothing in the answer separates them, so nothing here
//! pretends to.
//!
//! Poll the status query to confirm a success; read the notice to learn about
//! a failure.
//!
//! # Nothing here says a payment was refunded
//!
//! PayTR reports refunds as a list on the payment and gives the payment itself
//! no refunded state. So [`PayTr::refunds`] summed and compared against
//! [`Charge::amount`](kasapay_core::Charge::amount) is what answers "is this
//! fully refunded"; [`Status`](kasapay_core::Status) has no word for it.
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
//! # Instalment rates are here, and their shape is not
//!
//! [`PayTr::instalment_rates`] calls `/odeme/taksit-oranlari` and answers the
//! four fields PayTR documents — the status, the echoed `request_id`, the
//! error message and `max_inst_non_bus` — with the body kept whole beside
//! them.
//!
//! The rates themselves stay on [`InstalmentRates::raw`]. PayTR documents
//! `oranlar` only as "the rates, by card family, in array format", and never
//! says what one entry looks like: not in the field table, not in the PDF, not
//! in any of the four sample programs, all of which print it and stop. A
//! struct for it would be a shape invented in this crate, and a shape invented
//! here fails at parse time against every real merchant.
//!
//! So the call is wrapped and the guess is not made. A caller who has a
//! merchant account can read the body today, and #73 is what a real one
//! finishes.
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
//! back before sending it again — that is always safe, and here it costs
//! nothing extra: PayTR names a payment by the reference the caller chose, so
//! [`lookup`](kasapay_core::Provider::lookup) answers `Ok(None)` when PayTR
//! has no record of it and `Ok(Some(..))` when it does. `Ok(None)` is the
//! licence to send it again.
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
pub mod notice;
pub mod payment;
mod signing;
mod wire;

#[doc(inline)]
pub use crate::card::{CardDetails, CardKind, CardScheme};
#[doc(inline)]
pub use crate::client::{Config, InstalmentRates, PAYTR, PayTr, RefundRecord, payment_id};
#[doc(inline)]
pub use crate::notice::Notice;
#[doc(inline)]
pub use crate::signing::Credentials;
