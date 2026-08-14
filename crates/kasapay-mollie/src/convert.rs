//! Moving between kasapay's vocabulary and Mollie's.

use kasapay_core::{Currency, Error, ErrorKind, Money, Status};

use crate::MOLLIE;
use crate::wire;

/// What Mollie calls a currency, where Mollie takes it at all.
///
/// Mollie settles in twenty-seven currencies and kasapay names nine, and the
/// two lists are not nested: **Mollie takes neither lira nor Kuwaiti dinar**,
/// which is to say the home currency of the two Turkish providers in this
/// workspace and the one currency with three decimal places.
///
/// Refused here rather than sent. The same mistake shipped in the PayTR
/// adapter as an empty currency field that went out over the wire and was
/// signed into the request hash.
///
/// From Mollie's [multicurrency table][], which is where the list is — their
/// OpenAPI document describes `currency` as "a three-character ISO 4217
/// currency code" and enumerates nothing.
///
/// [multicurrency table]: https://docs.mollie.com/docs/multicurrency
pub(crate) fn currency(currency: Currency) -> Result<&'static str, Error> {
    match currency {
        Currency::Usd => Ok("USD"),
        Currency::Eur => Ok("EUR"),
        Currency::Gbp => Ok("GBP"),
        Currency::Jpy => Ok("JPY"),
        Currency::Rub => Ok("RUB"),
        Currency::Chf => Ok("CHF"),
        Currency::Nok => Ok("NOK"),
        Currency::Try | Currency::Kwd => Err(Error::new(
            ErrorKind::Unsupported,
            MOLLIE,
            format!("Mollie does not settle in {currency}"),
        )),
    }
}

/// What Mollie calls a currency, read back.
///
/// The seven [`currency`] sends. A payment created outside this crate may be
/// in one of the twenty others, and that is
/// [`ErrorKind::Unsupported`] rather than a guess: `Money` cannot hold an
/// amount whose minor unit it does not know.
pub(crate) fn currency_back(code: &str) -> Result<Currency, Error> {
    match code {
        "USD" => Ok(Currency::Usd),
        "EUR" => Ok(Currency::Eur),
        "GBP" => Ok(Currency::Gbp),
        "JPY" => Ok(Currency::Jpy),
        "RUB" => Ok(Currency::Rub),
        "CHF" => Ok(Currency::Chf),
        "NOK" => Ok(Currency::Nok),
        other => Err(Error::new(
            ErrorKind::Unsupported,
            MOLLIE,
            format!("kasapay has no Currency for Mollie's {other}"),
        )),
    }
}

/// Writes an amount the way Mollie takes one: the currency code and a decimal
/// string with exactly the places that currency has.
pub(crate) fn amount_out(money: Money) -> Result<wire::AmountOut, Error> {
    Ok(wire::AmountOut {
        currency: currency(money.currency())?,
        value: money.to_decimal_string(),
    })
}

/// Reads one of Mollie's amount objects.
pub(crate) fn money(amount: &wire::Amount, what: &str) -> Result<Money, Error> {
    let code = amount.currency.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            MOLLIE,
            format!("the amount on {what} carried no currency"),
        )
    })?;
    let value = amount.value.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            MOLLIE,
            format!("the amount on {what} carried no value"),
        )
    })?;
    let currency = currency_back(code)?;
    Money::parse(value, currency).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            MOLLIE,
            format!("Mollie sent `{value}` as an amount in {currency}"),
        )
        .with_source(e)
    })
}

/// Maps a payment's status onto kasapay's.
///
/// Six of Mollie's seven land somewhere exact. **`expired` does not.** It is a
/// payment the payer never finished and can no longer finish, which is neither
/// a refusal nor a withdrawal, and [`Status`] has no word for it. It arrives
/// here as [`Status::Failed`] — final, no money moved — and the real one is in
/// [`Charge::raw`](kasapay_core::Charge::raw). A caller that needs to tell an
/// abandoned checkout from a declined card reads it there.
///
/// Mollie's own document allows a status this list has not met. `Pending` is
/// the only safe reading of one: it says the payment may still change, so a
/// caller asks again rather than shipping.
pub(crate) fn status(status: &str) -> Status {
    match status {
        "open" => Status::RequiresAction,
        "authorized" => Status::Authorized,
        "paid" => Status::Captured,
        "canceled" => Status::Canceled,
        "failed" | "expired" => Status::Failed,
        _ => Status::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::{currency, currency_back, status};
    use kasapay_core::{Currency, ErrorKind, Status};

    /// Whatever is sent to Mollie can be read back from Mollie.
    ///
    /// The two directions are separate matches and only one of them is
    /// exhaustive: `currency_back` matches on text, so the compiler cannot see
    /// a currency missing from it.
    #[test]
    fn every_currency_mollie_is_sent_can_be_read_back() {
        for money in every_currency() {
            if let Ok(sent) = currency(money) {
                assert_eq!(
                    currency_back(sent).ok(),
                    Some(money),
                    "{money} is sent to Mollie as `{sent}` and cannot be read back"
                );
            }
        }
    }

    /// The two Mollie does not take are refused rather than guessed at.
    #[test]
    fn lira_and_dinar_have_no_mollie_currency() {
        for money in [Currency::Try, Currency::Kwd] {
            let error = currency(money).expect_err("Mollie does not settle in it");
            assert_eq!(error.kind(), ErrorKind::Unsupported);
        }
    }

    #[test]
    fn a_currency_mollie_settles_in_and_kasapay_cannot_name_is_not_guessed() {
        // Swedish krona: on Mollie's list, not in `Currency`.
        let error = currency_back("SEK").expect_err("kasapay cannot name it");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    /// Every one of these is a value from Mollie's own `payment-status` enum.
    #[test]
    fn each_documented_status_lands_somewhere_it_belongs() {
        assert_eq!(status("open"), Status::RequiresAction);
        assert_eq!(status("pending"), Status::Pending);
        assert_eq!(status("authorized"), Status::Authorized);
        assert_eq!(status("paid"), Status::Captured);
        assert_eq!(status("canceled"), Status::Canceled);
        assert_eq!(status("failed"), Status::Failed);
        // Neither refused nor withdrawn, and `Status` has no third word.
        assert_eq!(status("expired"), Status::Failed);
        // Mollie's document allows one this build has not met.
        assert_eq!(status("something_new"), Status::Pending);
        assert!(status("something_new").is_open());
    }

    /// Every currency there is.
    ///
    /// The `match` is what keeps this list honest: adding a variant to
    /// `Currency` stops it compiling until somebody comes here and decides
    /// whether Mollie takes it.
    fn every_currency() -> Vec<Currency> {
        let every = vec![
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
        for money in &every {
            match money {
                Currency::Try
                | Currency::Usd
                | Currency::Eur
                | Currency::Gbp
                | Currency::Jpy
                | Currency::Kwd
                | Currency::Rub
                | Currency::Chf
                | Currency::Nok => {}
            }
        }
        every
    }
}
