//! Moving between kasapay's vocabulary and Stripe's.

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Status};

pub(crate) const PROVIDER: ProviderId = ProviderId::STRIPE;

/// Maps a currency kasapay knows onto Stripe's.
pub(crate) const fn currency(currency: Currency) -> stripe_types::Currency {
    match currency {
        Currency::Try => stripe_types::Currency::TRY,
        Currency::Usd => stripe_types::Currency::USD,
        Currency::Eur => stripe_types::Currency::EUR,
        Currency::Gbp => stripe_types::Currency::GBP,
    }
}

/// Maps a currency Stripe reports back onto kasapay's.
pub(crate) fn currency_back(currency: &stripe_types::Currency) -> Option<Currency> {
    match currency {
        stripe_types::Currency::TRY => Some(Currency::Try),
        stripe_types::Currency::USD => Some(Currency::Usd),
        stripe_types::Currency::EUR => Some(Currency::Eur),
        stripe_types::Currency::GBP => Some(Currency::Gbp),
        _ => None,
    }
}

/// Reads a PaymentIntent's amount, in the currency Stripe settled it in.
pub(crate) fn amount(minor_units: i64, from: &stripe_types::Currency) -> Result<Money, Error> {
    let currency = currency_back(from).ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            format!("kasapay has no Currency for Stripe's {from:?}"),
        )
    })?;
    Ok(Money::from_minor_units(minor_units, currency))
}

/// Maps a PaymentIntent's status onto kasapay's.
///
/// `RequiresPaymentMethod` and `RequiresConfirmation` land on
/// [`Status::RequiresAction`] with `RequiresAction` itself: from the caller's
/// side all three say the same thing — stalled until the payer acts.
#[expect(
    clippy::match_same_arms,
    reason = "naming Processing is worth more than folding it into the wildcard"
)]
pub(crate) fn status(status: &stripe_shared::PaymentIntentStatus) -> Status {
    use stripe_shared::PaymentIntentStatus as S;
    match status {
        S::Canceled => Status::Canceled,
        S::RequiresCapture => Status::Authorized,
        S::Succeeded => Status::Captured,
        S::RequiresAction | S::RequiresConfirmation | S::RequiresPaymentMethod => {
            Status::RequiresAction
        }
        S::Processing => Status::Pending,
        // A status Stripe added since this build of async-stripe. Pending is
        // the only safe reading of one we cannot name.
        _ => Status::Pending,
    }
}

/// Maps a Stripe client failure onto kasapay's error.
///
/// The HTTP status is the whole signal: Stripe answers 402 for a card the
/// network refused and 400 for a request it would not have sent on.
pub(crate) fn error(error: &stripe::StripeError) -> Error {
    let kind = match error {
        stripe::StripeError::Stripe(_, code) => kind_for_status(*code),
        stripe::StripeError::JSONDeserialize(_) => ErrorKind::Malformed,
        stripe::StripeError::ClientError(_) | stripe::StripeError::Timeout => ErrorKind::Transport,
        stripe::StripeError::ConfigError(_) => ErrorKind::InvalidRequest,
    };
    Error::new(kind, PROVIDER, error.to_string())
}

/// Maps the HTTP status of a Stripe error response onto a kind.
const fn kind_for_status(code: u16) -> ErrorKind {
    match code {
        401 | 403 => ErrorKind::Auth,
        402 => ErrorKind::Declined,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    }
}

#[cfg(test)]
mod tests {
    use kasapay_core::{Currency, ErrorKind, Status};

    #[test]
    fn a_refused_card_is_a_decline_and_a_bad_request_is_not() {
        assert_eq!(super::kind_for_status(402), ErrorKind::Declined);
        assert!(!ErrorKind::Declined.is_retryable());
        assert_eq!(super::kind_for_status(400), ErrorKind::InvalidRequest);
        assert_eq!(super::kind_for_status(401), ErrorKind::Auth);
    }

    #[test]
    fn a_timeout_is_worth_retrying() {
        assert!(super::error(&stripe::StripeError::Timeout).is_retryable());
    }

    #[test]
    fn every_currency_survives_the_round_trip() {
        for currency in [Currency::Try, Currency::Usd, Currency::Eur, Currency::Gbp] {
            assert_eq!(
                super::currency_back(&super::currency(currency)),
                Some(currency)
            );
        }
    }

    #[test]
    fn the_three_stalled_statuses_collapse_to_one() {
        use stripe_shared::PaymentIntentStatus as S;
        for stalled in [
            S::RequiresAction,
            S::RequiresConfirmation,
            S::RequiresPaymentMethod,
        ] {
            assert_eq!(super::status(&stalled), Status::RequiresAction);
        }
        assert_eq!(super::status(&S::Succeeded), Status::Captured);
        assert_eq!(super::status(&S::RequiresCapture), Status::Authorized);
    }
}
