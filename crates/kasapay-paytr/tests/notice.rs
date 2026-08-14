//! The payment notice: the only place PayTR reports a refusal.
//!
//! Every hash here was computed from PayTR's own formula — HMAC-SHA256 over
//! `merchant_oid + salt + status + total_amount`, keyed with the merchant key —
//! rather than from the code under test.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, IdSource, Status};
use kasapay_paytr::{Credentials, Notice};

fn credentials() -> Credentials {
    Credentials::new("merchant-1", "merchant-key", "merchant-salt")
}

/// A notice for `ord-1` at 149.90 lira, which PayTR posts as `14990`.
fn notice(status: &str, hash: &str) -> Notice {
    Notice {
        merchant_oid: "ord-1".into(),
        status: status.into(),
        total_amount: "14990".into(),
        hash: hash.into(),
        failed_reason_code: None,
        failed_reason_msg: None,
        payment_amount: None,
        currency: Some("TL".into()),
        payment_type: Some("card".into()),
        test_mode: Some("1".into()),
    }
}

#[test]
fn a_payment_notice_verifies_against_paytrs_hash() {
    // The salt sits between the order reference and the outcome here, unlike
    // every other call, which is the mistake this test exists to catch.
    let hash = "bGOjbiyL0EfGqNehYd/+AxWSZyvFgfp4Uc+KUiqnfsc=";
    assert!(credentials().verify_callback(hash, "ord-1", "success", "14990"));

    // A notice claiming ten times the amount, with the hash left alone.
    assert!(!credentials().verify_callback(hash, "ord-1", "success", "149900"));
    assert!(!credentials().verify_callback(hash, "ord-2", "success", "14990"));
    assert!(!credentials().verify_callback(hash, "ord-1", "failed", "14990"));
}

#[test]
fn a_paid_notice_is_a_captured_charge_named_by_the_order_reference() {
    let charge = notice("success", "bGOjbiyL0EfGqNehYd/+AxWSZyvFgfp4Uc+KUiqnfsc=")
        .charge(&credentials(), Currency::Try)
        .expect("a notice PayTR signed");

    assert_eq!(charge.status, Status::Captured);
    // The notice carries minor units where a refund takes "149.90".
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert!(charge.order_amount.is_none());
    let id = charge.id.as_ref().expect("a notice names its payment");
    assert_eq!(id.as_str(), "ord-1");
    assert!(matches!(id.source(), IdSource::Derived(_)));
}

/// The whole of #74: a refusal is a commercial outcome with a status, not an
/// error kind a caller has to read a verdict out of.
#[test]
fn a_refused_notice_is_a_failed_charge_carrying_what_was_attempted() {
    let refused = Notice {
        failed_reason_code: Some("0".into()),
        failed_reason_msg: Some("Kartin limiti yetersiz".into()),
        ..notice("failed", "zOqgVKwSDLdfdAnVubkNJHYEWoG+mIdveLQjgYSya3E=")
    };

    let charge = refused
        .charge(&credentials(), Currency::Try)
        .expect("a notice PayTR signed");

    assert_eq!(charge.status, Status::Failed);
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert_eq!(
        charge.raw.text_at("/failed_reason_msg").as_deref(),
        Some("Kartin limiti yetersiz")
    );
}

/// PayTR sends the surcharge and the order's own price as two fields, both
/// multiplied by a hundred.
#[test]
fn an_instalment_surcharge_reports_both_amounts() {
    let charged = Notice {
        total_amount: "16489".into(),
        payment_amount: Some("14990".into()),
        ..notice("success", "aD+ZIzxqzmpU1kYARUpnm5qOxzk/xW92Prf39O1tfbQ=")
    };

    let charge = charged
        .charge(&credentials(), Currency::Try)
        .expect("a notice PayTR signed");

    assert_eq!(charge.amount.minor_units(), 16_489);
    assert_eq!(
        charge
            .order_amount
            .expect("the order's own price")
            .minor_units(),
        14_990
    );
}

#[test]
fn a_notice_nobody_signed_is_untrusted() {
    let error = notice("success", "not the hash PayTR would have sent")
        .charge(&credentials(), Currency::Try)
        .expect_err("an unsigned notice must never be acted on");

    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// A status PayTR does not document is not quietly read as either outcome,
/// even when the hash over it is PayTR's own.
#[test]
fn a_status_paytr_does_not_document_is_malformed() {
    let error = notice("odendi", "hUP8BnP50CZUT/Y0I1cIIMvAXwGyel4wQ6nSduTL9Uw=")
        .charge(&credentials(), Currency::Try)
        .expect_err("neither success nor failed");

    assert_eq!(error.kind(), ErrorKind::Malformed);
}
