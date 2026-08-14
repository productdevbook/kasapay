//! What a Terminal API refusal means.
//!
//! This API does not use the error codes the rest of iyzico does. It has its
//! own list — six digits beginning `380`, plus two token codes in the `100`
//! range — and, unusually, an `errorGroup` beside the code saying which layer
//! refused: the device, the bank, iyzico's payment service, or the request
//! itself. From <https://docs.iyzico.com/en/products/physical-pos/terminal-api-integration/error-codes>.
//!
//! The group is the coarse answer and the code overrides it, because two of
//! the groups hold codes that mean quite different things: a `SYSTEM_ERROR` is
//! usually iyzico's side failing, but `100311` in that group is an expired
//! token and nothing about the system is wrong.
//!
//! A `PAYMENT_ERROR` is the one group that carries the classic API's codes —
//! it is "an error received from iyzico Payment services" — so an unrecognised
//! code falls through to [`crate::errors::kind_for`], which knows a declined
//! card from a bank that asked us to come back.

use kasapay_core::ErrorKind;

/// The token is not one that will work, whatever the group says.
///
/// `380204` is in `DEVICE_ERROR` and reads as one — "error occurred during
/// refresh token and user session has been terminated" — but there is nothing
/// wrong with the device and nothing to retry: the session is gone and the
/// caller has to log in again.
const AUTH: &[&str] = &[
    "100310", // Invalid token!
    "100311", // Access token expired!
    "380102", // You do not have access to the information of this auth device!
    "380204", // Error occurred during refresh token and user session has been terminated
];

/// Nothing by that name exists.
const NOT_FOUND: &[&str] = &[
    "380101", // Auth Device not found! For username: {0}
    "380107", // Payment not found with paymentId: {0}
    "380108", // Payment not found with id: {0}
    "380109", // Payment not found with orderId: {0}
];

/// Nothing is wrong with the request; something was busy or slow.
///
/// [`ErrorKind::Provider`] is retryable, and **that is not a promise the retry
/// is safe**. `380103` is a timeout waiting for the POS device to answer, and
/// a timeout is exactly the case where nobody knows whether the card was
/// charged. Read the payment back with
/// [`Client::payment`](crate::terminal::Client::payment) before sending it
/// again.
const BUSY: &[&str] = &[
    "380103", // Timeout occurred while waiting for device response!
    "380201", // Terminal is currently busy, please wait for the previous operation to complete
];

/// What a refusal means, from its group and its code.
#[must_use]
pub(crate) fn kind_for(group: Option<&str>, code: Option<&str>) -> ErrorKind {
    if let Some(code) = code {
        if AUTH.contains(&code) {
            return ErrorKind::Auth;
        }
        if NOT_FOUND.contains(&code) {
            return ErrorKind::NotFound;
        }
        if BUSY.contains(&code) {
            return ErrorKind::Provider;
        }
    }
    let fallback = match group {
        // The bank understood and said no. iyzico's own words: "if the
        // transaction receives an error on the bank side (other than authCode
        // 00)".
        Some("BANK_ERROR") => ErrorKind::Declined,
        // The request itself, or a state it asked for that cannot exist.
        Some("VALIDATION_ERROR" | "BUSINESS_ERROR") => ErrorKind::InvalidRequest,
        // The device, iyzico's systems, or a wait that ran out. None of these
        // is a verdict on the payment.
        Some("TIMEOUT_ERROR" | "SYSTEM_ERROR" | "DEVICE_ERROR" | "PAYMENT_ERROR") => {
            ErrorKind::Provider
        }
        // A group iyzico has added since this was written, or none at all.
        _ => ErrorKind::Provider,
    };
    crate::errors::kind_for(code, fallback)
}

#[cfg(test)]
mod tests {
    use super::{AUTH, BUSY, NOT_FOUND, kind_for};
    use kasapay_core::ErrorKind;

    #[test]
    fn an_expired_token_is_an_auth_failure_and_not_a_system_one() {
        // 100311 arrives in SYSTEM_ERROR, whose other codes are iyzico's side
        // failing. Reporting this one as Provider would make it retryable, and
        // retrying with the same dead token cannot ever succeed.
        let kind = kind_for(Some("SYSTEM_ERROR"), Some("100311"));
        assert_eq!(kind, ErrorKind::Auth);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn a_terminated_session_is_auth_although_the_device_reported_it() {
        assert_eq!(
            kind_for(Some("DEVICE_ERROR"), Some("380204")),
            ErrorKind::Auth
        );
    }

    #[test]
    fn the_bank_saying_no_is_a_decline() {
        let kind = kind_for(Some("BANK_ERROR"), Some("51"));
        assert_eq!(kind, ErrorKind::Declined);
        assert!(!kind.is_retryable());
    }

    #[test]
    fn a_payment_error_still_reads_the_classic_codes() {
        // 10051 is NOT_SUFFICIENT_FUNDS in iyzico's own error list, which is
        // what a PAYMENT_ERROR forwards.
        assert_eq!(
            kind_for(Some("PAYMENT_ERROR"), Some("10051")),
            ErrorKind::Declined
        );
        // And one that list does not know keeps the group's answer.
        assert_eq!(
            kind_for(Some("PAYMENT_ERROR"), Some("99999")),
            ErrorKind::Provider
        );
    }

    #[test]
    fn a_missing_payment_is_not_found_and_a_bad_request_is_not() {
        assert_eq!(
            kind_for(Some("BUSINESS_ERROR"), Some("380107")),
            ErrorKind::NotFound
        );
        // 380112 is "the amount you want to cancel/refund is not available in
        // your account" — a business rule, not a missing record.
        assert_eq!(
            kind_for(Some("BUSINESS_ERROR"), Some("380112")),
            ErrorKind::InvalidRequest
        );
        assert_eq!(
            kind_for(Some("VALIDATION_ERROR"), Some("380104")),
            ErrorKind::InvalidRequest
        );
    }

    #[test]
    fn a_busy_terminal_is_worth_trying_again() {
        assert!(kind_for(Some("DEVICE_ERROR"), Some("380201")).is_retryable());
        assert!(kind_for(Some("TIMEOUT_ERROR"), Some("380103")).is_retryable());
    }

    #[test]
    fn a_refusal_with_nothing_on_it_is_not_read_as_a_bad_request() {
        // Blaming the caller for a body that says nothing sends them to fix a
        // request that may have been fine.
        assert_eq!(kind_for(None, None), ErrorKind::Provider);
    }

    #[test]
    fn no_code_is_in_two_lists() {
        for code in AUTH {
            assert!(!NOT_FOUND.contains(code) && !BUSY.contains(code), "{code}");
        }
        for code in NOT_FOUND {
            assert!(!BUSY.contains(code), "{code}");
        }
    }
}
