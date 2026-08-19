//! Stripe's signature, and what a verified delivery says.
//!
//! **Every signature here was computed from Stripe's own formula** — the
//! HMAC-SHA256 of `timestamp.body` under the endpoint secret, hex — rather
//! than from the code under test. A test that asks the code what it thinks the
//! signature is proves only that it is consistent with itself.
//!
//! The timestamp is a fixed one from 2017, which is why the tolerance is
//! opened where the subject is the signature and left alone where the subject
//! is the age.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use std::time::Duration;

use kasapay_core::{Currency, Delivery, ErrorKind, EventKind, IdSource, Secret, Webhook};
use kasapay_stripe::Webhooks;

/// The timestamp every fixture here is signed at.
const SIGNED_AT: &str = "1492774577";

/// Longer than this library will be running: the age check is a different
/// test's subject.
fn ignoring_age() -> Webhooks {
    Webhooks::new(Secret::new("whsec_kasapay")).tolerance(Duration::from_secs(u64::from(u32::MAX)))
}

fn captured() -> (&'static str, String) {
    (
        r#"{"id":"evt_1","type":"payment_intent.succeeded","data":{"object":{"id":"pi_kasapay1","amount":1999,"currency":"usd"}}}"#,
        signature("6e29706e954663d03b3010b116b42659ca469e3fbcde0e1c43bddbfb68d21ecf"),
    )
}

fn signature(v1: &str) -> String {
    format!("t={SIGNED_AT},v1={v1}")
}

#[tokio::test]
async fn a_signed_delivery_is_the_payment_it_names() {
    let (body, header) = captured();
    let headers = [("Stripe-Signature", header.as_str())];
    let event = ignoring_age()
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect("Stripe signed this");

    assert_eq!(event.kind, EventKind::Captured);
    assert_eq!(event.id.as_str(), "evt_1");
    // Stripe names the delivery itself, so the caller's unique index is a
    // guarantee rather than a heuristic.
    assert_eq!(event.id.source(), IdSource::Provider);
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("pi_kasapay1")
    );
    let amount = event.amount.expect("a PaymentIntent carries its amount");
    assert_eq!(amount.minor_units(), 1999);
    assert_eq!(amount.currency(), Currency::Usd);
}

#[tokio::test]
async fn a_body_with_one_byte_changed_is_refused() {
    let (body, header) = captured();
    let tampered = body.replace("1999", "9999");
    let headers = [("Stripe-Signature", header.as_str())];
    let error = ignoring_age()
        .verify(&Delivery::new(&headers, tampered.as_bytes()))
        .await
        .expect_err("the signature is over the bytes");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// A valid signature beside a second one is still refused, and it is the
/// *valid* one that arrives first: a verifier that reads the first header and
/// stops would accept this delivery while whatever wrote the second header
/// believes it signed something else. Two claims about one delivery are not
/// one claim.
#[tokio::test]
async fn a_delivery_carrying_two_signature_headers_is_refused() {
    let (body, header) = captured();
    let headers = [
        ("Stripe-Signature", header.as_str()),
        ("stripe-signature", "t=1492774577,v1=deadbeef"),
    ];
    let error = ignoring_age()
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect_err("two signatures over one body is not a signature");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
    assert!(error.to_string().contains("arrived 2 times"), "{error}");
}

#[tokio::test]
async fn a_delivery_with_no_signature_at_all_is_refused() {
    let (body, _) = captured();
    let error = ignoring_age()
        .verify(&Delivery::new(&[], body.as_bytes()))
        .await
        .expect_err("an unsigned body never becomes an event");
    assert_eq!(error.kind(), ErrorKind::Untrusted);

    let headers = [("Stripe-Signature", "t=1492774577")];
    let error = ignoring_age()
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect_err("a header with no v1 signature is no signature");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// The signature matches and the delivery is nine years old: that is what a
/// replay looks like.
#[tokio::test]
async fn a_correctly_signed_replay_is_still_refused() {
    let (body, header) = captured();
    let headers = [("Stripe-Signature", header.as_str())];
    let error = Webhooks::new(Secret::new("whsec_kasapay"))
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect_err("outside the tolerance");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

#[tokio::test]
async fn a_secret_from_another_endpoint_does_not_verify() {
    let (body, header) = captured();
    let headers = [("Stripe-Signature", header.as_str())];
    let error = Webhooks::new(Secret::new("whsec_another"))
        .tolerance(Duration::from_secs(u64::from(u32::MAX)))
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect_err("one address's secret is not another's");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

#[tokio::test]
async fn a_refunded_charge_answers_what_went_back_rather_than_what_was_taken() {
    let body = r#"{"id":"evt_2","type":"charge.refunded","data":{"object":{"id":"ch_1","payment_intent":"pi_kasapay1","amount":1999,"amount_refunded":500,"currency":"usd"}}}"#;
    let header = signature("53046599f61bcd25f80860a487e208e06849ab67d596c101a1ad5ced86e6be3f");
    let headers = [("Stripe-Signature", header.as_str())];
    let event = ignoring_age()
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect("Stripe signed this");

    assert_eq!(event.kind, EventKind::Refunded);
    // The charge names the intent; kasapay's payment id is the intent's.
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("pi_kasapay1")
    );
    assert_eq!(
        event.amount.map(kasapay_core::Money::minor_units),
        Some(500)
    );
}

/// An event type this crate has never heard of is an ordinary delivery.
///
/// Answering an error for one is how a handler earns days of redeliveries for
/// something nobody wanted.
#[tokio::test]
async fn an_event_type_nobody_models_is_not_an_error() {
    let body = r#"{"id":"evt_3","type":"issuing_card.created","data":{"object":{"id":"ic_1"}}}"#;
    let header = signature("ac4125611f5398f60f6be1050db4e6ba24fdfbf08fd699af602b41d10febcee8");
    let headers = [("Stripe-Signature", header.as_str())];
    let event = ignoring_age()
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect("an unknown type still verifies");

    assert_eq!(event.kind, EventKind::Other("issuing_card.created".into()));
    assert!(event.payment.is_none());
    assert!(event.amount.is_none());
}
