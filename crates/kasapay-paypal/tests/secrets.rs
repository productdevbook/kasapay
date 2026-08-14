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

use kasapay_core::Secret;
use kasapay_paypal::{Config, PayPal};

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
