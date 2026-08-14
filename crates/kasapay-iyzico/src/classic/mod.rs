//! iyzico's classic API — everything that is not the In-Store till flow.
//!
//! Ordinary card payments, subscriptions, marketplace, mass payout, card
//! storage: eighty-four operations, all signed with
//! [`IYZWSv2`](crate::Credentials) rather than authenticated with headers.
//!
//! # What is here
//!
//! [`Client::bin_check`], and no more yet. It is the smallest operation the
//! API has, which makes it the one that proves the signing works end to end —
//! it is also the example iyzico's own authentication page is written around.
//!
//! Taking a payment through this API needs card details on the request, which
//! `ChargeRequest` has nowhere to put and which drag PCI scope in with them.
//! That is a decision about the core rather than about this module.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_iyzico::{Credentials, classic};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//!
//! let card = iyzipay.bin_check("535805").await?;
//! println!("{} {:?} {:?}", card.bank_name.unwrap_or_default(), card.card_type, card.association);
//! # Ok(())
//! # }
//! ```

mod client;
mod wire;

#[doc(inline)]
pub use crate::classic::client::{Association, BinDetails, CardType, Client, Config};
