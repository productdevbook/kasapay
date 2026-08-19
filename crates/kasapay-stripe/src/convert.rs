//! Moving between kasapay's vocabulary and Stripe's.

use std::str::FromStr;

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Status};

pub(crate) const PROVIDER: ProviderId = ProviderId::STRIPE;

/// Maps a currency kasapay knows onto Stripe's, where Stripe has one.
///
/// By the ISO code rather than by a hundred-odd match arms, because
/// `stripe_types::Currency` reads one and the two lists are not the same list:
/// kasapay names currencies Stripe has never heard of and Stripe settles in
/// ones kasapay does not name.
///
/// **A code Stripe does not know is refused, not sent.** `async-stripe` reads
/// an unknown code into `Currency::Unknown` and would put it on the wire
/// happily; that is the one answer this must never pass on, because a payment
/// in a currency Stripe cannot settle is a payment that fails after the money
/// has been asked for rather than before.
///
/// The Kuwaiti dinar is that case and the reason it is worth stating: Stripe
/// names no currency with three decimal places, so a dinar has nowhere to go
/// and is refused rather than rounded into something else.
pub(crate) fn currency(currency: Currency) -> Result<stripe_types::Currency, Error> {
    let refuse = || {
        Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            format!("Stripe does not settle in {currency}"),
        )
    };
    // Reading one never fails: an unrecognised code becomes Unknown rather
    // than an error, which is exactly the answer that must not be passed on.
    let Ok(mapped) = stripe_types::Currency::from_str(&currency.code().to_ascii_lowercase()) else {
        return Err(refuse());
    };
    if matches!(mapped, stripe_types::Currency::Unknown(_)) {
        return Err(refuse());
    }
    Ok(mapped)
}

/// Maps a currency Stripe reports back onto kasapay's.
///
/// `None` for a currency kasapay does not name — including Stripe's own
/// `Unknown`, whose `Display` is whatever string arrived. Reading an amount in
/// a currency whose minor unit is unknown is how a hundredfold error is
/// written into a ledger, so it is refused rather than guessed.
pub(crate) fn currency_back(currency: &stripe_types::Currency) -> Option<Currency> {
    currency.to_string().to_ascii_uppercase().parse().ok()
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
/// Stripe says what kind of failure it was in the body, so the body is read
/// rather than the HTTP status guessed from — and the message a caller sees is
/// Stripe's own sentence rather than a Debug dump of their error struct, which
/// is what `StripeError`'s own `Display` produces.
pub(crate) fn error(error: &stripe::StripeError) -> Error {
    let stripe::StripeError::Stripe(api, status) = error else {
        let kind = match error {
            stripe::StripeError::JSONDeserialize(_) => ErrorKind::Malformed,
            stripe::StripeError::ClientError(_) | stripe::StripeError::Timeout => {
                ErrorKind::Transport
            }
            stripe::StripeError::ConfigError(_) => ErrorKind::InvalidRequest,
            stripe::StripeError::Stripe(..) => unreachable!("matched by the let-else"),
        };
        return Error::new(kind, PROVIDER, error.to_string());
    };

    let kind = match &api.type_ {
        stripe_shared::ApiErrorsType::CardError => ErrorKind::Declined,
        stripe_shared::ApiErrorsType::ApiError => ErrorKind::Provider,
        // An invalid request, a reused idempotency key with different
        // parameters, or a type this build has not met: the status says as
        // much as anything else does.
        _ => kind_for_status(*status),
    };

    let message = api
        .message
        .clone()
        .unwrap_or_else(|| format!("Stripe answered {status} with no message"));
    let error = Error::new(kind, PROVIDER, message);

    // `decline_code` is the specific reason — `insufficient_funds` — and
    // `code` the general one — `card_declined`. A shop shows the first.
    match api
        .decline_code
        .as_deref()
        .or_else(|| api.code.as_ref().map(stripe_shared::ApiErrorsCode::as_str))
    {
        Some(code) => error.with_code(code),
        None => error,
    }
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
    fn an_authentication_failure_is_not_a_bad_request() {
        // 401 and 403 both mean the key, not the request.
        assert_eq!(super::kind_for_status(401), ErrorKind::Auth);
        assert_eq!(super::kind_for_status(403), ErrorKind::Auth);
        assert!(!ErrorKind::Auth.is_retryable());
    }

    #[test]
    fn a_timeout_is_worth_retrying() {
        assert!(super::error(&stripe::StripeError::Timeout).is_retryable());
    }

    /// Whatever is sent to Stripe can be read back from Stripe.
    ///
    /// Only one of the two directions is exhaustive: `currency_back` ends in a
    /// wildcard, because Stripe names far more currencies than kasapay does.
    /// So a currency added to `currency` and forgotten in `currency_back` is
    /// invisible to the compiler, and a payment this crate opened would come
    /// back as one it has no name for.
    #[test]
    fn every_currency_stripe_settles_survives_the_round_trip() {
        for currency in Currency::KNOWN.iter().copied() {
            if let Ok(there) = super::currency(currency) {
                assert_eq!(
                    super::currency_back(&there),
                    Some(currency),
                    "{currency} is sent to Stripe and cannot be read back"
                );
            }
        }
    }

    #[test]
    fn a_currency_stripe_cannot_settle_is_refused_rather_than_rounded() {
        let error = super::currency(Currency::Kwd).expect_err("Stripe has no three-place currency");
        assert_eq!(error.kind(), ErrorKind::Unsupported);
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
