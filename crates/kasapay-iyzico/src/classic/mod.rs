//! iyzico's classic API — everything that is not the In-Store till flow.
//!
//! Ordinary card payments, subscriptions, marketplace, mass payout, card
//! storage: eighty-four operations, all signed with
//! [`IYZWSv2`](crate::Credentials) rather than authenticated with headers.
//!
//! # What is here
//!
//! Three operations, chosen because none of them touches a card number:
//!
//! - [`Client::bin_check`] — what kind of card a BIN belongs to
//! - [`Client::stored_cards`] — the cards iyzico holds for a user
//! - [`Client::forget_card`] — drop one of them
//!
//! Taking a payment through this API needs the card on the request, which
//! `ChargeRequest` has nowhere to put and which drags PCI scope in with it.
//! **So does storing a card**: `POST /cardstorage/card` wants the number too.
//! The way to store a card without one reaching the caller's server is the
//! hosted checkout form, which collects it directly. That is a decision about
//! the core rather than about this module.
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
pub use crate::classic::client::{Association, BinDetails, CardType, Client, Config, StoredCard};
