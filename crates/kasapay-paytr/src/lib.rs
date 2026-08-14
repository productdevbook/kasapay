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
//! arrives on the payment notice. So a [`PaymentId`](kasapay_core::PaymentId)
//! from this crate is the caller's own string handed back to them.
//!
//! It follows that an order reference must never be reused, and that two
//! payments for one order need two references.
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

mod client;
pub mod payment;
mod signing;
mod wire;

#[doc(inline)]
pub use crate::client::{Config, PAYTR, PayTr};
#[doc(inline)]
pub use crate::signing::Credentials;
