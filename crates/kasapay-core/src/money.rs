//! Amounts and currencies.
//!
//! An amount is held as an integer count of a currency's minor unit — 1050
//! kuruş rather than 10.50 TRY — because that is what every provider settles
//! in and because binary floating point cannot hold 10.10 exactly.

use std::fmt;
use std::str::FromStr;

/// A currency kasapay knows how to move money in.
///
/// Deliberately exhaustive: adding one is a breaking change, and that is the
/// point — every adapter has to say what the new currency maps to rather than
/// falling into a wildcard arm and silently doing the wrong thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Currency {
    /// Turkish lira.
    Try,
    /// United States dollar.
    Usd,
    /// Euro.
    Eur,
    /// Pound sterling.
    Gbp,
}

impl Currency {
    /// The ISO 4217 alphabetic code, uppercase.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Try => "TRY",
            Self::Usd => "USD",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
        }
    }

    /// How many decimal places the currency's minor unit sits at.
    #[must_use]
    pub const fn exponent(self) -> u32 {
        match self {
            Self::Try | Self::Usd | Self::Eur | Self::Gbp => 2,
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// The string was not a currency code kasapay supports.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported currency code: {0}")]
pub struct UnknownCurrency(pub String);

impl FromStr for Currency {
    type Err = UnknownCurrency;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "TRY" => Ok(Self::Try),
            "USD" => Ok(Self::Usd),
            "EUR" => Ok(Self::Eur),
            "GBP" => Ok(Self::Gbp),
            _ => Err(UnknownCurrency(s.to_owned())),
        }
    }
}

/// An amount in one currency, counted in that currency's minor unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    minor_units: i64,
    currency: Currency,
}

/// A decimal string could not be read as an amount in the given currency.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    /// The text was not a plain decimal number.
    #[error("`{0}` is not a decimal amount")]
    NotDecimal(String),
    /// The text carried more decimal places than the currency has.
    #[error("`{value}` has more than {exponent} decimal places for {currency}")]
    TooPrecise {
        /// The amount as it was written.
        value: String,
        /// The currency it was to be read in.
        currency: Currency,
        /// The number of decimal places the currency allows.
        exponent: u32,
    },
    /// The amount did not fit in an `i64` of minor units.
    #[error("`{0}` does not fit in 64 bits of minor units")]
    Overflow(String),
    /// The amount was zero or negative where a positive one was required.
    #[error("amount must be positive, got {0}")]
    NotPositive(i64),
}

impl Money {
    /// Builds an amount from a count of minor units — 1050 for 10.50 TRY.
    #[must_use]
    pub const fn from_minor_units(minor_units: i64, currency: Currency) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    /// Reads a plain decimal string such as `"10.50"`.
    pub fn parse(value: &str, currency: Currency) -> Result<Self, MoneyError> {
        let text = value.trim();
        let (sign, digits) = match text.strip_prefix('-') {
            Some(rest) => (-1i64, rest),
            None => (1i64, text.strip_prefix('+').unwrap_or(text)),
        };
        let (whole, frac) = match digits.split_once('.') {
            Some((w, f)) => (w, f),
            None => (digits, ""),
        };
        let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !numeric(whole) || (!frac.is_empty() && !numeric(frac)) {
            return Err(MoneyError::NotDecimal(value.to_owned()));
        }
        let exponent = currency.exponent();
        let places = u32::try_from(frac.len()).unwrap_or(u32::MAX);
        if places > exponent {
            return Err(MoneyError::TooPrecise {
                value: value.to_owned(),
                currency,
                exponent,
            });
        }
        let mut padded = String::with_capacity(whole.len() + frac.len() + 1);
        padded.push_str(whole);
        padded.push_str(frac);
        for _ in 0..(exponent - places) {
            padded.push('0');
        }
        let minor_units: i64 = padded
            .parse()
            .map_err(|_| MoneyError::Overflow(value.to_owned()))?;
        Ok(Self {
            minor_units: sign * minor_units,
            currency,
        })
    }

    /// The amount as a count of minor units.
    #[must_use]
    pub const fn minor_units(self) -> i64 {
        self.minor_units
    }

    /// The currency the amount is in.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// Fails unless the amount is greater than zero.
    pub fn require_positive(self) -> Result<Self, MoneyError> {
        if self.minor_units > 0 {
            Ok(self)
        } else {
            Err(MoneyError::NotPositive(self.minor_units))
        }
    }

    /// Renders the amount as a plain decimal string, without the currency code.
    ///
    /// This is the form providers that take a decimal amount expect: `10.50`,
    /// never `10.5` and never `1.05e1`.
    #[must_use]
    pub fn to_decimal_string(self) -> String {
        let exponent = self.currency.exponent();
        let scale = 10u64.pow(exponent);
        let sign = if self.minor_units < 0 { "-" } else { "" };
        let magnitude = self.minor_units.unsigned_abs();
        if exponent == 0 {
            return format!("{sign}{magnitude}");
        }
        format!(
            "{sign}{}.{:0>width$}",
            magnitude / scale,
            magnitude % scale,
            width = usize::try_from(exponent).unwrap_or(usize::MAX)
        )
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal_string(), self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::{Currency, Money, MoneyError};

    #[test]
    fn parses_and_renders_a_two_place_amount() {
        let money = Money::parse("10.50", Currency::Try).expect("valid amount");
        assert_eq!(money.minor_units(), 1050);
        assert_eq!(money.to_decimal_string(), "10.50");
    }

    #[test]
    fn pads_a_missing_fractional_part() {
        assert_eq!(
            Money::parse("7", Currency::Usd)
                .expect("valid")
                .minor_units(),
            700
        );
        assert_eq!(
            Money::parse("7.5", Currency::Usd)
                .expect("valid")
                .minor_units(),
            750
        );
    }

    #[test]
    fn renders_amounts_below_one_with_a_leading_zero() {
        let money = Money::from_minor_units(5, Currency::Try);
        assert_eq!(money.to_decimal_string(), "0.05");
    }

    #[test]
    fn rejects_more_precision_than_the_currency_has() {
        let err = Money::parse("10.505", Currency::Try).expect_err("too precise");
        assert!(matches!(err, MoneyError::TooPrecise { .. }));
    }

    #[test]
    fn rejects_text_that_is_not_a_number() {
        assert!(matches!(
            Money::parse("ten", Currency::Try),
            Err(MoneyError::NotDecimal(_))
        ));
        assert!(matches!(
            Money::parse("", Currency::Try),
            Err(MoneyError::NotDecimal(_))
        ));
        assert!(matches!(
            Money::parse("1.2.3", Currency::Try),
            Err(MoneyError::NotDecimal(_))
        ));
    }

    #[test]
    fn round_trips_through_its_decimal_form() {
        for minor in [1i64, 5, 99, 100, 101, 1050, 123_456_789] {
            let money = Money::from_minor_units(minor, Currency::Eur);
            let back = Money::parse(&money.to_decimal_string(), Currency::Eur).expect("valid");
            assert_eq!(back, money);
        }
    }

    #[test]
    fn require_positive_rejects_zero() {
        let zero = Money::from_minor_units(0, Currency::Try);
        assert!(matches!(
            zero.require_positive(),
            Err(MoneyError::NotPositive(0))
        ));
    }
}
