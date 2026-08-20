//! Charging a card iyzico already holds, and the pair that names one.
//!
//! `POST /payment/auth` takes a `paymentCard`, and iyzico will fill that object
//! two ways: with a card number, or with the `cardUserKey` and `cardToken` they
//! answered when the card was stored. [`Card`] is the second of those and
//! cannot express the first, so a payment built here sends no card number and
//! the caller's process never holds one.
//!
//! The buyer, the two addresses and the basket are the same shapes the hosted
//! form takes and live in [`checkout`](crate::classic::checkout); iyzico
//! requires all of them here too.
//!
//! # Where the card came from
//!
//! Not from here, and not from `POST /cardstorage/card` either — that one wants
//! the number, expiry and holder name, and is not in this crate. The way a card
//! gets into the vault without a number crossing this process is the hosted
//! checkout form, which offers the payer a save-my-card box and answers the
//! `cardUserKey` and `cardToken` on its result. So the loop is:
//!
//! 1. [`Client::start_checkout_form`](crate::classic::Client::start_checkout_form),
//!    with
//!    [`card_user_key`](crate::classic::checkout::CheckoutFormBuilder::card_user_key)
//!    set if the payer already has one
//! 2. [`Client::checkout_result`](crate::classic::Client::checkout_result), and
//!    read the pair off [`Charge::raw`](kasapay_core::Charge::raw)
//! 3. [`Client::stored_cards`](crate::classic::Client::stored_cards) to show
//!    them again, this module to charge one,
//!    [`Client::forget_card`](crate::classic::Client::forget_card) to drop it
//!
//! # Both halves, or neither
//!
//! iyzico's error `5111` is *"`cardUserKey` must be sent with `cardToken`"*. A
//! [`Card`] cannot be built with one and not the other, so that refusal happens
//! here rather than after a round trip.
//!
//! # Without 3-D Secure
//!
//! `/payment/auth` takes no `callbackUrl` and runs no challenge, so a payment
//! taken through this module is unauthenticated and the chargeback liability is
//! the merchant's. iyzico's own description of the endpoint says a stored card
//! can be charged *"NON3D or 3DS"*, and their legacy field tables mark
//! `cardNumber` and `cvc` on `/payment/3dsecure/initialize` as required only
//! *"if the payment is not being made with a stored card"* — so the pair very
//! likely works there too. This crate implements neither 3-D Secure call, which
//! is why there is no authenticated variant to offer rather than because there
//! could not be one.

use kasapay_core::{InstrumentId, Money, MoneyError, OrderRef};

use crate::classic::checkout::{Address, BasketItem, Buyer};

/// The pair iyzico names one stored card by.
///
/// `cardUserKey` says whose vault, `cardToken` says which card in it; neither
/// means anything without the other. Both come back from
/// [`Client::stored_cards`](crate::classic::Client::stored_cards) and from
/// whatever stored the card in the first place.
///
/// Construction is fallible on purpose — see [`CardError`].
///
/// The token is an [`InstrumentId`], so a payment id cannot stand in for one
/// however alike the two strings look:
///
/// ```compile_fail
/// use kasapay_core::PaymentId;
/// use kasapay_iyzico::classic::saved::Card;
///
/// Card::new("card-user-key-1", PaymentId::issued("12345678")).ok();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    user_key: Box<str>,
    token: InstrumentId,
}

impl Card {
    /// Names a stored card, refusing anything that is not one.
    ///
    /// ```
    /// use kasapay_core::InstrumentId;
    /// use kasapay_iyzico::classic::saved::Card;
    ///
    /// let card = Card::new("card-user-key-1", InstrumentId::issued("card-token-1"))?;
    /// assert_eq!(card.user_key(), "card-user-key-1");
    /// # Ok::<(), kasapay_iyzico::classic::saved::CardError>(())
    /// ```
    ///
    /// A card number in either half does not become a payment:
    ///
    /// ```
    /// use kasapay_core::InstrumentId;
    /// use kasapay_iyzico::classic::saved::{Card, CardError};
    ///
    /// let wired_wrong = Card::new("card-user-key-1", InstrumentId::issued("5528790000000008"));
    /// assert!(matches!(wired_wrong, Err(CardError::CardNumber { .. })));
    /// ```
    pub fn new(user_key: impl Into<Box<str>>, token: InstrumentId) -> Result<Self, CardError> {
        let user_key = user_key.into();
        if user_key.is_empty() {
            return Err(CardError::NoUserKey);
        }
        if token.as_str().is_empty() {
            return Err(CardError::NoToken);
        }
        for (field, value) in [("cardUserKey", &*user_key), ("cardToken", token.as_str())] {
            if kasapay_core::looks_like_a_card_number(value) {
                return Err(CardError::CardNumber { field });
            }
        }
        Ok(Self { user_key, token })
    }

    /// iyzico's `cardUserKey`: whose vault this card sits in.
    #[must_use]
    pub fn user_key(&self) -> &str {
        &self.user_key
    }

    /// iyzico's `cardToken`: which card in that vault.
    #[must_use]
    pub const fn token(&self) -> &InstrumentId {
        &self.token
    }
}

/// A [`Card`] was named by something that does not name a stored card.
///
/// No variant carries the value it refused: an error is a thing that gets
/// logged, and the whole point of refusing a card number is that it does not
/// end up somewhere it will be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CardError {
    /// The `cardUserKey` was empty, and iyzico requires one.
    #[error("a stored card needs the cardUserKey whose vault it sits in")]
    NoUserKey,
    /// The `cardToken` was empty, and iyzico requires one.
    #[error("a stored card needs the cardToken that names it")]
    NoToken,
    /// One of the two was a card number.
    #[error(
        "{field} was given what is a card number by shape; a stored card is named by iyzico's \
         handles, and a card number reaching this process is the thing this refuses"
    )]
    CardNumber {
        /// Which of the two, named as iyzico names it.
        field: &'static str,
    },
}

/// A payment to take against a card iyzico already holds.
///
/// Build one with [`Payment::builder`]. Everything iyzico requires of an
/// ordinary card payment it requires of this one as well — a buyer with an
/// identity number, a billing address, an itemised basket — because it is the
/// same endpoint. The one thing it does not require is the card.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payment {
    /// The caller's reference for the order.
    pub order: OrderRef,
    /// The basket total.
    pub price: Money,
    /// What is actually charged, which differs from the total under an
    /// instalment surcharge.
    pub paid_price: Money,
    /// The card to charge.
    pub card: Card,
    /// Who is paying.
    pub buyer: Buyer,
    /// Where to bill.
    pub billing_address: Address,
    /// Where to ship.
    pub shipping_address: Address,
    /// What is being bought.
    pub basket: Vec<BasketItem>,
    /// How many instalments to split it over, or `None` for one.
    pub instalment: Option<u8>,
}

impl Payment {
    /// Starts building a payment against a stored card.
    ///
    /// `price` is the basket total. [`PaymentBuilder::paid_price`] sets what is
    /// actually charged; without it the two are the same, which is the case
    /// when nothing is added for instalments.
    #[must_use]
    pub fn builder(order: OrderRef, price: Money, buyer: Buyer, card: Card) -> PaymentBuilder {
        PaymentBuilder {
            order,
            price,
            paid_price: None,
            card,
            buyer,
            billing_address: None,
            shipping_address: None,
            basket: Vec::new(),
            instalment: None,
        }
    }
}

/// Collects the parts of a [`Payment`] before it is checked.
#[derive(Debug, Clone)]
pub struct PaymentBuilder {
    order: OrderRef,
    price: Money,
    paid_price: Option<Money>,
    card: Card,
    buyer: Buyer,
    billing_address: Option<Address>,
    shipping_address: Option<Address>,
    basket: Vec<BasketItem>,
    instalment: Option<u8>,
}

impl PaymentBuilder {
    /// Sets what is actually charged, when it differs from the basket total.
    #[must_use]
    pub const fn paid_price(mut self, paid_price: Money) -> Self {
        self.paid_price = Some(paid_price);
        self
    }

    /// Sets where to bill. Also used for shipping unless one is set.
    #[must_use]
    pub fn billing_address(mut self, address: Address) -> Self {
        self.billing_address = Some(address);
        self
    }

    /// Sets where to ship.
    #[must_use]
    pub fn shipping_address(mut self, address: Address) -> Self {
        self.shipping_address = Some(address);
        self
    }

    /// Adds one line to the basket.
    #[must_use]
    pub fn item(mut self, item: BasketItem) -> Self {
        self.basket.push(item);
        self
    }

    /// Splits the payment over this many instalments.
    ///
    /// iyzico documents one, two, three, six, nine and twelve, and which of
    /// them a card may be charged in is the issuer's answer rather than
    /// iyzico's — [`Client::bin_check`](crate::classic::Client::bin_check) is
    /// what asks. Anything else is refused by iyzico rather than here, so a
    /// count they add later needs no change in this crate.
    #[must_use]
    pub const fn instalment(mut self, count: u8) -> Self {
        self.instalment = Some(count);
        self
    }

    /// Checks the request and produces it.
    pub fn build(self) -> Result<Payment, PaymentError> {
        let billing_address = self.billing_address.ok_or(PaymentError::NoBillingAddress)?;
        if self.basket.is_empty() {
            return Err(PaymentError::EmptyBasket);
        }
        if self.instalment == Some(0) {
            return Err(PaymentError::NoInstalments);
        }
        self.price.require_positive()?;
        let paid_price = self.paid_price.unwrap_or(self.price);
        paid_price.require_positive()?;
        if paid_price.currency() != self.price.currency() {
            return Err(PaymentError::Amount(MoneyError::CurrencyMismatch {
                left: self.price.currency(),
                right: paid_price.currency(),
            }));
        }

        // iyzico refuses a basket whose lines do not add up to the total, and
        // says so with an error code rather than a message anyone can read.
        let total = self.basket.iter().try_fold(
            Money::from_minor_units(0, self.price.currency()),
            |sum, item| sum.checked_add(item.price),
        )?;
        if total != self.price {
            return Err(PaymentError::BasketDoesNotAddUp {
                basket: total,
                price: self.price,
            });
        }

        let shipping_address = self
            .shipping_address
            .unwrap_or_else(|| billing_address.clone());
        Ok(Payment {
            order: self.order,
            price: self.price,
            paid_price,
            card: self.card,
            buyer: self.buyer,
            billing_address,
            shipping_address,
            basket: self.basket,
            instalment: self.instalment,
        })
    }
}

/// A [`Payment`] was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PaymentError {
    /// No billing address was set, and iyzico requires one.
    #[error("a billing address is required")]
    NoBillingAddress,
    /// The basket had no lines, and iyzico requires at least one.
    #[error("the basket is empty")]
    EmptyBasket,
    /// The basket's lines do not add up to the stated total.
    #[error("the basket comes to {basket}, but the price is {price}")]
    BasketDoesNotAddUp {
        /// What the lines add up to.
        basket: Money,
        /// What the total was said to be.
        price: Money,
    },
    /// Zero instalments was asked for, which is not a payment.
    #[error("an instalment count of zero takes no money; leave it unset for one instalment")]
    NoInstalments,
    /// An amount was not one iyzico will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}

#[cfg(test)]
mod tests {
    use kasapay_core::InstrumentId;

    use super::{Card, CardError};

    #[test]
    fn the_error_never_repeats_the_number_it_refused() {
        let number = "5528790000000008";
        let refused = Card::new("card-user-key-1", InstrumentId::issued(number))
            .expect_err("a card number does not name a stored card");
        assert_eq!(refused, CardError::CardNumber { field: "cardToken" });
        assert!(!format!("{refused:?}").contains(number));
        assert!(!refused.to_string().contains(number));
    }

    #[test]
    fn both_halves_are_required() {
        let token = InstrumentId::issued("card-token-1");
        assert_eq!(
            Card::new("", token.clone()).expect_err("no user key"),
            CardError::NoUserKey
        );
        assert_eq!(
            Card::new("card-user-key-1", InstrumentId::issued("")).expect_err("no token"),
            CardError::NoToken
        );
        assert!(Card::new("card-user-key-1", token).is_ok());
    }
}
