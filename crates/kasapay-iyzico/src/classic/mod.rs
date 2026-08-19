//! iyzico's classic API — everything that is not the In-Store till flow.
//!
//! Ordinary card payments, subscriptions, marketplace, mass payout, card
//! storage: eighty-four operations, all signed with
//! [`IYZWSv2`](crate::Credentials) rather than authenticated with headers.
//!
//! # What is here
//!
//! Thirteen operations, chosen because none of them touches a card number:
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
//! # Holding money rather than taking it
//!
//! Three more, which are the same three calls again with the money held
//! instead of taken:
//!
//! - [`Client::start_checkout_form_preauth`] — the hosted form, authorising
//! - [`Client::checkout_result_preauth`] — the same result endpoint, read as
//!   a hold, because iyzico's answer does not say which form it was
//! - [`Client::preauth_with_saved_card`] — `/payment/auth`'s own request sent
//!   to `/payment/preauth`
//!
//! and [`Provider::capture`](kasapay_core::Provider::capture), which is
//! `/payment/postauth`, is what turns the hold into a sale.
//! [`Client::cancel`] releases one that will never be taken — same-day, after
//! which it is a refund like any other.
//!
//! All three of those take an optional [`Reason`], which is what iyzico is
//! told the money went back for.
//!
//! iyzico Link, Subscription and Mass Payout are part of this API as well, and
//! are signed the same way, but they are products rather than payment calls:
//! they live in [`crate::iyzilink`], [`crate::subscription`] and
//! [`crate::mass`], over this same [`Client`].
//!
//! [`Client`] implements [`Provider`](kasapay_core::Provider), and most of
//! what starts a payment there answers
//! [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported): the hosted
//! form needs more than `ChargeRequest` carries to start, and what identifies
//! one before the payer finishes is a [`FormToken`] rather than a
//! [`PaymentId`](kasapay_core::PaymentId). What the trait does answer is
//! [`capture`](kasapay_core::Provider::capture),
//! [`refund`](kasapay_core::Provider::refund),
//! [`charge_status`](kasapay_core::Provider::charge_status),
//! [`lookup`](kasapay_core::Provider::lookup),
//! [`instruments`](kasapay_core::Provider::instruments) and
//! [`capabilities`](kasapay_core::Provider::capabilities) — everything that
//! happens to a payment once one exists.
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
//! What is left out is the API call that stores a card. `POST
//! /cardstorage/card` wants `cardNumber`, `expireMonth`, `expireYear` and
//! `cardHolderName`, and `registerCard: 1` stores the card being charged and
//! exists only on the endpoints that take a number. Neither is here.
//!
//! **The hosted form fills the vault instead, and no card number goes near
//! this process while it does.** iyzico's form offers the payer a save-my-card
//! box of its own, and the `cardUserKey` and `cardToken` it produces come back
//! on the form's result. Neither field is in `specs/`, whose record of the
//! form's request and answer is silent on both — they are in iyzico's own SDKs
//! and in the sample result on their documentation site, so this crate follows
//! those rather than pretending the loop is open.
//!
//! [`CheckoutFormBuilder::card_user_key`](checkout::CheckoutFormBuilder::card_user_key)
//! is the outbound half: send the key a payer already has and iyzico shows
//! them their saved cards and files any new one under the same key. Without
//! it, every card a payer saves lands under a key of its own and nothing ties
//! them together. The inbound half is on
//! [`Charge::raw`](kasapay_core::Charge::raw), because a saved card is not
//! something the shared [`Charge`](kasapay_core::Charge) has a field for —
//! [`Client::checkout_result`] says which two paths to read.
//!
//! So: fill the vault through the form, read it, charge it, empty it, and hold
//! the handles as [`InstrumentId`](kasapay_core::InstrumentId).
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

pub(crate) use crate::classic::client::{fraud_status, refused};
