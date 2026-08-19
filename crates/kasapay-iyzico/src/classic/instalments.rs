//! What a card may be paid in instalments, and what each one costs.
//!
//! Turkish cards are paid in instalments far more often than not, and the
//! surcharge is the merchant's arrangement with the bank rather than anything
//! on a payment. `POST /payment/iyzipos/installment` is where iyzico answers
//! both: which counts this card's bank allows for this amount, and what the
//! payer pays for each.
//!
//! [`Client::instalments`](crate::classic::Client::instalments) is the call.
//!
//! # Amounts are numbers on this endpoint, and strings on every other
//!
//! iyzico types `price` as a JSON number here — `100.0`, not `"100.0"` — and
//! types it as a decimal string everywhere else in the same API. Nothing in
//! this crate reads money through an `f64`, so both directions go through the
//! literal text: the request writes the decimal string as a JSON number
//! without quoting it, and the answer's numbers are read as they were written
//! rather than through a float that would land on `33.333333333333336`.
//!
//! # No currency, either way
//!
//! iyzico's request has no currency field and neither does its answer, so
//! every amount here is in the currency the question was asked in.
//! [`Client::instalments`](crate::classic::Client::instalments) takes a
//! [`Money`] and reads the answers back in that same currency. In practice
//! this service is lira: instalments are a Turkish card feature and iyzico
//! documents no other currency for it.
//!
//! # Not signed
//!
//! iyzico's own signature list does not name this endpoint, and its documented
//! response carries no `signature` field. So the answer is as trustworthy as
//! the connection it arrived over, like the classic cancel and everything in
//! [`crate::iyzilink`]. That is fine for what it is — a price list read before
//! a payment exists — and it is worth knowing before it is shown to a payer as
//! a price.

use kasapay_core::{Error, ErrorKind, Money, ProviderId, Raw};

use crate::classic::client::{Association, CardType};

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// One card's instalment options, as iyzico calculated them.
///
/// One of these per card family that matched: a request naming a BIN answers
/// one, and a request naming only an amount answers the list of what every
/// family the merchant is set up with would charge.
#[derive(Debug, Clone)]
pub struct Instalments {
    /// The BIN iyzico calculated with, where the request named one.
    pub bin: Option<Box<str>>,
    /// The amount the calculation was made for.
    pub price: Money,
    /// Credit, debit or prepaid.
    pub card_type: Option<CardType>,
    /// The scheme the card runs on.
    pub association: Option<Association>,
    /// The issuer's own name for the product — `Bonus`, `Axess`, `World`.
    pub family: Option<Box<str>>,
    /// The issuing bank.
    pub bank_name: Option<Box<str>>,
    /// The issuing bank's code.
    pub bank_code: Option<i64>,
    /// Whether 3-D Secure is required for this card.
    ///
    /// iyzico's `force3ds`. A payment that skips it for a card marked here is
    /// one the bank will refuse, and the liability for a card not marked is
    /// still the merchant's — see [`crate::classic::saved`] for that argument.
    pub force_3ds: bool,
    /// Whether the CVC must be collected — iyzico's `forceCvc`.
    pub force_cvc: bool,
    /// Whether this is a commercial card rather than a personal one.
    pub commercial: bool,
    /// Whether the card supports dynamic currency conversion.
    pub dcc_enabled: bool,
    /// Whether this is an agricultural card, which some banks treat apart.
    pub agriculture_enabled: bool,
    /// What each allowed instalment count costs, as iyzico calculated it.
    ///
    /// **A single payment is in here too**, as the entry whose
    /// [`count`](Instalment::count) is `1`. Empty means this card cannot be
    /// paid in instalments at all through this merchant — which is an answer
    /// rather than a failure.
    pub options: Vec<Instalment>,
}

impl Instalments {
    /// What one count would cost, where iyzico allowed it.
    #[must_use]
    pub fn option(&self, count: u8) -> Option<&Instalment> {
        self.options.iter().find(|option| option.count == count)
    }

    /// The largest count iyzico allowed, or `None` where it allowed nothing.
    #[must_use]
    pub fn largest(&self) -> Option<&Instalment> {
        self.options.iter().max_by_key(|option| option.count)
    }
}

/// One instalment count, and what the payer pays for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instalment {
    /// How many instalments — iyzico's `installmentNumber`. `1` is a single
    /// payment.
    pub count: u8,
    /// What the payer pays each month — iyzico's `installmentPrice`.
    pub each: Money,
    /// What the payer pays altogether — iyzico's `totalPrice`.
    ///
    /// **This is what the payment is opened for**, not the basket total: the
    /// difference between this and the price asked about is the instalment
    /// surcharge, and it is money that moves. It is
    /// [`ChargeRequest::amount`](kasapay_core::ChargeRequest::amount)'s value
    /// for an instalment payment, with the basket total as
    /// [`Charge::order_amount`](kasapay_core::Charge::order_amount).
    pub total: Money,
}

impl Instalment {
    /// What the instalments add on top of the amount asked about.
    ///
    /// Negative where a bank charges less for instalments than for a single
    /// payment, which some do as a promotion. `None` if the two amounts are
    /// in different currencies, which iyzico's answer cannot express.
    #[must_use]
    pub fn surcharge(&self, asked: Money) -> Option<Money> {
        self.total.checked_sub(asked)
    }
}

/// Reads one of iyzico's JSON numbers as money, through its literal text.
///
/// `raw` is the number exactly as iyzico wrote it. Reading it as an `f64`
/// first is what this avoids: `33.33` through a float and back is
/// `33.329999999999998`, and [`Money::parse`] would refuse it — correctly,
/// and for the wrong reason.
///
/// A quoted number is accepted too. iyzico types this field as a number and
/// writes decimal strings on every other endpoint in the same API, so being
/// strict about the quotes would be betting on which of their two habits this
/// one follows.
pub(crate) fn number_as_money(raw: &str, currency: kasapay_core::Currency) -> Result<Money, Error> {
    let text = raw.trim().trim_matches('"');
    Money::parse(text, currency).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            format!("iyzico sent `{text}` where an amount in {currency} belongs"),
        )
        .with_source(e)
    })
}

/// Whether one of iyzico's `0`/`1` flags is set. Anything else is `false`.
pub(crate) const fn flag(value: Option<i64>) -> bool {
    matches!(value, Some(1))
}

/// The answer, kept whole, for everything this type does not model.
///
/// Not a field on [`Instalments`]: one request answers a list, and giving
/// every entry a copy of the same body would be five copies of it for five
/// card families.
#[derive(Debug, Clone)]
pub struct Options {
    /// One entry per card family iyzico answered for.
    pub cards: Vec<Instalments>,
    /// iyzico's own response, untouched.
    pub raw: Raw,
}

#[cfg(test)]
mod tests {
    use super::{Instalment, number_as_money};
    use kasapay_core::{Currency, Money};

    #[test]
    fn a_number_is_read_through_its_own_text_and_not_a_float() {
        let money = number_as_money("33.33", Currency::Try).expect("a decimal iyzico wrote");
        assert_eq!(money.minor_units(), 3333);
        // The same value quoted, which is how iyzico writes it everywhere else.
        assert_eq!(
            number_as_money("\"33.33\"", Currency::Try).expect("quoted"),
            money
        );
        // A whole number, which is how a JSON encoder writes 100.00.
        assert_eq!(
            number_as_money("100", Currency::Try)
                .expect("whole")
                .minor_units(),
            10_000
        );
    }

    #[test]
    fn precision_a_currency_does_not_have_is_refused_rather_than_rounded() {
        // What an f64 round trip does to 33.33, and what iyzico never sends.
        assert!(number_as_money("33.329999999999998", Currency::Try).is_err());
    }

    #[test]
    fn the_surcharge_is_what_the_instalments_add() {
        let asked = Money::from_minor_units(10_000, Currency::Try);
        let option = Instalment {
            count: 6,
            each: Money::from_minor_units(1834, Currency::Try),
            total: Money::from_minor_units(11_004, Currency::Try),
        };
        assert_eq!(
            option
                .surcharge(asked)
                .expect("same currency")
                .minor_units(),
            1004
        );
        // A currency the answer cannot be in is not a surcharge of anything.
        assert!(
            option
                .surcharge(Money::from_minor_units(10_000, Currency::Usd))
                .is_none()
        );
    }
}
