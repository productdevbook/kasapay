//! Nothing that holds a credential may print one.
//!
//! PayPal's `client_id` and `client_secret` are Basic-auth material for one
//! call, `POST /v1/oauth2/token`, and the bearer token that call buys is on
//! every request after — three secrets, and a `{:?}` on the wrong struct
//! hands over any of them.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Money, Raw, Secret};
use kasapay_paypal::{AuthorizationId, CaptureId, Config, PayPal, RefundId, RefundState};

const CLIENT_ID: &str = "AeMUST-NOT-APPEAR-IN-LOGS-id";
const CLIENT_SECRET: &str = "EMUST-NOT-APPEAR-IN-LOGS-secret";

#[test]
fn the_config_and_client_keep_the_credentials() {
    let config = Config::sandbox(Secret::new(CLIENT_ID), Secret::new(CLIENT_SECRET));
    let shown = format!("{config:?}");
    assert!(!shown.contains("MUST-NOT-APPEAR"), "{shown}");

    let paypal = PayPal::new(config).expect("client builds");
    let shown = format!("{paypal:?}");
    assert!(!shown.contains("MUST-NOT-APPEAR"), "{shown}");
}

/// `Refund` and `Capture` both hold a `Raw` — PayPal's own answer, carrying
/// whatever `invoice_id`, `custom_id` or other free text the merchant sent
/// along with money moving. Neither struct's derived `Debug` may print it.
#[test]
fn a_refund_and_a_capture_do_not_print_their_raw_body() {
    let raw = Raw::from_text(
        r#"{"invoice_id":"MUST-NOT-APPEAR-IN-LOGS-invoice","custom_id":"MUST-NOT-APPEAR-IN-LOGS-custom"}"#,
    );

    let refund = kasapay_paypal::Refund {
        id: RefundId::issued("0K35355239430361V"),
        capture: CaptureId::issued("7TK53561YB803214S"),
        amount: Money::from_minor_units(100, kasapay_core::Currency::Usd),
        state: RefundState::Completed,
        raw: raw.clone(),
    };
    let shown = format!("{refund:?}");
    assert!(!shown.contains("MUST-NOT-APPEAR"), "{shown}");

    let capture = kasapay_paypal::Capture {
        id: CaptureId::issued("3C679366HH908993F"),
        authorization: AuthorizationId::issued("0AW2184448108334S"),
        amount: Money::from_minor_units(100, kasapay_core::Currency::Usd),
        status: kasapay_core::Status::Captured,
        raw,
    };
    let shown = format!("{capture:?}");
    assert!(!shown.contains("MUST-NOT-APPEAR"), "{shown}");
}
