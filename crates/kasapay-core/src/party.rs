//! Who is paying, where they are, and what they are buying.
//!
//! Three of the five providers this workspace ships refuse a payment without
//! these. iyzico's hosted form wants a buyer with a national identity number,
//! an address and an itemised basket; PayTR's wants an email, an address, a
//! phone number and the payer's IP; PayPal builds a purchase unit out of much
//! the same. Stripe and Mollie want none of it.
//!
//! # Why this is in core rather than in each adapter
//!
//! Because it was in each adapter, and the cost of that was the library's own
//! promise: [`Provider::charge`](crate::Provider::charge) answered
//! [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) for iyzico's
//! classic API and for PayTR, so "which provider takes the money is a
//! deployment decision rather than a rewrite" was false for exactly the two a
//! Turkish shop would swap between. A caller had to reach past the trait and
//! build a `classic::CheckoutForm` or a `paytr::Payment` by hand.
//!
//! A buyer and a basket are not a provider's idea. They are a shop's, and a
//! shop that is taking payments has both already.
//!
//! # None of it is required
//!
//! [`ChargeRequest`](crate::ChargeRequest) carries these as options, and a
//! Stripe or Mollie caller who never sets one is unaffected. An adapter that
//! needs a field and does not get it answers
//! [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest) **naming
//! the field**, before a socket opens — which is a better failure than the
//! provider's own 400, and a much better one than a method that refuses every
//! call.

use crate::money::{Money, MoneyError};

/// Somewhere to bill, ship or register a payer at.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Address {
    /// Who the address is for, where the provider asks separately.
    ///
    /// iyzico requires it on a billing address. `None` is a caller who did not
    /// say, and an adapter that needs one falls back to the buyer's own name
    /// rather than sending nothing.
    pub contact_name: Option<Box<str>>,
    /// The street address, as one line.
    pub line: Box<str>,
    /// City.
    pub city: Box<str>,
    /// Country.
    pub country: Box<str>,
    /// Postcode, where there is one.
    pub zip_code: Option<Box<str>>,
}

impl Address {
    /// An address with the three parts every provider that wants one asks for.
    #[must_use]
    pub fn new(
        line: impl Into<Box<str>>,
        city: impl Into<Box<str>>,
        country: impl Into<Box<str>>,
    ) -> Self {
        Self {
            contact_name: None,
            line: line.into(),
            city: city.into(),
            country: country.into(),
            zip_code: None,
        }
    }

    /// Names who the address is for.
    #[must_use]
    pub fn contact_name(mut self, name: impl Into<Box<str>>) -> Self {
        self.contact_name = Some(name.into());
        self
    }

    /// Adds the postcode.
    #[must_use]
    pub fn zip_code(mut self, zip_code: impl Into<Box<str>>) -> Self {
        self.zip_code = Some(zip_code.into());
        self
    }
}

/// The person paying.
///
/// Only the name and the email are required here, because they are the two
/// every provider that asks for a buyer asks for. The rest are `Option`
/// because which of them is mandatory is the provider's own decision — iyzico
/// refuses a payment without an identity number, PayTR without the payer's IP
/// — and an adapter says which one is missing when it needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Buyer {
    /// Given name. A provider that wants one field takes this.
    pub name: Box<str>,
    /// Family name, where the provider keeps them apart. iyzico does.
    pub surname: Option<Box<str>>,
    /// Email address.
    pub email: Box<str>,
    /// Mobile number.
    pub phone: Option<Box<str>>,
    /// Turkish national identity number, or the equivalent. iyzico's classic
    /// API requires one on every hosted form.
    pub identity_number: Option<Box<str>>,
    /// The address the request came from, which fraud checks read. PayTR
    /// requires it.
    pub ip: Option<Box<str>>,
    /// Where the payer is registered, which iyzico sends as
    /// `registrationAddress` and PayTR as the payer's address.
    pub address: Option<Address>,
}

impl Buyer {
    /// A buyer with the two fields every provider that wants one asks for.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, email: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            surname: None,
            email: email.into(),
            phone: None,
            identity_number: None,
            ip: None,
            address: None,
        }
    }

    /// Adds the family name.
    #[must_use]
    pub fn surname(mut self, surname: impl Into<Box<str>>) -> Self {
        self.surname = Some(surname.into());
        self
    }

    /// Adds the mobile number.
    #[must_use]
    pub fn phone(mut self, phone: impl Into<Box<str>>) -> Self {
        self.phone = Some(phone.into());
        self
    }

    /// Adds the national identity number.
    #[must_use]
    pub fn identity_number(mut self, identity_number: impl Into<Box<str>>) -> Self {
        self.identity_number = Some(identity_number.into());
        self
    }

    /// Adds the address the request came from.
    #[must_use]
    pub fn ip(mut self, ip: impl Into<Box<str>>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// Adds where the payer is registered.
    #[must_use]
    pub fn address(mut self, address: Address) -> Self {
        self.address = Some(address);
        self
    }
}

/// What kind of thing is being sold.
///
/// iyzico refuses a basket line without it, and it is what decides whether a
/// shipping address means anything.
///
/// Deliberately exhaustive, for the reason
/// [`Currency`](crate::Currency) is: it goes out to a provider, so a third
/// variant would be one every adapter has to say what it sends for. A
/// wildcard arm would let it go out as whatever the last variant mapped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemKind {
    /// Something that ships. The default, because most baskets ship.
    #[default]
    Physical,
    /// Something that does not.
    Virtual,
}

/// One line of what is being paid for.
///
/// [`BasketItem::price`] is what **one** of the thing costs, and
/// [`BasketItem::line_total`] is what the line comes to. The distinction is
/// not cosmetic: PayTR takes the unit price and the count as two fields, and
/// iyzico takes one figure and has nowhere to put a count, so an adapter that
/// guessed which of the two it had been handed would silently charge a basket
/// of three for one.
///
/// The sum of a basket does not have to equal
/// [`ChargeRequest::amount`](crate::ChargeRequest::amount), and nothing here
/// checks that it does: an instalment surcharge is money the payer is charged
/// and not a line of the basket, which is exactly the difference
/// [`Charge::order_amount`](crate::Charge::order_amount) exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BasketItem {
    /// The shop's own id for it.
    pub id: Box<str>,
    /// What it is called.
    pub name: Box<str>,
    /// The category it sits in. iyzico asks; nobody else does.
    pub category: Option<Box<str>>,
    /// Whether it ships.
    pub kind: ItemKind,
    /// What one of it costs. [`BasketItem::line_total`] is the whole line.
    pub price: Money,
    /// How many. PayTR sends this as its own field; iyzico has no such field,
    /// so its adapter sends the line total instead.
    pub quantity: u32,
}

impl BasketItem {
    /// One line, priced.
    #[must_use]
    pub fn new(id: impl Into<Box<str>>, name: impl Into<Box<str>>, price: Money) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            category: None,
            kind: ItemKind::Physical,
            price,
            quantity: 1,
        }
    }

    /// Says which category it sits in.
    #[must_use]
    pub fn category(mut self, category: impl Into<Box<str>>) -> Self {
        self.category = Some(category.into());
        self
    }

    /// Marks the line as something that does not ship.
    #[must_use]
    pub const fn virtual_item(mut self) -> Self {
        self.kind = ItemKind::Virtual;
        self
    }

    /// Says how many of it.
    #[must_use]
    pub const fn quantity(mut self, quantity: u32) -> Self {
        self.quantity = quantity;
        self
    }

    /// What the line comes to: the unit price times the count.
    pub fn line_total(&self) -> Result<Money, MoneyError> {
        self.price.checked_mul(self.quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, BasketItem, Buyer, ItemKind};
    use crate::money::{Currency, Money};

    #[test]
    fn a_buyer_carries_only_what_every_provider_asks_for_until_told_more() {
        let plain = Buyer::new("Ayse", "ayse@example.test");
        assert!(plain.surname.is_none());
        assert!(plain.identity_number.is_none());

        let full = Buyer::new("Ayse", "ayse@example.test")
            .surname("Yilmaz")
            .identity_number("11111111111")
            .ip("203.0.113.7")
            .phone("+905350000000")
            .address(Address::new("Bagdat Cad. 1", "Istanbul", "Turkey").zip_code("34000"));
        assert_eq!(full.surname.as_deref(), Some("Yilmaz"));
        assert_eq!(
            full.address.expect("an address").zip_code.as_deref(),
            Some("34000")
        );
    }

    #[test]
    fn a_line_ships_unless_it_is_told_not_to() {
        let item = BasketItem::new(
            "sku-1",
            "Kahve",
            Money::from_minor_units(14_990, Currency::Try),
        );
        assert_eq!(item.kind, ItemKind::Physical);
        assert_eq!(item.quantity, 1);

        let download =
            BasketItem::new("sku-2", "PDF", Money::from_minor_units(1000, Currency::Try))
                .virtual_item()
                .quantity(3)
                .category("Kitap");
        assert_eq!(download.kind, ItemKind::Virtual);
        assert_eq!(download.quantity, 3);
        assert_eq!(download.category.as_deref(), Some("Kitap"));
    }

    #[test]
    fn a_line_total_is_the_unit_price_times_the_count() {
        let three = BasketItem::new(
            "sku-1",
            "Kahve",
            Money::from_minor_units(1499, Currency::Try),
        )
        .quantity(3);
        assert_eq!(
            three.line_total().expect("no overflow"),
            Money::from_minor_units(4497, Currency::Try)
        );
    }

    #[test]
    fn a_line_that_would_overflow_says_so_rather_than_wrapping() {
        let absurd = BasketItem::new(
            "sku-1",
            "Ev",
            Money::from_minor_units(i64::MAX, Currency::Try),
        )
        .quantity(2);
        assert!(absurd.line_total().is_err());
    }
}
