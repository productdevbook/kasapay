//! Moving between kasapay's vocabulary and PayPal's.

use kasapay_core::{Currency, Error, ErrorKind, Money, Status};

use crate::PAYPAL;
use crate::wire;

/// What PayPal calls a currency, where PayPal takes it at all.
///
/// PayPal's [currency codes reference][codes] lists twenty-five currencies
/// and this sends seven of them — the same seven Mollie takes, and everything
/// else kasapay names is refused here rather than sent, before a socket opens.
/// **PayPal settles in neither Turkish lira nor Kuwaiti dinar.**
///
/// PayPal's `checkout_orders_v2` OpenAPI document types `currency_code` as a
/// bare three-letter string with no `enum`, so the list comes from their
/// reference page rather than the schema, the same gap Mollie's document has.
///
/// [codes]: https://developer.paypal.com/api/rest/reference/currency-codes/
pub(crate) fn currency(currency: Currency) -> Result<&'static str, Error> {
    match currency {
        Currency::Usd => Ok("USD"),
        Currency::Eur => Ok("EUR"),
        Currency::Gbp => Ok("GBP"),
        Currency::Jpy => Ok("JPY"),
        Currency::Rub => Ok("RUB"),
        Currency::Chf => Ok("CHF"),
        Currency::Nok => Ok("NOK"),
        _ => Err(Error::new(
            ErrorKind::Unsupported,
            PAYPAL,
            format!("PayPal does not settle in {currency}"),
        )),
    }
}

/// What PayPal calls a currency, read back.
///
/// The seven [`currency`] sends. An order created outside this crate may
/// carry one of PayPal's other eighteen, and that is
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
            PAYPAL,
            format!("kasapay has no Currency for PayPal's {other}"),
        )),
    }
}

/// Writes an amount the way PayPal takes one: the currency code and a decimal
/// string with exactly the places that currency has — no decimal point at all
/// for JPY, which is the one PayPal-supported currency this crate also names
/// that has none.
pub(crate) fn amount_out(money: Money) -> Result<wire::AmountOut, Error> {
    Ok(wire::AmountOut {
        currency_code: currency(money.currency())?,
        value: money.to_decimal_string(),
    })
}

/// Reads one of PayPal's `money` objects.
pub(crate) fn money(amount: &wire::AmountIn, what: &str) -> Result<Money, Error> {
    let code = amount.currency_code.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("the amount on {what} carried no currency_code"),
        )
    })?;
    let value = amount.value.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("the amount on {what} carried no value"),
        )
    })?;
    let currency = currency_back(code)?;
    Money::parse(value, currency).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("PayPal sent `{value}` as an amount in {currency}"),
        )
        .with_source(e)
    })
}

/// Maps an order's own status onto kasapay's — read only when nothing on the
/// order names a capture or an authorization to read a sharper status from.
/// See the crate docs for why that is the common case rather than the
/// exception.
pub(crate) fn order_status(status: &str) -> Status {
    match status {
        "CREATED" | "SAVED" | "PAYER_ACTION_REQUIRED" => Status::RequiresAction,
        // The payer has approved and nothing has been captured yet — poised
        // for Provider::capture, the closest of kasapay's six words to what
        // PayPal calls APPROVED.
        "APPROVED" => Status::Authorized,
        "COMPLETED" => Status::Captured,
        "VOIDED" => Status::Canceled,
        _ => Status::Pending,
    }
}

/// Maps a capture's own status onto kasapay's.
///
/// **`REFUNDED` and `PARTIALLY_REFUNDED` both land on [`Status::Captured`].**
/// PayPal is the first provider in this workspace whose capture carries a
/// refund outcome inside the same enum as its capture status — see
/// `kasapay_core::Status`'s own documentation, "Nothing here says a payment
/// was refunded": the money was taken, which is what [`Status::Captured`]
/// says, and that some of it went back afterwards is not a fact
/// [`Status`] has a word for. It is still on [`Charge::raw`](kasapay_core::Charge::raw).
pub(crate) fn capture_status(status: &str) -> Status {
    match status {
        "COMPLETED" | "PARTIALLY_REFUNDED" | "REFUNDED" => Status::Captured,
        "DECLINED" | "FAILED" => Status::Failed,
        // "PENDING", and anything PayPal starts sending that this build has
        // not met, both read as open rather than settled.
        _ => Status::Pending,
    }
}

/// Maps an authorization's own status onto kasapay's.
///
/// This crate never creates one — see the crate docs for why `intent:
/// AUTHORIZE` is out of scope — but [`Provider::charge_status`](kasapay_core::Provider::charge_status)
/// can still be asked about an order somebody else created that way, and
/// answering nothing for it would be worse than reading what PayPal sent.
pub(crate) fn authorization_status(status: &str) -> Status {
    match status {
        "CREATED" | "PENDING" => Status::Authorized,
        "CAPTURED" | "PARTIALLY_CAPTURED" => Status::Captured,
        "DENIED" => Status::Failed,
        "VOIDED" => Status::Canceled,
        _ => Status::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::{authorization_status, capture_status, currency, currency_back, order_status};
    use kasapay_core::{Currency, ErrorKind, Status};

    /// Whatever is sent to PayPal can be read back from PayPal.
    ///
    /// The two directions are separate matches and only one of them is
    /// exhaustive: `currency_back` matches on text, so the compiler cannot see
    /// a currency missing from it.
    #[test]
    fn every_currency_paypal_is_sent_can_be_read_back() {
        for money in Currency::KNOWN.iter().copied() {
            if let Ok(sent) = currency(money) {
                assert_eq!(
                    currency_back(sent).ok(),
                    Some(money),
                    "{money} is sent to PayPal as `{sent}` and cannot be read back"
                );
            }
        }
    }

    /// The two PayPal does not take are refused rather than guessed at.
    #[test]
    fn lira_and_dinar_have_no_paypal_currency() {
        for money in [Currency::Try, Currency::Kwd] {
            let error = currency(money).expect_err("PayPal does not settle in it");
            assert_eq!(error.kind(), ErrorKind::Unsupported);
        }
    }

    #[test]
    fn a_currency_paypal_settles_in_and_kasapay_cannot_name_is_not_guessed() {
        // Swedish krona: on PayPal's list, not in `Currency`.
        let error = currency_back("SEK").expect_err("kasapay cannot name it");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    /// Every value PayPal's `order_status` schema documents.
    #[test]
    fn each_documented_order_status_lands_somewhere_it_belongs() {
        assert_eq!(order_status("CREATED"), Status::RequiresAction);
        assert_eq!(order_status("SAVED"), Status::RequiresAction);
        assert_eq!(
            order_status("PAYER_ACTION_REQUIRED"),
            Status::RequiresAction
        );
        assert_eq!(order_status("APPROVED"), Status::Authorized);
        assert_eq!(order_status("COMPLETED"), Status::Captured);
        assert_eq!(order_status("VOIDED"), Status::Canceled);
    }

    /// Every value PayPal's `capture_status` schema documents.
    #[test]
    fn each_documented_capture_status_lands_somewhere_it_belongs() {
        assert_eq!(capture_status("COMPLETED"), Status::Captured);
        assert_eq!(capture_status("PENDING"), Status::Pending);
        assert_eq!(capture_status("DECLINED"), Status::Failed);
        assert_eq!(capture_status("FAILED"), Status::Failed);
        // Both refund outcomes: the money was taken, and Status has no third
        // word for "and then given back".
        assert_eq!(capture_status("PARTIALLY_REFUNDED"), Status::Captured);
        assert_eq!(capture_status("REFUNDED"), Status::Captured);
    }

    /// Every value PayPal's `authorization_status` schema documents.
    #[test]
    fn each_documented_authorization_status_lands_somewhere_it_belongs() {
        assert_eq!(authorization_status("CREATED"), Status::Authorized);
        assert_eq!(authorization_status("PENDING"), Status::Authorized);
        assert_eq!(authorization_status("CAPTURED"), Status::Captured);
        assert_eq!(authorization_status("PARTIALLY_CAPTURED"), Status::Captured);
        assert_eq!(authorization_status("DENIED"), Status::Failed);
        assert_eq!(authorization_status("VOIDED"), Status::Canceled);
    }
}
