//! `IYZWSv2` request signing, for iyzico's classic API.
//!
//! The In-Store API this crate mostly speaks authenticates with plain headers.
//! Everything else iyzico offers — ordinary card payments, subscriptions,
//! marketplace, card storage — signs each request instead, and this is that
//! scheme.
//!
//! A request carries two headers:
//!
//! ```http
//! Authorization: IYZWSv2 <base64EncodedAuthorization>
//! x-iyzi-rnd: <randomKey>
//! ```
//!
//! built in three steps, each documented at
//! <https://docs.iyzico.com/en/getting-started/preliminaries/authentication/hmacsha256-auth>:
//!
//! 1. `signature = HMACSHA256(randomKey + uriPath + body, secretKey)`, hex
//! 2. `authorization = base64("apiKey:" + apiKey + "&randomKey:" + randomKey + "&signature:" + signature)`
//! 3. the header is `IYZWSv2`, one space, then that
//!
//! The body is left out of step 1 when there is none — a GET, or a POST with
//! an empty payload.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use kasapay_core::Secret;
use sha2::Sha256;

/// A merchant's key pair, and the `IYZWSv2` signing that goes with it.
///
/// The In-Store API this crate mostly speaks authenticates with plain headers.
/// Everything else iyzico offers — ordinary card payments, subscriptions,
/// marketplace, card storage — signs each request instead, and this is that
/// scheme: two headers, built in three steps.
///
/// ```http
/// Authorization: IYZWSv2 <base64EncodedAuthorization>
/// x-iyzi-rnd: <randomKey>
/// ```
///
/// 1. `signature = HMACSHA256(randomKey + uriPath + body, secretKey)`, in hex
/// 2. `authorization = base64("apiKey:" + apiKey + "&randomKey:" + randomKey + "&signature:" + signature)`
/// 3. the header is `IYZWSv2`, one space, then that
///
/// The body is left out of step 1 when there is none — a GET, or a POST with
/// an empty payload.
///
/// Documented at
/// <https://docs.iyzico.com/en/getting-started/preliminaries/authentication/hmacsha256-auth>.
#[derive(Debug, Clone)]
pub struct Credentials {
    api_key: Secret,
    secret_key: Secret,
}

impl Credentials {
    /// Holds a merchant's key pair.
    pub fn new(api_key: impl Into<Secret>, secret_key: impl Into<Secret>) -> Self {
        Self {
            api_key: api_key.into(),
            secret_key: secret_key.into(),
        }
    }

    /// The `Authorization` header value for one request.
    ///
    /// `uri_path` is the path alone — `/payment/bin/check`, no host and no
    /// query string. `body` is the request body exactly as it will be sent,
    /// byte for byte: signing a re-serialised copy signs something the server
    /// will not receive.
    #[must_use]
    pub fn authorization(&self, random_key: &str, uri_path: &str, body: &str) -> String {
        let signature = self.signature(random_key, uri_path, body);
        let authorization = format!(
            "apiKey:{}&randomKey:{random_key}&signature:{signature}",
            self.api_key.expose()
        );
        format!("IYZWSv2 {}", BASE64.encode(authorization))
    }

    /// The hex HMAC-SHA256 of `randomKey + uriPath + body` under the secret key.
    #[must_use]
    pub fn signature(&self, random_key: &str, uri_path: &str, body: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret_key.expose().as_bytes())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(random_key.as_bytes());
        mac.update(uri_path.as_bytes());
        mac.update(body.as_bytes());
        hex(&mac.finalize().into_bytes())
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use super::{Credentials, hex};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    /// The header iyzico publishes as a worked example, and its parts.
    const PUBLISHED_HEADER: &str = "IYZWSv2 YXBpS2V5OnNhbmRib3gtbDlNZDFHajNJWWNtdTROZGFXeGFTVW9Db1g3REM1UkEmcmFuZG9tS2V5OjEyMzQ1Njc4OSZzaWduYXR1cmU6MDc5ZGY0YjI0MjZmYzdmNDIwOGQ4ZjIyZmJjMDM0OTc5NDAxOWY4Y2UyYjA3MTFkZTc4MDhiNDg3NGY0ZTc5Ng==";
    const PUBLISHED_API_KEY: &str = "sandbox-l9Md1Gj3IYcmu4NdaWxaSUoCoX7DC5RA";
    const PUBLISHED_RANDOM_KEY: &str = "123456789";
    const PUBLISHED_SIGNATURE: &str =
        "079df4b2426fc7f4208d8f22fbc0349794019f8ce2b0711de7808b4874f4e796";

    #[test]
    fn the_header_matches_the_one_iyzico_publishes() {
        // iyzico does not publish the secret key that produced this signature,
        // so the signature is fed in and the encoding is what is under test —
        // the separators, their order, and the single space after IYZWSv2.
        let encoded = BASE64.encode(format!(
            "apiKey:{PUBLISHED_API_KEY}&randomKey:{PUBLISHED_RANDOM_KEY}&signature:{PUBLISHED_SIGNATURE}"
        ));
        assert_eq!(format!("IYZWSv2 {encoded}"), PUBLISHED_HEADER);
    }

    #[test]
    fn authorization_assembles_the_published_shape() {
        let credentials = Credentials::new(PUBLISHED_API_KEY, "a-secret");
        let header = credentials.authorization(PUBLISHED_RANDOM_KEY, "/payment/bin/check", "{}");

        let encoded = header
            .strip_prefix("IYZWSv2 ")
            .expect("the scheme, then one space");
        let decoded =
            String::from_utf8(BASE64.decode(encoded).expect("valid base64")).expect("valid utf-8");
        let signature = credentials.signature(PUBLISHED_RANDOM_KEY, "/payment/bin/check", "{}");
        assert_eq!(
            decoded,
            format!(
                "apiKey:{PUBLISHED_API_KEY}&randomKey:{PUBLISHED_RANDOM_KEY}&signature:{signature}"
            )
        );
    }

    #[test]
    fn the_signature_covers_random_key_then_path_then_body() {
        let credentials = Credentials::new("k", "s");
        // Concatenation order is the whole scheme: a signature that ignored the
        // path would authenticate a body against the wrong endpoint.
        let signed = credentials.signature("123", "/payment/auth", r#"{"a":1}"#);
        assert_ne!(
            signed,
            credentials.signature("123", "/payment/bin/check", r#"{"a":1}"#)
        );
        assert_ne!(
            signed,
            credentials.signature("124", "/payment/auth", r#"{"a":1}"#)
        );
        assert_ne!(
            signed,
            credentials.signature("123", "/payment/auth", r#"{"a":2}"#)
        );
        // And it is the concatenation, not the parts hashed separately.
        assert_eq!(
            signed,
            credentials.signature("", "123/payment/auth", r#"{"a":1}"#)
        );
    }

    #[test]
    fn an_empty_body_signs_the_random_key_and_path_alone() {
        let credentials = Credentials::new("k", "s");
        assert_eq!(
            credentials.signature("123", "/payment/detail", ""),
            credentials.signature("123/payment/detail", "", "")
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231_test_case_2() {
        // "Jefe" / "what do ya want for nothing?" — proves the primitive is
        // wired up, independently of anything iyzico publishes.
        let credentials = Credentials::new("k", "Jefe");
        assert_eq!(
            credentials.signature("what do ya want ", "for nothing?", ""),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn credentials_do_not_print_their_keys() {
        let credentials = Credentials::new("api-key-value", "secret-key-value");
        let shown = format!("{credentials:?}");
        assert!(!shown.contains("api-key-value"));
        assert!(!shown.contains("secret-key-value"));
    }
}
