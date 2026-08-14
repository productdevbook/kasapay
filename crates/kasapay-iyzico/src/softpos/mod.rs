//! iyzico's PayPOS softpos — a sale on the payer's own phone.
//!
//! PayPOS turns an Android phone with NFC into a contactless card reader:
//! the caller's own app asks this API for a `deeplink_url`, hands the payer
//! off to Paynet's PayPOS app to tap their card, and reads the outcome back
//! either through the deeplink's callback or with [`Client::check_transaction`].
//! There is no card number anywhere in this module — the card only ever
//! touches the payer's phone.
//!
//! # What is here
//!
//! All three operations iyzico documents for `softpos`:
//!
//! | | |
//! |---|---|
//! | [`Client::init_sale_transaction`] | `POST /v1/softpos/init_sale_transaction` |
//! | [`Client::init_reversal_transaction`] | `POST /v1/softpos/init_reversal_transaction` |
//! | [`Client::check_transaction`] | `POST /v1/softpos/check_transaction` |
//!
//! # Read [`crate::agent`] first
//!
//! Every call here wants a `Session-Key`, and the only way to one is
//! [`agent::Client::get_auth_key`](crate::agent::Client::get_auth_key). Its
//! module documentation covers what this one otherwise would have to repeat:
//! which of the three ways of authenticating "nothing documented" this is,
//! why the host is `api.paynet.com.tr` rather than `api.iyzipay.com`, and why
//! neither product has a worked example to build a test fixture from.
//!
//! # This client does not renew its own session
//!
//! [`Client`] sends the `Session-Key` it was given and reports a refusal as
//! [`ErrorKind::Auth`](kasapay_core::ErrorKind::Auth) rather than quietly
//! fetching another session and retrying — the same choice
//! [`crate::terminal::Client`] makes about its bearer token, for the same
//! reason: a sale already sent to a payer's phone is not safe to retry on
//! this crate's own initiative when nothing here can show whether the first
//! attempt reached them. [`Client::set_session_key`] is how a caller puts a
//! fresh one in front of the same connection pool.
//!
//! # Currency
//!
//! `TRY`, and nothing else is accepted on [`Client::init_sale_transaction`] —
//! see [`InitSale::new`](crate::softpos::InitSale::new) for the evidence and
//! for what is inference rather than a documented enum. Reading is the
//! permissive direction, as it is everywhere else in this crate:
//! [`Transaction`]'s amount fields read whatever [`Currency`](kasapay_core::Currency)
//! its `currency` names, and are `None` only for a code
//! [`Currency`](kasapay_core::Currency) has no name for, with the figure
//! still in [`Transaction::raw`].
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money};
//! use kasapay_iyzico::{agent, softpos};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let dealer = agent::Client::new(
//!     agent::Config::sandbox(),
//!     agent::Credentials::new("sck_test_xxx"),
//! )?;
//! let session = dealer.get_auth_key("agent-1", "till-7").await?;
//!
//! let till = softpos::Client::new(softpos::Config::sandbox(), session.session_key.clone())?;
//! let flow = till
//!     .init_sale_transaction(&softpos::InitSale::new(Money::parse("149.90", Currency::Try)?))
//!     .await?;
//!
//! // Hand `flow.deeplink_url` to the mobile app, then poll for the outcome.
//! if let Some(payment_session_id) = flow.payment_session_id.as_deref() {
//!     let transactions = till.check_transaction(payment_session_id).await?;
//!     println!("{} transaction(s) recorded", transactions.len());
//! }
//! # Ok(())
//! # }
//! ```

mod client;
mod request;
mod wire;

use kasapay_core::{Error, ErrorKind, ProviderId};

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// A request that never got a usable answer.
fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PROVIDER, error.to_string())
}

#[doc(inline)]
pub use crate::softpos::client::{Client, Config, PaymentFlow, Transaction};
#[doc(inline)]
pub use crate::softpos::request::{InitReversal, InitSale};
