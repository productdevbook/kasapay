//! What iyzico's error codes mean.
//!
//! Every refusal arrives as `status: "failure"` with an `errorCode`, and until
//! this module they all became [`ErrorKind::InvalidRequest`] — so a caller
//! could not tell a declined card from a malformed request, and a bank outage
//! looked like something retrying would never fix.
//!
//! From <https://docs.iyzico.com/en/add-ons/error-codes>.
//!
//! # iyzico's own `Retry` column is not this
//!
//! Their error list has a `Retry` flag, and it says `true` for
//! "Email is mandatory". It means *the payer can correct this and try
//! again*, not *this same request may succeed*. Mapping it onto
//! [`Error::is_retryable`](kasapay_core::Error::is_retryable) would tell a
//! caller's retry loop to hammer a request that will fail identically forever.
//! Only the bank-side codes below are retried.

use kasapay_core::ErrorKind;

/// The bank could not be reached, or asked us to come back later.
///
/// Every one of these is documented as a timeout, a communication failure, or
/// "Error occurred at the bank, please try again later".
const RETRYABLE: &[&str] = &[
    "10214", // COMMUNICATION_OR_SYSTEM_ERROR
    "10216", // NO_SUCH_ISSUER — documented as a temporary interruption
    "10219", // REQUEST_TIMEOUT
    "10228", // ISSUER_OR_SWITCH_INOPERATIVE
    "10240", // TIMEOUT
    "10241", // REMOTE_ERROR
    "10242", // LOCAL_ERROR
    "10246", // REFUND_DECLINE — "cannot currently … please try again later"
];

/// The bank understood and said no.
///
/// A caller shows these to the payer: try another card, ring your bank. They
/// are not a fault in the request, and telling a payer their details were
/// invalid when their limit was exceeded sends them round a loop that cannot end.
const DECLINED: &[&str] = &[
    "10034", // FRAUD_SUSPECT
    "10043", // LOST_CARD
    "10051", // NOT_SUFFICIENT_FUNDS
    "10093", // RESTRICTED_BY_LAW
    "10209", // BLOCKED_CARD
    "10220", // DECLINED
    "10224", // EXCEEDS_WITHDRAWAL_AMOUNT_LIMIT
    "10225", // RESTRICTED_CARD
    "10226", // EXCEEDS_ALLOWABLE_PIN_TRIES
    "10230", // REQUEST_BLOCKED_BY_BANK
    "10234", // NOT_SUFFICIENT_AWARD
    "10243", // LOCAL_DECLINE
    "10244", // REMOTE_DECLINE
    "10248", // EXCEEDED_TRANSACTION_AMOUNT
];

/// What an `errorCode` means, with `fallback` for one this list does not know.
#[must_use]
pub(crate) fn kind_for(code: Option<&str>, fallback: ErrorKind) -> ErrorKind {
    match code {
        Some(code) if RETRYABLE.contains(&code) => ErrorKind::Provider,
        Some(code) if DECLINED.contains(&code) => ErrorKind::Declined,
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::{DECLINED, RETRYABLE, kind_for};
    use kasapay_core::ErrorKind;

    #[test]
    fn a_bank_that_asked_us_to_come_back_is_retryable() {
        // 10219 is REQUEST_TIMEOUT: "Error occurred at the bank, please try
        // again later."
        let kind = kind_for(Some("10219"), ErrorKind::InvalidRequest);
        assert_eq!(kind, ErrorKind::Provider);
        assert!(kind.is_retryable());
    }

    #[test]
    fn a_declined_card_is_a_decline_and_not_a_bad_request() {
        // 10051 is NOT_SUFFICIENT_FUNDS. Telling a payer their details were
        // invalid sends them round a loop that cannot end.
        let kind = kind_for(Some("10051"), ErrorKind::InvalidRequest);
        assert_eq!(kind, ErrorKind::Declined);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn a_code_this_list_does_not_know_keeps_the_callers_reading() {
        assert_eq!(
            kind_for(Some("99999"), ErrorKind::InvalidRequest),
            ErrorKind::InvalidRequest
        );
        assert_eq!(kind_for(None, ErrorKind::Provider), ErrorKind::Provider);
    }

    #[test]
    fn no_code_is_in_both_lists() {
        for code in RETRYABLE {
            assert!(!DECLINED.contains(code), "{code} is in both lists");
        }
    }
}
