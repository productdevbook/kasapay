//! Amounts and currencies.
//!
//! An amount is held as an integer count of a currency's minor unit — 1050
//! kuruş rather than 10.50 TRY — because that is what every provider settles
//! in and because binary floating point cannot hold 10.10 exactly.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Builds [`Currency`] and its four tables from one list.
///
/// A macro rather than four hand-kept `match` blocks because there are more
/// than a hundred entries and four tables that must agree: a code, an ISO
/// numeric code, a minor-unit exponent, and the two readings back. Kept by
/// hand, one of them drifts, and the one that matters is the exponent — an
/// amount read at the wrong number of decimal places is out by a factor of a
/// hundred.
macro_rules! currencies {
    ($($(#[$meta:meta])* $variant:ident => $code:literal, $numeric:literal, $exponent:literal;)*) => {
        /// A currency kasapay knows how to move money in.
        ///
        /// # What is here, and what is not
        ///
        /// A currency is named here when ISO 4217 currently defines it, its
        /// minor unit is **exactly two decimal places**, and at least one
        /// provider in this workspace settles in it — plus the nine this
        /// library shipped with, whatever their exponent.
        ///
        /// The two-decimal rule is the safety rule rather than a tidiness one.
        /// Two decimals is the only convention nobody disagrees about. The
        /// zero- and three-decimal currencies are exactly where a provider's
        /// reading and ISO's diverge: Stripe treats Icelandic króna as having
        /// no minor unit and requires its three-decimal currencies to arrive
        /// as a multiple of ten, and Malagasy ariary is a fifth rather than a
        /// hundredth of its unit. Each of those needs a reading of that
        /// provider's own documentation before it can be named, and being
        /// wrong about one is a payment out by a factor of a hundred. The two
        /// already here — yen and Kuwaiti dinar — have had that reading.
        ///
        /// # It is still exhaustive, and a match may still not guess
        ///
        /// Adding one is still a breaking change. What changed is what an
        /// adapter must do about it: a currency match may carry a wildcard arm
        /// **only where that arm refuses**. Mapping an unknown currency onto
        /// something is the thing that was never allowed; refusing it before a
        /// socket opens is the thing this type exists for, and
        /// `crates/kasapay/tests/conformance.rs` walks every variant here past
        /// every adapter to prove each one does one or the other. What it does
        /// not prove is which code went out: providers spell them differently,
        /// and that is each adapter's own test.
        ///
        /// The list came from ISO 4217 as the `iso-codes` dataset publishes
        /// it, intersected with the currencies `async-stripe` names, on
        /// 2026-08-19.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Currency {
            $($(#[$meta])* $variant,)*
        }

        impl Currency {
            /// Every currency this library names, in code order.
            ///
            /// What a test walks to ask whether an adapter has an answer for
            /// each one. A caller does not usually want this: the currency a
            /// payment is in comes from the order, not from a list.
            pub const KNOWN: &'static [Self] = &[$(Self::$variant,)*];

            /// The ISO 4217 alphabetic code, uppercase.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)*
                }
            }

            /// The ISO 4217 numeric code, three digits, zero-padded.
            ///
            /// The standard defines this alongside the alphabetic one and some
            /// providers answer with it: iyzico's In-Store API reports lira as
            /// `0949`.
            #[must_use]
            pub const fn numeric(self) -> &'static str {
                match self {
                    $(Self::$variant => $numeric,)*
                }
            }

            /// How many decimal places the currency's minor unit sits at.
            #[must_use]
            pub const fn exponent(self) -> u32 {
                match self {
                    $(Self::$variant => $exponent,)*
                }
            }

            /// Reads an uppercase alphabetic code.
            fn from_alpha(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// Reads a numeric code already padded to three digits.
            fn from_numeric(code: &str) -> Option<Self> {
                match code {
                    $($numeric => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

currencies! {
    /// UAE Dirham.
    Aed => "AED", "784", 2;
    /// Afghani.
    Afn => "AFN", "971", 2;
    /// Lek.
    All => "ALL", "008", 2;
    /// Armenian Dram.
    Amd => "AMD", "051", 2;
    /// Netherlands Antillean Guilder.
    Ang => "ANG", "532", 2;
    /// Kwanza.
    Aoa => "AOA", "973", 2;
    /// Argentine Peso.
    Ars => "ARS", "032", 2;
    /// Australian Dollar.
    Aud => "AUD", "036", 2;
    /// Aruban Florin.
    Awg => "AWG", "533", 2;
    /// Azerbaijan Manat.
    Azn => "AZN", "944", 2;
    /// Convertible Mark.
    Bam => "BAM", "977", 2;
    /// Barbados Dollar.
    Bbd => "BBD", "052", 2;
    /// Taka.
    Bdt => "BDT", "050", 2;
    /// Bulgarian Lev.
    Bgn => "BGN", "975", 2;
    /// Bermudian Dollar.
    Bmd => "BMD", "060", 2;
    /// Brunei Dollar.
    Bnd => "BND", "096", 2;
    /// Boliviano.
    Bob => "BOB", "068", 2;
    /// Brazilian Real.
    Brl => "BRL", "986", 2;
    /// Bahamian Dollar.
    Bsd => "BSD", "044", 2;
    /// Pula.
    Bwp => "BWP", "072", 2;
    /// Belarusian Ruble.
    Byn => "BYN", "933", 2;
    /// Belize Dollar.
    Bzd => "BZD", "084", 2;
    /// Canadian Dollar.
    Cad => "CAD", "124", 2;
    /// Congolese Franc.
    Cdf => "CDF", "976", 2;
    /// Swiss Franc.
    Chf => "CHF", "756", 2;
    /// Yuan Renminbi.
    Cny => "CNY", "156", 2;
    /// Colombian Peso.
    Cop => "COP", "170", 2;
    /// Costa Rican Colon.
    Crc => "CRC", "188", 2;
    /// Cabo Verde Escudo.
    Cve => "CVE", "132", 2;
    /// Czech Koruna.
    Czk => "CZK", "203", 2;
    /// Danish Krone.
    Dkk => "DKK", "208", 2;
    /// Dominican Peso.
    Dop => "DOP", "214", 2;
    /// Algerian Dinar.
    Dzd => "DZD", "012", 2;
    /// Egyptian Pound.
    Egp => "EGP", "818", 2;
    /// Ethiopian Birr.
    Etb => "ETB", "230", 2;
    /// Euro.
    Eur => "EUR", "978", 2;
    /// Fiji Dollar.
    Fjd => "FJD", "242", 2;
    /// Falkland Islands Pound.
    Fkp => "FKP", "238", 2;
    /// Pound Sterling.
    Gbp => "GBP", "826", 2;
    /// Lari.
    Gel => "GEL", "981", 2;
    /// Gibraltar Pound.
    Gip => "GIP", "292", 2;
    /// Dalasi.
    Gmd => "GMD", "270", 2;
    /// Quetzal.
    Gtq => "GTQ", "320", 2;
    /// Guyana Dollar.
    Gyd => "GYD", "328", 2;
    /// Hong Kong Dollar.
    Hkd => "HKD", "344", 2;
    /// Lempira.
    Hnl => "HNL", "340", 2;
    /// Kuna.
    Hrk => "HRK", "191", 2;
    /// Gourde.
    Htg => "HTG", "332", 2;
    /// Forint.
    Huf => "HUF", "348", 2;
    /// Rupiah.
    Idr => "IDR", "360", 2;
    /// New Israeli Sheqel.
    Ils => "ILS", "376", 2;
    /// Indian Rupee.
    Inr => "INR", "356", 2;
    /// Jamaican Dollar.
    Jmd => "JMD", "388", 2;
    /// Japanese yen, which has no minor unit at all.
    Jpy => "JPY", "392", 0;
    /// Kenyan Shilling.
    Kes => "KES", "404", 2;
    /// Som.
    Kgs => "KGS", "417", 2;
    /// Riel.
    Khr => "KHR", "116", 2;
    /// Kuwaiti dinar, whose minor unit is a thousandth.
    Kwd => "KWD", "414", 3;
    /// Cayman Islands Dollar.
    Kyd => "KYD", "136", 2;
    /// Tenge.
    Kzt => "KZT", "398", 2;
    /// Lao Kip.
    Lak => "LAK", "418", 2;
    /// Lebanese Pound.
    Lbp => "LBP", "422", 2;
    /// Sri Lanka Rupee.
    Lkr => "LKR", "144", 2;
    /// Liberian Dollar.
    Lrd => "LRD", "430", 2;
    /// Loti.
    Lsl => "LSL", "426", 2;
    /// Moroccan Dirham.
    Mad => "MAD", "504", 2;
    /// Moldovan Leu.
    Mdl => "MDL", "498", 2;
    /// Denar.
    Mkd => "MKD", "807", 2;
    /// Kyat.
    Mmk => "MMK", "104", 2;
    /// Tugrik.
    Mnt => "MNT", "496", 2;
    /// Pataca.
    Mop => "MOP", "446", 2;
    /// Mauritius Rupee.
    Mur => "MUR", "480", 2;
    /// Rufiyaa.
    Mvr => "MVR", "462", 2;
    /// Malawi Kwacha.
    Mwk => "MWK", "454", 2;
    /// Mexican Peso.
    Mxn => "MXN", "484", 2;
    /// Malaysian Ringgit.
    Myr => "MYR", "458", 2;
    /// Mozambique Metical.
    Mzn => "MZN", "943", 2;
    /// Namibia Dollar.
    Nad => "NAD", "516", 2;
    /// Naira.
    Ngn => "NGN", "566", 2;
    /// Cordoba Oro.
    Nio => "NIO", "558", 2;
    /// Norwegian Krone.
    Nok => "NOK", "578", 2;
    /// Nepalese Rupee.
    Npr => "NPR", "524", 2;
    /// New Zealand Dollar.
    Nzd => "NZD", "554", 2;
    /// Balboa.
    Pab => "PAB", "590", 2;
    /// Sol.
    Pen => "PEN", "604", 2;
    /// Kina.
    Pgk => "PGK", "598", 2;
    /// Philippine Peso.
    Php => "PHP", "608", 2;
    /// Pakistan Rupee.
    Pkr => "PKR", "586", 2;
    /// Zloty.
    Pln => "PLN", "985", 2;
    /// Qatari Rial.
    Qar => "QAR", "634", 2;
    /// Romanian Leu.
    Ron => "RON", "946", 2;
    /// Serbian Dinar.
    Rsd => "RSD", "941", 2;
    /// Russian Ruble.
    Rub => "RUB", "643", 2;
    /// Saudi Riyal.
    Sar => "SAR", "682", 2;
    /// Solomon Islands Dollar.
    Sbd => "SBD", "090", 2;
    /// Seychelles Rupee.
    Scr => "SCR", "690", 2;
    /// Swedish Krona.
    Sek => "SEK", "752", 2;
    /// Singapore Dollar.
    Sgd => "SGD", "702", 2;
    /// Saint Helena Pound.
    Shp => "SHP", "654", 2;
    /// Somali Shilling.
    Sos => "SOS", "706", 2;
    /// Surinam Dollar.
    Srd => "SRD", "968", 2;
    /// El Salvador Colon.
    Svc => "SVC", "222", 2;
    /// Lilangeni.
    Szl => "SZL", "748", 2;
    /// Baht.
    Thb => "THB", "764", 2;
    /// Somoni.
    Tjs => "TJS", "972", 2;
    /// Pa’anga.
    Top => "TOP", "776", 2;
    /// Turkish lira.
    Try => "TRY", "949", 2;
    /// Trinidad and Tobago Dollar.
    Ttd => "TTD", "780", 2;
    /// New Taiwan Dollar.
    Twd => "TWD", "901", 2;
    /// Tanzanian Shilling.
    Tzs => "TZS", "834", 2;
    /// Hryvnia.
    Uah => "UAH", "980", 2;
    /// US Dollar.
    Usd => "USD", "840", 2;
    /// Peso Uruguayo.
    Uyu => "UYU", "858", 2;
    /// Uzbekistan Sum.
    Uzs => "UZS", "860", 2;
    /// Tala.
    Wst => "WST", "882", 2;
    /// East Caribbean Dollar.
    Xcd => "XCD", "951", 2;
    /// Yemeni Rial.
    Yer => "YER", "886", 2;
    /// Rand.
    Zar => "ZAR", "710", 2;
    /// Zambian Kwacha.
    Zmw => "ZMW", "967", 2;
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

    /// Reads either ISO 4217 code, alphabetic or numeric.
    ///
    /// Both, because providers answer with both: iyzico's In-Store API reports
    /// lira as `0949` where every other API of theirs writes `TRY`. The two
    /// cannot be confused — one is three letters and the other three digits —
    /// and a numeric code is read whatever it is padded to, since `0949`,
    /// `949` and ISO's own `008` for the lek are the same number written three
    /// ways.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let unknown = || UnknownCurrency(s.to_owned());
        if !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit()) {
            let digits = trimmed.trim_start_matches('0');
            if digits.len() > 3 {
                return Err(unknown());
            }
            let mut padded = String::from("000");
            padded.truncate(3 - digits.len());
            padded.push_str(digits);
            return Self::from_numeric(&padded).ok_or_else(unknown);
        }
        Self::from_alpha(&trimmed.to_ascii_uppercase()).ok_or_else(unknown)
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
    /// Two amounts in different currencies were combined.
    #[error("cannot combine {left} with {right}")]
    CurrencyMismatch {
        /// The currency of the amount on the left.
        left: Currency,
        /// The currency of the amount on the right.
        right: Currency,
    },
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

    /// Adds another amount in the same currency.
    ///
    /// Fails on a currency mismatch, and on the overflow that would otherwise
    /// wrap a total round to a negative one.
    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        self.minor_units
            .checked_add(other.minor_units)
            .map(|minor_units| Self {
                minor_units,
                currency: self.currency,
            })
            .ok_or_else(|| MoneyError::Overflow(format!("{self} + {other}")))
    }

    /// Subtracts another amount in the same currency.
    ///
    /// The result may be negative, because a ledger needs it to be: an
    /// over-refund is a number somebody has to see, not one to clamp away.
    /// [`Money::require_positive`] is what refuses it where it must be refused.
    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        self.same_currency(other)?;
        self.minor_units
            .checked_sub(other.minor_units)
            .map(|minor_units| Self {
                minor_units,
                currency: self.currency,
            })
            .ok_or_else(|| MoneyError::Overflow(format!("{self} - {other}")))
    }

    /// Multiplies by a count — a unit price by how many were bought.
    ///
    /// Fails on the overflow that would otherwise wrap a line total round to a
    /// negative one.
    pub fn checked_mul(self, count: u32) -> Result<Self, MoneyError> {
        self.minor_units
            .checked_mul(i64::from(count))
            .map(|minor_units| Self {
                minor_units,
                currency: self.currency,
            })
            .ok_or_else(|| MoneyError::Overflow(format!("{self} x {count}")))
    }

    /// Whether the amount is exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.minor_units == 0
    }

    fn same_currency(self, other: Self) -> Result<(), MoneyError> {
        if self.currency == other.currency {
            Ok(())
        } else {
            Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            })
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

/// Orders two amounts, and refuses to order two currencies.
///
/// `partial_cmp` answers `None` across currencies, because ten lira and ten
/// dollars have no order. Deriving [`Ord`] would invent one out of the
/// declaration order of [`Currency`], which is how a comparison quietly starts
/// answering a question nobody asked.
impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (self.currency == other.currency).then(|| self.minor_units.cmp(&other.minor_units))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.to_decimal_string(), self.currency)
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

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
    fn a_currency_with_no_minor_unit_never_grows_a_decimal_point() {
        let money = Money::parse("1200", Currency::Jpy).expect("valid amount");
        assert_eq!(money.minor_units(), 1200);
        assert_eq!(money.to_decimal_string(), "1200");
        assert!(matches!(
            Money::parse("1200.50", Currency::Jpy),
            Err(MoneyError::TooPrecise { .. })
        ));
    }

    #[test]
    fn a_numeric_iso_code_reads_as_the_currency_it_names() {
        // What iyzico's In-Store API actually answers, zero-padded.
        assert_eq!("0949".parse(), Ok(Currency::Try));
        assert_eq!("949".parse(), Ok(Currency::Try));
        assert_eq!("643".parse(), Ok(Currency::Rub));
        assert_eq!("TRY".parse(), Ok(Currency::Try));
        assert_eq!("try".parse(), Ok(Currency::Try));
    }

    #[test]
    fn a_number_that_names_no_currency_is_refused_rather_than_guessed() {
        assert!("999".parse::<Currency>().is_err());
        assert!("0".parse::<Currency>().is_err());
        assert!("94".parse::<Currency>().is_err());
        // Not a number and not a code — neither branch may claim it.
        assert!("9X9".parse::<Currency>().is_err());
    }

    #[test]
    fn a_three_place_currency_keeps_all_three() {
        let money = Money::parse("1.500", Currency::Kwd).expect("valid amount");
        assert_eq!(money.minor_units(), 1500);
        assert_eq!(money.to_decimal_string(), "1.500");
        assert_eq!(
            Money::parse("1.5", Currency::Kwd)
                .expect("valid amount")
                .minor_units(),
            1500
        );
        assert!(matches!(
            Money::parse("1.5005", Currency::Kwd),
            Err(MoneyError::TooPrecise { .. })
        ));
    }

    /// The nine this library shipped with, whose minor unit is settled and
    /// whose exponent is therefore allowed to be something other than two.
    const SHIPPED_WITH: &[Currency] = &[
        Currency::Try,
        Currency::Usd,
        Currency::Eur,
        Currency::Gbp,
        Currency::Jpy,
        Currency::Kwd,
        Currency::Rub,
        Currency::Chf,
        Currency::Nok,
    ];

    /// The rule the list is chosen by, asserted rather than trusted.
    ///
    /// A currency whose minor unit is not two decimal places is where a
    /// provider's reading and ISO's diverge, and being wrong about one is a
    /// payment out by a factor of a hundred. Adding one anyway is allowed —
    /// it just has to be a decision somebody made, which is what failing here
    /// forces.
    #[test]
    fn nothing_but_the_nine_it_shipped_with_has_an_unusual_minor_unit() {
        for currency in Currency::KNOWN.iter().copied() {
            if SHIPPED_WITH.contains(&currency) {
                continue;
            }
            assert_eq!(
                currency.exponent(),
                2,
                "{currency} has {} decimal places and no reading to say whose",
                currency.exponent()
            );
        }
    }

    #[test]
    fn every_currency_reads_back_from_both_of_its_codes() {
        for currency in Currency::KNOWN.iter().copied() {
            assert_eq!(
                currency.code().parse::<Currency>().expect("its own code"),
                currency
            );
            assert_eq!(
                currency
                    .numeric()
                    .parse::<Currency>()
                    .expect("its own code"),
                currency
            );
            // The padding iyzico uses, and the unpadded form ISO's own tables
            // are sometimes printed in.
            assert_eq!(
                format!("0{}", currency.numeric())
                    .parse::<Currency>()
                    .expect("padded"),
                currency
            );
            assert_eq!(
                currency
                    .numeric()
                    .trim_start_matches('0')
                    .parse::<Currency>()
                    .expect("unpadded"),
                currency
            );
        }
    }

    /// Two currencies sharing a code would make one of them unreachable
    /// through `FromStr`, and which one is whichever the `match` reached first.
    #[test]
    fn no_two_currencies_share_a_code() {
        let mut alpha: Vec<&str> = Currency::KNOWN.iter().map(|c| c.code()).collect();
        let mut numeric: Vec<&str> = Currency::KNOWN.iter().map(|c| c.numeric()).collect();
        let total = Currency::KNOWN.len();
        alpha.sort_unstable();
        alpha.dedup();
        numeric.sort_unstable();
        numeric.dedup();
        assert_eq!(
            alpha.len(),
            total,
            "two currencies share an alphabetic code"
        );
        assert_eq!(numeric.len(), total, "two currencies share a numeric code");
    }

    #[test]
    fn every_code_is_the_shape_iso_writes_it_in() {
        for currency in Currency::KNOWN.iter().copied() {
            let code = currency.code();
            assert!(
                code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()),
                "{code} is not three uppercase letters"
            );
            let numeric = currency.numeric();
            assert!(
                numeric.len() == 3 && numeric.bytes().all(|b| b.is_ascii_digit()),
                "{numeric} is not three digits"
            );
        }
    }

    #[test]
    fn round_trips_through_its_decimal_form() {
        for currency in [
            Currency::Try,
            Currency::Usd,
            Currency::Eur,
            Currency::Gbp,
            Currency::Jpy,
            Currency::Kwd,
        ] {
            for minor in [1i64, 5, 99, 100, 101, 1050, 123_456_789] {
                let money = Money::from_minor_units(minor, currency);
                let back = Money::parse(&money.to_decimal_string(), currency).expect("valid");
                assert_eq!(back, money);
            }
        }
    }

    #[test]
    fn amounts_in_one_currency_add_and_subtract() {
        let ten = Money::parse("10.00", Currency::Try).expect("valid");
        let three = Money::parse("3.50", Currency::Try).expect("valid");
        assert_eq!(
            ten.checked_add(three).expect("same currency"),
            Money::parse("13.50", Currency::Try).expect("valid")
        );
        assert_eq!(
            ten.checked_sub(three).expect("same currency"),
            Money::parse("6.50", Currency::Try).expect("valid")
        );
    }

    #[test]
    fn combining_two_currencies_is_an_error_rather_than_a_sum() {
        let lira = Money::parse("10.00", Currency::Try).expect("valid");
        let dollars = Money::parse("10.00", Currency::Usd).expect("valid");
        assert!(matches!(
            lira.checked_add(dollars),
            Err(MoneyError::CurrencyMismatch {
                left: Currency::Try,
                right: Currency::Usd,
            })
        ));
        assert!(lira.checked_sub(dollars).is_err());
    }

    #[test]
    #[expect(
        clippy::neg_cmp_op_on_partial_ord,
        reason = "asserting that both directions are false is the whole test"
    )]
    fn two_currencies_have_no_order_in_either_direction() {
        let lira = Money::parse("10.00", Currency::Try).expect("valid");
        let dollars = Money::parse("10.00", Currency::Usd).expect("valid");
        assert!(lira.partial_cmp(&dollars).is_none());
        // Checking only one direction would pass for a type that had silently
        // become totally ordered.
        assert!(!(lira < dollars));
        assert!(!(lira >= dollars));
        assert_ne!(lira, dollars);
    }

    #[test]
    fn one_currency_orders_by_amount() {
        let small = Money::parse("3.50", Currency::Try).expect("valid");
        let large = Money::parse("10.00", Currency::Try).expect("valid");
        assert!(small < large);
        assert!(large >= small);
        assert_eq!(small.partial_cmp(&large), Some(Ordering::Less));
        // No `Money::max`: that comes from Ord, which this type deliberately
        // does not have.
    }

    #[test]
    fn subtracting_past_zero_is_negative_and_still_refused_where_it_matters() {
        let three = Money::parse("3.50", Currency::Try).expect("valid");
        let ten = Money::parse("10.00", Currency::Try).expect("valid");
        let owed = three.checked_sub(ten).expect("same currency");
        assert_eq!(owed.minor_units(), -650);
        assert_eq!(owed.to_decimal_string(), "-6.50");
        assert!(owed.require_positive().is_err());
    }

    #[test]
    fn overflow_is_an_error_rather_than_a_wrap() {
        let huge = Money::from_minor_units(i64::MAX, Currency::Try);
        let one = Money::from_minor_units(1, Currency::Try);
        assert!(matches!(
            huge.checked_add(one),
            Err(MoneyError::Overflow(_))
        ));
        let lowest = Money::from_minor_units(i64::MIN, Currency::Try);
        assert!(matches!(
            lowest.checked_sub(one),
            Err(MoneyError::Overflow(_))
        ));
    }

    #[test]
    fn zero_knows_itself() {
        assert!(Money::from_minor_units(0, Currency::Try).is_zero());
        assert!(!Money::from_minor_units(-1, Currency::Try).is_zero());
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
