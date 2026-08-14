//! Checking that a response really came from iyzico.
//!
//! Every money-moving endpoint answers with a `signature`: the HMAC-SHA256 of
//! a handful of the response's own fields, joined with colons, under the
//! merchant's secret key. Checking it is what tells a forged callback from a
//! real one, and a caller who skips it will take a payment that never happened.
//!
//! Two things about it are easy to get wrong, and both are checked here rather
//! than left to the caller:
//!
//! - **Which fields, and in what order.** It differs per endpoint. A checkout
//!   form's start signs `conversationId:token`; its result signs eight fields.
//! - **Trailing zeros.** An amount is signed without them, so `10.50` signs as
//!   `10.5` and `50.00` as `50`. iyzico says outright that this is the
//!   merchant's job and they do not provide it.

use subtle::ConstantTimeEq as _;

use crate::signing::Credentials;

/// An amount as it goes into a signature, with trailing zeros removed.
///
/// iyzico signs `10.50` as `10.5`, `10.0` as `10` and `10.51050` as `10.5105`.
/// A whole number keeps no point at all.
#[must_use]
pub fn signed_amount(price: &str) -> &str {
    let Some((whole, fraction)) = price.split_once('.') else {
        return price;
    };
    let trimmed = fraction.trim_end_matches('0');
    if trimmed.is_empty() {
        whole
    } else {
        // Safe because `trimmed` is a prefix of `fraction`, which sits directly
        // after the point in `price`.
        &price[..=whole.len() + trimmed.len()]
    }
}

/// Which fields an endpoint signs, in the order it signs them.
///
/// From <https://docs.iyzico.com/en/advanced/response-signature-validation>.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Signed {
    /// `/payment/iyzipos/checkoutform/initialize/auth/ecom` — the form opening.
    CheckoutFormStart,
    /// `/payment/iyzipos/checkoutform/auth/ecom/detail` — the form's result.
    CheckoutFormResult,
    /// The redirect iyzico makes to a 3DS `callbackUrl`.
    Callback,
    /// `/payment/auth` and the other non-3DS payment endpoints.
    Payment,
    /// `/payment/refund`.
    Refund,
}

impl Signed {
    /// The field names this endpoint signs, in order.
    ///
    /// Only useful for reading; [`verify`] takes the values themselves.
    #[must_use]
    pub const fn fields(self) -> &'static [&'static str] {
        match self {
            Self::CheckoutFormStart => &["conversationId", "token"],
            Self::CheckoutFormResult => &[
                "paymentStatus",
                "paymentId",
                "currency",
                "basketId",
                "conversationId",
                "paidPrice",
                "price",
                "token",
            ],
            Self::Callback => &[
                "conversationData",
                "conversationId",
                "mdStatus",
                "paymentId",
                "status",
            ],
            Self::Payment => &[
                "paymentId",
                "currency",
                "basketId",
                "conversationId",
                "paidPrice",
                "price",
            ],
            Self::Refund => &["paymentId", "price", "currency", "conversationId"],
        }
    }
}

impl Credentials {
    /// The signature iyzico should have sent for these values.
    ///
    /// The values must be in the order [`Signed::fields`] gives, and any
    /// amount among them must already have gone through [`signed_amount`].
    #[must_use]
    pub fn response_signature(&self, values: &[&str]) -> String {
        self.signature("", "", &values.join(":"))
    }

    /// Whether a response's signature is the one it should have carried.
    ///
    /// Compared in constant time: a comparison that returns early tells an
    /// attacker how much of a guess was right.
    #[must_use]
    pub fn verify_response(&self, signature: &str, values: &[&str]) -> bool {
        let expected = self.response_signature(values);
        expected.as_bytes().ct_eq(signature.as_bytes()).unwrap_u8() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::{Signed, signed_amount};
    use crate::signing::Credentials;

    /// The worked example from iyzico's own documentation, sandbox key and all.
    const DOC_SECRET: &str = "sandbox-qaIiLIxhjMgx3LSKIVvp6j17NunHOFtD";
    const DOC_SIGNATURE: &str = "836c3a6c8db86c81043f2ca74edb13518b54a813f454f8dd762f0dd658610173";

    fn documented() -> Credentials {
        Credentials::new("api-key", DOC_SECRET)
    }

    #[test]
    fn the_documented_payment_signature_is_reproduced() {
        // 22416032:TRY:basketId:conversationId:10.5:10.5
        let values = [
            "22416032",
            "TRY",
            "basketId",
            "conversationId",
            "10.5",
            "10.5",
        ];
        assert_eq!(documented().response_signature(&values), DOC_SIGNATURE);
        assert!(documented().verify_response(DOC_SIGNATURE, &values));
    }

    #[test]
    fn a_tampered_field_fails_verification() {
        let mut values = vec![
            "22416032",
            "TRY",
            "basketId",
            "conversationId",
            "10.5",
            "10.5",
        ];
        assert!(documented().verify_response(DOC_SIGNATURE, &values));
        // One kuruş more, same signature.
        values[4] = "10.51";
        assert!(!documented().verify_response(DOC_SIGNATURE, &values));
    }

    #[test]
    fn reordering_the_fields_fails_verification() {
        let values = [
            "TRY",
            "22416032",
            "basketId",
            "conversationId",
            "10.5",
            "10.5",
        ];
        assert!(!documented().verify_response(DOC_SIGNATURE, &values));
    }

    #[test]
    fn trailing_zeros_go_exactly_as_iyzico_documents() {
        // Every one of these is a worked example from their page.
        assert_eq!(signed_amount("10"), "10");
        assert_eq!(signed_amount("10.0"), "10");
        assert_eq!(signed_amount("10.5"), "10.5");
        assert_eq!(signed_amount("10.50"), "10.5");
        assert_eq!(signed_amount("10.510"), "10.51");
        assert_eq!(signed_amount("10.5105"), "10.5105");
        assert_eq!(signed_amount("10.51050"), "10.5105");
        assert_eq!(signed_amount("50.00"), "50");
    }

    #[test]
    fn an_amount_from_money_is_normalised_before_signing() {
        use kasapay_core::{Currency, Money};
        // Money renders two places always; iyzico signs without the trailing
        // zero, so the two would never agree unsigned.
        let money = Money::parse("149.90", Currency::Try).expect("valid amount");
        assert_eq!(money.to_decimal_string(), "149.90");
        assert_eq!(signed_amount(&money.to_decimal_string()), "149.9");
        let round = Money::parse("50.00", Currency::Try).expect("valid amount");
        assert_eq!(signed_amount(&round.to_decimal_string()), "50");
    }

    #[test]
    fn each_endpoint_signs_the_fields_iyzico_lists() {
        assert_eq!(
            Signed::CheckoutFormStart.fields(),
            ["conversationId", "token"]
        );
        assert_eq!(Signed::CheckoutFormResult.fields().len(), 8);
        assert_eq!(Signed::Payment.fields()[0], "paymentId");
        assert_eq!(Signed::Refund.fields().last(), Some(&"conversationId"));
    }
}
