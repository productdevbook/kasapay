//! The hosted checkout form: what iyzico needs to open one.
//!
//! Everything here is iyzico's shape rather than kasapay's, because the form
//! needs more than [`ChargeRequest`](kasapay_core::ChargeRequest) carries and
//! most of it is mandatory — a buyer's identity number, a billing and a
//! shipping address, and the basket itemised. Those are what iyzico's
//! compliance needs, not what every payment needs, so they stay here.
//!
//! What comes back is kasapay's shape: a [`Charge`](kasapay_core::Charge).

use kasapay_core::{Money, MoneyError, OrderRef};

/// Who is paying.
///
/// Every field bar the optional ones is required by iyzico, including the
/// national identity number.
#[derive(Debug, Clone)]
pub struct Buyer {
    /// The merchant's own id for this person.
    pub id: Box<str>,
    /// Given name.
    pub name: Box<str>,
    /// Family name.
    pub surname: Box<str>,
    /// Turkish national identity number, or the equivalent.
    pub identity_number: Box<str>,
    /// Email address.
    pub email: Box<str>,
    /// Mobile number.
    pub phone: Box<str>,
    /// The address the account was registered with.
    pub registration_address: Box<str>,
    /// City.
    pub city: Box<str>,
    /// Country.
    pub country: Box<str>,
    /// Postcode.
    pub zip_code: Option<Box<str>>,
    /// The address the request came from, which iyzico's fraud checks use.
    pub ip: Option<Box<str>>,
}

/// Somewhere to bill or ship to.
#[derive(Debug, Clone)]
pub struct Address {
    /// Who to address it to.
    pub contact_name: Box<str>,
    /// The street address.
    pub address: Box<str>,
    /// City.
    pub city: Box<str>,
    /// Country.
    pub country: Box<str>,
    /// Postcode.
    pub zip_code: Option<Box<str>>,
}

/// What kind of thing is being sold.
///
/// iyzico wants to know, and refuses a basket item without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// Something that ships.
    Physical,
    /// Something that does not.
    Virtual,
}

impl ItemKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Physical => "PHYSICAL",
            Self::Virtual => "VIRTUAL",
        }
    }
}

/// One line of the basket.
#[derive(Debug, Clone)]
pub struct BasketItem {
    /// The merchant's own id for this item.
    pub id: Box<str>,
    /// What it is called.
    pub name: Box<str>,
    /// The category it sits in.
    pub category: Box<str>,
    /// What kind of thing it is.
    pub kind: ItemKind,
    /// What it costs.
    pub price: Money,
}

/// A request to open a hosted checkout form.
///
/// Build one with [`CheckoutForm::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CheckoutForm {
    /// The caller's reference for the order.
    pub order: OrderRef,
    /// The basket total.
    pub price: Money,
    /// What is actually charged, which differs from the total under an
    /// instalment surcharge.
    pub paid_price: Money,
    /// Where iyzico sends the payer when the form is done.
    pub callback_url: url::Url,
    /// Who is paying.
    pub buyer: Buyer,
    /// Where to bill.
    pub billing_address: Address,
    /// Where to ship.
    pub shipping_address: Address,
    /// What is being bought.
    pub basket: Vec<BasketItem>,
    /// Which instalment counts to offer, or all of them if empty.
    pub instalments: Vec<u8>,
    /// Whose saved cards to offer, and whose vault a new one joins.
    pub card_user_key: Option<Box<str>>,
}

impl CheckoutForm {
    /// Starts building a request.
    ///
    /// `price` is the basket total. [`CheckoutFormBuilder::paid_price`] sets
    /// what is actually charged; without it the two are the same, which is the
    /// case when nothing is added for instalments.
    #[must_use]
    pub fn builder(
        order: OrderRef,
        price: Money,
        callback_url: url::Url,
        buyer: Buyer,
    ) -> CheckoutFormBuilder {
        CheckoutFormBuilder {
            order,
            price,
            paid_price: None,
            callback_url,
            buyer,
            billing_address: None,
            shipping_address: None,
            basket: Vec::new(),
            instalments: Vec::new(),
            card_user_key: None,
        }
    }
}

/// Collects the parts of a [`CheckoutForm`] before it is checked.
#[derive(Debug, Clone)]
pub struct CheckoutFormBuilder {
    order: OrderRef,
    price: Money,
    paid_price: Option<Money>,
    callback_url: url::Url,
    buyer: Buyer,
    billing_address: Option<Address>,
    shipping_address: Option<Address>,
    basket: Vec<BasketItem>,
    instalments: Vec<u8>,
    card_user_key: Option<Box<str>>,
}

impl CheckoutFormBuilder {
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

    /// Offers only these instalment counts.
    #[must_use]
    pub fn instalments(mut self, counts: impl IntoIterator<Item = u8>) -> Self {
        self.instalments = counts.into_iter().collect();
        self
    }

    /// Shows this payer the cards iyzico already holds for them.
    ///
    /// iyzico's `cardUserKey`. The form then lists their saved cards, and a
    /// card they choose to save on this payment is filed under the same key.
    ///
    /// **Send it whenever the payer has one.** Left out, iyzico issues a fresh
    /// key for any card saved here, and a payer's cards end up scattered across
    /// keys with nothing tying them together —
    /// [`Client::stored_cards`](crate::classic::Client::stored_cards) can only
    /// ever answer for one.
    ///
    /// Not in `specs/`: iyzico's documentation of this request does not mention
    /// the field, and their own SDKs send it.
    #[must_use]
    pub fn card_user_key(mut self, key: impl Into<Box<str>>) -> Self {
        self.card_user_key = Some(key.into());
        self
    }

    /// Checks the request and produces it.
    pub fn build(self) -> Result<CheckoutForm, CheckoutFormError> {
        let billing_address = self
            .billing_address
            .ok_or(CheckoutFormError::NoBillingAddress)?;
        if self.basket.is_empty() {
            return Err(CheckoutFormError::EmptyBasket);
        }
        self.price.require_positive()?;
        let paid_price = self.paid_price.unwrap_or(self.price);
        paid_price.require_positive()?;
        if paid_price.currency() != self.price.currency() {
            return Err(CheckoutFormError::Amount(MoneyError::CurrencyMismatch {
                left: self.price.currency(),
                right: paid_price.currency(),
            }));
        }

        // iyzico rejects a basket whose lines do not add up to the total, and
        // says so with an error code rather than a message anyone can read.
        let total = self.basket.iter().try_fold(
            Money::from_minor_units(0, self.price.currency()),
            |sum, item| sum.checked_add(item.price),
        )?;
        if total != self.price {
            return Err(CheckoutFormError::BasketDoesNotAddUp {
                basket: total,
                price: self.price,
            });
        }

        let shipping_address = self
            .shipping_address
            .unwrap_or_else(|| billing_address.clone());
        Ok(CheckoutForm {
            order: self.order,
            price: self.price,
            paid_price,
            callback_url: self.callback_url,
            buyer: self.buyer,
            billing_address,
            shipping_address,
            basket: self.basket,
            instalments: self.instalments,
            card_user_key: self.card_user_key,
        })
    }
}

/// A [`CheckoutForm`] was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckoutFormError {
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
    /// An amount was not one iyzico will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}
