//! The payment notice as a delivery, through the shared [`Webhook`] trait.
//!
//! Every hash here is the one `notice.rs` computed from PayTR's own formula,
//! rather than from the code under test — and percent-encoded, because a
//! notice arrives as a form body and base64 carries `+` and `/`.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Delivery, ErrorKind, EventKind, IdSource, Webhook};
use kasapay_paytr::{Config, Credentials, PayTr};

/// The hash for `ord-1`, `success`, `14990`, percent-encoded for a form body.
const SIGNED: &str = "bGOjbiyL0EfGqNehYd%2F%2BAxWSZyvFgfp4Uc%2BKUiqnfsc%3D";

fn client() -> PayTr {
    let config = Config::at(
        "https://paytr.test",
        Credentials::new("merchant-1", "merchant-key", "merchant-salt"),
    )
    .expect("valid base");
    PayTr::new(config).expect("client builds")
}

fn body(status: &str, total: &str, hash: &str) -> String {
    format!(
        "merchant_oid=ord-1&status={status}&total_amount={total}&hash={hash}\
         &payment_type=card&currency=TL&test_mode=1"
    )
}

#[tokio::test]
async fn a_signed_notice_becomes_an_event_named_by_what_paytr_signed() {
    let body = body("success", "14990", SIGNED);
    let event = client()
        .verify(&Delivery::new(&[], body.as_bytes()))
        .await
        .expect("a notice PayTR signed");

    assert_eq!(event.kind, EventKind::Captured);
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("ord-1")
    );
    // Composed, not issued: PayTR names neither the payment nor the delivery.
    assert!(matches!(event.id.source(), IdSource::Derived(_)));
    assert_eq!(event.id.as_str(), "ord-1:success");
    // The currency is outside the hash, so the figure has no unit to trust.
    assert!(event.amount.is_none());
    assert_eq!(event.raw.text_at("/total_amount").as_deref(), Some("14990"));
}

#[tokio::test]
async fn a_notice_claiming_ten_times_the_amount_is_refused() {
    // The hash is the one PayTR sent for 14990, and the amount is not.
    let body = body("success", "149900", SIGNED);
    let error = client()
        .verify(&Delivery::new(&[], body.as_bytes()))
        .await
        .expect_err("a notice nobody signed");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// A status PayTR does not document is a delivery to acknowledge, not one to
/// answer an error for.
#[tokio::test]
async fn a_status_paytr_does_not_document_is_not_a_failure() {
    // Signed for `pending`, which is not one of PayTR's two.
    let hash = "F8tifHcxi8xcIoxNpafbchYIokBplBy4FRBI1l%2FizlI%3D";
    let body = body("pending", "14990", hash);
    let event = client()
        .verify(&Delivery::new(&[], body.as_bytes()))
        .await
        .expect("an unknown status still verifies");
    assert_eq!(event.kind, EventKind::Other("pending".into()));
}

#[tokio::test]
async fn a_body_that_is_not_the_form_paytr_posts_is_malformed() {
    let error = client()
        .verify(&Delivery::new(&[], br#"{"merchant_oid":"ord-1"}"#))
        .await
        .expect_err("JSON is not what PayTR posts");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}
