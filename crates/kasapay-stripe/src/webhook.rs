//! Checking that a delivery really came from Stripe.
//!
//! Stripe signs the body with HMAC-SHA256 and puts the result in a
//! `Stripe-Signature` header:
//!
//! ```http
//! Stripe-Signature: t=1492774577,v1=5257a869e7…,v0=6ffbb59b2300…
//! ```
//!
//! The signed text is `{t}.{body}` — the timestamp, a full stop, then the body
//! byte for byte — keyed with the endpoint's signing secret, which is the
//! `whsec_…` value from the dashboard and **not** the API key.
//!
//! Two things beyond the hash are load-bearing:
//!
//! - **The timestamp is checked.** A signature stays valid forever, so a body
//!   captured off the wire could be replayed a week later and still verify.
//!   [`DEFAULT_TOLERANCE`] is how far out of date a delivery may be.
//! - **`v1` is the scheme, and there may be several.** `v0` is a test-mode
//!   scheme that is not this one, and Stripe can add more, so the header is
//!   read as a list and every `v1` in it is tried.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use kasapay_core::{
    Currency, Error, ErrorKind, Event, EventId, EventKind, Money, PaymentId, Raw, Secret, header,
};
use sha2::Sha256;
use subtle::ConstantTimeEq as _;

use crate::convert::PROVIDER;

/// How far out of date a delivery may be and still be believed.
///
/// Stripe's own libraries use five minutes and their documentation recommends
/// it. Longer widens the window in which a captured body can be replayed;
/// shorter starts refusing genuine deliveries whose clock has drifted.
pub const DEFAULT_TOLERANCE: Duration = Duration::from_secs(300);

/// The header Stripe signs with.
const SIGNATURE_HEADER: &str = "Stripe-Signature";

/// Verifies one delivery and reads it as an [`Event`].
pub(crate) fn verify(
    secret: Option<&Secret>,
    tolerance: Duration,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Event, Error> {
    let secret = secret.ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "no webhook signing secret is configured; \
             build the client with Stripe::with_webhook_secret",
        )
    })?;
    let header_value = header(headers, SIGNATURE_HEADER).ok_or_else(|| {
        Error::new(
            ErrorKind::Untrusted,
            PROVIDER,
            "the delivery carried no Stripe-Signature header, so it has not been \
             shown to have come from Stripe",
        )
    })?;

    let parsed = Signature::parse(header_value)?;
    parsed.check_age(tolerance)?;
    parsed.check_hash(secret.expose().as_bytes(), body)?;

    read(body)
}

/// The parts of a `Stripe-Signature` header.
struct Signature<'a> {
    timestamp: i64,
    /// Every `v1` in the header. Stripe documents more than one during a
    /// secret rollover, and refusing on the first mismatch would refuse a
    /// genuine delivery mid-rotation.
    candidates: Vec<&'a str>,
}

impl<'a> Signature<'a> {
    fn parse(value: &'a str) -> Result<Self, Error> {
        let mut timestamp = None;
        let mut candidates = Vec::new();
        for part in value.split(',') {
            match part.trim().split_once('=') {
                Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
                Some(("v1", value)) => candidates.push(value),
                _ => {}
            }
        }
        let timestamp = timestamp.ok_or_else(|| {
            Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the Stripe-Signature header carried no timestamp",
            )
        })?;
        if candidates.is_empty() {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the Stripe-Signature header carried no v1 signature; \
                 v0 is a different scheme and is not accepted for this",
            ));
        }
        Ok(Self {
            timestamp,
            candidates,
        })
    }

    /// Refuses a delivery older — or newer — than the tolerance allows.
    ///
    /// Both directions, because a clock ahead of ours is as much a sign of a
    /// replayed or forged timestamp as one behind it.
    fn check_age(&self, tolerance: Duration) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            });
        let drift = now.saturating_sub(self.timestamp).abs();
        if drift > i64::try_from(tolerance.as_secs()).unwrap_or(i64::MAX) {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                format!(
                    "the delivery is signed for {drift} seconds away from now, \
                     outside the tolerance; a signature never expires on its own, \
                     so a replayed body would otherwise verify"
                ),
            ));
        }
        Ok(())
    }

    /// Compares in constant time: an early return tells an attacker how much
    /// of a guess was right.
    fn check_hash(&self, secret: &[u8], body: &[u8]) -> Result<(), Error> {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret)
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(self.timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        let expected = hex(&mac.finalize().into_bytes());

        let matched = self.candidates.iter().fold(0u8, |seen, candidate| {
            seen | expected.as_bytes().ct_eq(candidate.as_bytes()).unwrap_u8()
        });
        if matched == 1 {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the delivery's signature does not match what Stripe should have sent; \
                 the body may not be theirs and must not be acted on",
            ))
        }
    }
}

/// Reads a verified body. Never called before the signature has matched.
fn read(body: &[u8]) -> Result<Event, Error> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "the delivery verified but was not JSON",
        )
        .with_source(e)
    })?;

    let id = value
        .pointer("/id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "the delivery carried no event id, which is what a caller keys on",
            )
        })?;
    let kind = value
        .pointer("/type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let object = value.pointer("/data/object");

    let payment = object
        .and_then(|o| o.pointer("/payment_intent").or_else(|| o.pointer("/id")))
        .and_then(serde_json::Value::as_str)
        .map(PaymentId::new);
    let amount = object.and_then(read_amount);

    Ok(Event {
        id: EventId::Provider(id.into()),
        kind: event_kind(kind),
        payment,
        amount,
        provider: PROVIDER,
        raw: Raw::from_text(String::from_utf8_lossy(body).into_owned()),
    })
}

/// The amount an event's object is about, where kasapay has a currency for it.
///
/// Stripe settles in 135 currencies and [`Currency`] names six, so an event in
/// one of the other 129 loses its amount rather than being refused. Refusing
/// would be worse: the delivery is genuine and the caller still has to answer
/// it.
fn read_amount(object: &serde_json::Value) -> Option<Money> {
    let currency: Currency = object.pointer("/currency")?.as_str()?.parse().ok()?;
    // A partial capture leaves `amount` at what was authorised, exactly as on
    // the PaymentIntent this crate reads elsewhere.
    let minor = object
        .pointer("/amount_received")
        .and_then(serde_json::Value::as_i64)
        .filter(|received| *received > 0)
        .or_else(|| {
            object
                .pointer("/amount")
                .and_then(serde_json::Value::as_i64)
        })?;
    Some(Money::from_minor_units(minor, currency))
}

/// Stripe's event types, in kasapay's terms.
///
/// Anything not named here is [`EventKind::Other`] carrying Stripe's own word
/// for it. Stripe ships new event types continually, and answering an error
/// for one would put a working endpoint into a retry loop that lasts days.
fn event_kind(kind: &str) -> EventKind {
    match kind {
        "payment_intent.succeeded" | "charge.succeeded" | "charge.captured" => EventKind::Captured,
        "payment_intent.amount_capturable_updated" => EventKind::Authorized,
        "payment_intent.payment_failed" | "charge.failed" => EventKind::Failed,
        "payment_intent.canceled" | "charge.expired" => EventKind::Canceled,
        "charge.refunded" | "refund.created" | "refund.updated" | "charge.refund.updated" => {
            EventKind::Refunded
        }
        other => EventKind::Other(other.into()),
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
    use super::{DEFAULT_TOLERANCE, Signature, event_kind, hex, verify};
    use hmac::{Hmac, KeyInit, Mac};
    use kasapay_core::{ErrorKind, EventId, EventKind, Secret};
    use sha2::Sha256;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const SECRET: &str = "whsec_kasapay_test";

    fn now() -> i64 {
        i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("after the epoch")
                .as_secs(),
        )
        .expect("fits")
    }

    fn signed_header(timestamp: i64, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(SECRET.as_bytes()).expect("any key length");
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        format!("t={timestamp},v1={}", hex(&mac.finalize().into_bytes()))
    }

    fn headers(value: &str) -> Vec<(String, String)> {
        vec![("stripe-signature".to_owned(), value.to_owned())]
    }

    fn body() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "id": "evt_kasapay1",
            "type": "payment_intent.succeeded",
            "data": {"object": {"id": "pi_kasapay1", "amount": 1999, "currency": "usd"}},
        }))
        .expect("valid json")
    }

    fn secret() -> Secret {
        Secret::new(SECRET)
    }

    #[test]
    fn a_signed_delivery_reads_as_an_event() {
        let body = body();
        let event = verify(
            Some(&secret()),
            DEFAULT_TOLERANCE,
            &headers(&signed_header(now(), &body)),
            &body,
        )
        .expect("signed by us");
        assert_eq!(event.id, EventId::Provider("evt_kasapay1".into()));
        assert!(!event.id.is_derived(), "Stripe issues its own event id");
        assert_eq!(event.kind, EventKind::Captured);
        assert_eq!(event.payment.expect("named").as_str(), "pi_kasapay1");
        assert_eq!(event.amount.expect("carried").minor_units(), 1999);
    }

    #[test]
    fn a_tampered_body_is_refused() {
        let body = body();
        let header = signed_header(now(), &body);
        // One kuruş more, same signature.
        let tampered = String::from_utf8(body)
            .expect("utf-8")
            .replace("1999", "9999");
        let error = verify(
            Some(&secret()),
            DEFAULT_TOLERANCE,
            &headers(&header),
            tampered.as_bytes(),
        )
        .expect_err("a tampered body must not be believed");
        assert_eq!(error.kind(), ErrorKind::Untrusted);
    }

    #[test]
    fn a_delivery_with_no_signature_header_is_refused_rather_than_read() {
        let body = body();
        let error = verify(Some(&secret()), DEFAULT_TOLERANCE, &[], &body)
            .expect_err("an unsigned body is never an Event");
        assert_eq!(error.kind(), ErrorKind::Untrusted);
    }

    #[test]
    fn a_signature_under_the_wrong_secret_is_refused() {
        let body = body();
        let header = signed_header(now(), &body);
        let error = verify(
            Some(&Secret::new("whsec_someone_else")),
            DEFAULT_TOLERANCE,
            &headers(&header),
            &body,
        )
        .expect_err("not signed with our secret");
        assert_eq!(error.kind(), ErrorKind::Untrusted);
    }

    #[test]
    fn a_genuine_body_replayed_a_day_later_is_refused() {
        let body = body();
        // Signed by us, valid hash, and a day old. Nothing but the timestamp
        // check stops this one.
        let header = signed_header(now() - 86_400, &body);
        let error = verify(Some(&secret()), DEFAULT_TOLERANCE, &headers(&header), &body)
            .expect_err("outside the tolerance");
        assert_eq!(error.kind(), ErrorKind::Untrusted);
        assert!(error.to_string().contains("replayed"));
    }

    #[test]
    fn a_v0_signature_alone_is_not_accepted() {
        let error = Signature::parse("t=1492774577,v0=6ffbb59b2300").expect_err("v0 is not v1");
        assert_eq!(error.kind(), ErrorKind::Untrusted);
    }

    #[test]
    fn any_v1_in_the_header_verifies_it() {
        // Stripe sends two during a secret rollover; refusing on the first
        // mismatch would refuse a genuine delivery mid-rotation.
        let body = body();
        let genuine = signed_header(now(), &body);
        let v1 = genuine.split_once("v1=").expect("a v1 part").1;
        let header = format!(
            "{},v1=0000000000000000000000000000000000000000000000000000000000000000",
            genuine.split(",v1=").next().expect("the timestamp"),
        );
        let both = format!("{header},v1={v1}");
        verify(Some(&secret()), DEFAULT_TOLERANCE, &headers(&both), &body)
            .expect("one of them is ours");
    }

    #[test]
    fn an_event_type_stripe_added_later_is_other_rather_than_an_error() {
        // The whole point: an Err here would put a working endpoint into a
        // redelivery loop lasting days.
        assert_eq!(
            event_kind("payment_intent.partially_funded"),
            EventKind::Other("payment_intent.partially_funded".into())
        );
        assert_eq!(event_kind("payment_intent.succeeded"), EventKind::Captured);
        assert_eq!(event_kind("charge.refunded"), EventKind::Refunded);
        assert_eq!(
            event_kind("payment_intent.amount_capturable_updated"),
            EventKind::Authorized
        );
    }

    #[test]
    fn an_event_in_a_currency_kasapay_does_not_name_keeps_everything_else() {
        let body = serde_json::to_vec(&serde_json::json!({
            "id": "evt_kasapay2",
            "type": "payment_intent.succeeded",
            "data": {"object": {"id": "pi_2", "amount": 500, "currency": "sek"}},
        }))
        .expect("valid json");
        let event = verify(
            Some(&secret()),
            DEFAULT_TOLERANCE,
            &headers(&signed_header(now(), &body)),
            &body,
        )
        .expect("signed by us");
        // Refusing a genuine delivery over a currency would be worse: the
        // caller still has to answer it.
        assert!(event.amount.is_none());
        assert_eq!(event.payment.expect("named").as_str(), "pi_2");
    }

    #[test]
    fn a_client_with_no_signing_secret_says_so_rather_than_verifying_nothing() {
        let body = body();
        let error = verify(
            None,
            DEFAULT_TOLERANCE,
            &headers(&signed_header(now(), &body)),
            &body,
        )
        .expect_err("nothing to verify against");
        assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    }

    #[test]
    fn a_tolerance_of_zero_still_accepts_a_delivery_signed_now() {
        let body = body();
        verify(
            Some(&secret()),
            Duration::ZERO,
            &headers(&signed_header(now(), &body)),
            &body,
        )
        .expect("signed this second");
    }
}
