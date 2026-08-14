//! PayTR's request tokens and callback hash.
//!
//! Every call carries a `paytr_token`: the base64 of an HMAC-SHA256 over the
//! request's own fields, keyed with the merchant key. The merchant salt goes
//! into the hashed text rather than the key.
//!
//! **Where the salt sits differs per operation**, and that is the detail worth
//! encoding once rather than at four call sites:
//!
//! | | hashed text |
//! |---|---|
//! | opening a payment | the request fields, then the salt |
//! | reading one back | `merchant_id + merchant_oid` then the salt |
//! | a refund | `merchant_id + merchant_oid + amount` then the salt |
//! | a BIN query | `bin_number + merchant_id` then the salt — the BIN first |
//! | **a callback** | `merchant_oid` **then the salt** then `status + total_amount` |
//!
//! The callback is the odd one out, and it is the one where getting it wrong
//! means believing a forged payment notice.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use kasapay_core::Secret;
use sha2::Sha256;
use subtle::ConstantTimeEq as _;

/// What PayTR knows a merchant by.
#[derive(Debug, Clone)]
pub struct Credentials {
    merchant_id: Box<str>,
    key: Secret,
    salt: Secret,
}

impl Credentials {
    /// Holds a merchant's id, key and salt.
    pub fn new(
        merchant_id: impl Into<Box<str>>,
        key: impl Into<Secret>,
        salt: impl Into<Secret>,
    ) -> Self {
        Self {
            merchant_id: merchant_id.into(),
            key: key.into(),
            salt: salt.into(),
        }
    }

    /// The merchant id, which travels in the clear on every request.
    #[must_use]
    pub fn merchant_id(&self) -> &str {
        &self.merchant_id
    }

    /// The token for a request whose fields are `parts`, with the salt appended.
    #[must_use]
    pub fn token(&self, parts: &[&str]) -> String {
        self.hash(&(parts.concat() + self.salt.expose()))
    }

    /// The hash PayTR puts on a payment notice.
    ///
    /// The salt sits **between** the order reference and the outcome here,
    /// which is why this cannot go through [`Credentials::token`].
    #[must_use]
    pub fn callback_hash(&self, merchant_oid: &str, status: &str, total_amount: &str) -> String {
        self.hash(&format!(
            "{merchant_oid}{}{status}{total_amount}",
            self.salt.expose()
        ))
    }

    /// Whether a payment notice really came from PayTR.
    ///
    /// Compared in constant time. A notice that does not verify must be
    /// answered `OK` and otherwise ignored: PayTR retries anything else, and
    /// acting on it is how a shop ships against a payment nobody made.
    #[must_use]
    pub fn verify_callback(
        &self,
        hash: &str,
        merchant_oid: &str,
        status: &str,
        total_amount: &str,
    ) -> bool {
        let expected = self.callback_hash(merchant_oid, status, total_amount);
        expected.as_bytes().ct_eq(hash.as_bytes()).unwrap_u8() == 1
    }

    fn hash(&self, text: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.expose().as_bytes())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(text.as_bytes());
        BASE64.encode(mac.finalize().into_bytes())
    }
}
