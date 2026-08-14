//! iyzico's classic API — everything that is not the In-Store till flow.
//!
//! Ordinary card payments, subscriptions, marketplace, mass payout, card
//! storage: eighty-four operations, all signed with
//! [`IYZWSv2`](crate::Credentials) rather than authenticated with headers.
//!
//! # What is here
//!
//! Ten operations, chosen because none of them touches a card number:
//!
//! - [`Client::start_checkout_form`] — open a hosted form and get a URL to
//!   send the payer to
//! - [`Client::checkout_result`] — read what became of it, by the
//!   [`FormToken`] the form was opened with
//! - [`Client::payment`] — read a finished payment back by its id
//! - [`Client::bin_check`] — what kind of card a BIN belongs to
//! - [`Client::stored_cards`] — the cards iyzico holds for a user
//! - [`Client::pay_with_saved_card`] — charge one of them, by the pair that
//!   names it rather than by a number
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
//! iyzico Link, Subscription and Mass Payout are part of this API as well, and
//! are signed the same way, but they are products rather than payment calls:
//! they live in [`crate::iyzilink`], [`crate::subscription`] and
//! [`crate::mass`], over this same [`Client`].
//!
//! [`Client`] implements [`Provider`](kasapay_core::Provider), and every
//! payment operation on it answers
//! [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported): the hosted
//! form needs more than `ChargeRequest` carries to start, and what identifies
//! one before the payer finishes is a [`FormToken`] rather than a
//! [`PaymentId`](kasapay_core::PaymentId). This flow is driven by the calls
//! above; what the trait still answers here is which provider this is and what
//! it can do.
//!
//! # Where a card number may live, and where it does not
//!
//! Nowhere in this crate. A first payment goes through the checkout form:
//! iyzico hosts it and collects the card, so nothing sensitive crosses the
//! caller's server. A repeat payment goes through
//! [`Client::pay_with_saved_card`], which sends the `cardUserKey` and
//! `cardToken` iyzico answered when the card was stored — the same
//! `/payment/auth` endpoint an ordinary card payment uses, filled the other of
//! the two ways iyzico documents for it.
//!
//! What is left out is storing a card, and that is iyzico's boundary rather
//! than a decision here. `POST /cardstorage/card` wants `cardNumber`,
//! `expireMonth`, `expireYear` and `cardHolderName`; `registerCard: 1` on a
//! payment stores the card being charged, and is only available on the
//! endpoints that take a number. **iyzico documents no way to put a card in
//! their vault without holding the number** — not on the checkout form, whose
//! request has no `cardUserKey` and whose answer returns no `cardToken`, and
//! not anywhere else. A caller who wants a stored card either was already in
//! scope to collect one, or gets the handles from something that was.
//!
//! So: read the vault, charge it, empty it. Filling it is somebody else's act,
//! and the handles come in through
//! [`InstrumentId`](kasapay_core::InstrumentId).
//!
//! The other reason a payment cannot go through
//! [`Provider::charge`](kasapay_core::Provider::charge) is unchanged: iyzico
//! wants a buyer with an identity number, two addresses and an itemised basket
//! either way, and `ChargeRequest` carries none of it.
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
pub mod saved;
pub mod signature;
mod wire;

use kasapay_core::Id;

#[doc(inline)]
pub use crate::classic::client::{
    Association, BinDetails, CardType, Client, Config, Reason, ReasonCode, Reversal, StoredCard,
};

/// What a classic identifier names, where iyzico names something core has no
/// kind for.
pub mod kind {
    /// One hosted checkout form, which is not the payment it may become.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Checkout;

    impl kasapay_core::IdKind for Checkout {
        const NAMES: &'static str = "iyzico checkout form";
    }
}

/// iyzico's token for one hosted checkout form.
///
/// Not a [`PaymentId`](kasapay_core::PaymentId), and the compiler now says so:
/// until the payer finishes the form iyzico has issued no payment id, and this
/// token is all there is to read the form back by. It arrives as the
/// `continuation` of the [`NextAction::Redirect`](kasapay_core::NextAction)
/// that [`Client::start_checkout_form`] answers, and goes into
/// [`Client::checkout_result`].
pub type FormToken = Id<kind::Checkout>;

pub(crate) use crate::classic::client::refused;
