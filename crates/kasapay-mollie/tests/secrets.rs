//! Nothing that holds a key may print one.
//!
//! Mollie's key is a bearer token on every request, and it is the whole of the
//! authentication: one `{:?}` on a config in a log line hands somebody the
//! account.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::Secret;
use kasapay_mollie::{Config, Mollie};

const KEY: &str = "test_kasapayMUSTNOTAPPEARinlogs";

#[test]
fn the_config_and_client_keep_the_api_key() {
    let config = Config::new(Secret::new(KEY));
    assert!(
        !format!("{config:?}").contains("MUSTNOTAPPEAR"),
        "the API key reached a Debug"
    );

    let mollie = Mollie::new(config).expect("client builds");
    assert!(
        !format!("{mollie:?}").contains("MUSTNOTAPPEAR"),
        "the API key reached a Debug"
    );
}
