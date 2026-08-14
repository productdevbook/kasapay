//! iyzico's classic API — everything that is not the In-Store till flow.
//!
//! Ordinary card payments, subscriptions, marketplace, mass payout, card
//! storage: eighty-four operations, all signed with
//! [`IYZWSv2`](crate::Credentials) rather than authenticated with headers.
//!
//! # What is here
//!
//! Eight operations, chosen because none of them touches a card number:
//!
//! - [`Client::start_checkout_form`] — open a hosted form and get a URL to
//!   send the payer to
//! - [`Client::checkout_result`] — read what became of it
//! - [`Client::bin_check`] — what kind of card a BIN belongs to
//! - [`Client::stored_cards`] — the cards iyzico holds for a user
//! - [`Client::forget_card`] — drop one of them
//! - [`Client::refund`] and [`Client::refund_transaction`] — take money back.
//!   The second is the one for a basket with more than one line: iyzico says
//!   in so many words not to use the first there, because which line the
//!   refund comes off is then their choice rather than the shop's
//! - [`Client::cancel`] — void a payment before it settles
//!
//! All three of those take an optional [`Reason`], which is what iyzico is
//! told the money went back for.
//!
//! iyzico Link is part of this API as well, and is signed the same way, but it
//! is a product rather than a payment call: it lives in [`crate::iyzilink`],
//! over this same [`Client`].
//!
//! [`Client`] implements [`Provider`](kasapay_core::Provider), but only half
//! of it: `charge_status` reads a payment back, and `charge` answers
//! [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported) because
//! the hosted form needs more than `ChargeRequest` carries. That is the line
//! the shared trait sits on — it can say what happened to a payment, and not
//! how to start one.
//!
//! The checkout form is how most integrations should take a payment here:
//! iyzico hosts the form and collects the card, so nothing sensitive crosses
//! the caller's server.
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

pub mod checkout;
mod client;
pub mod signature;
mod wire;

#[doc(inline)]
pub use crate::classic::client::{
    Association, BinDetails, CardType, Client, Config, Reason, ReasonCode, Reversal, StoredCard,
};

pub(crate) use crate::classic::client::refused;
