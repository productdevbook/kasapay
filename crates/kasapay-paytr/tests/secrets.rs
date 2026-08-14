//! Nothing that holds a key may print one.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_paytr::{Config, Credentials, PayTr};

const KEY: &str = "merchant-key-MUSTNOTAPPEAR-1";
const SALT: &str = "merchant-salt-MUSTNOTAPPEAR-2";

fn leaks(shown: &str) -> bool {
    shown.contains("MUSTNOTAPPEAR")
}

#[test]
fn the_config_and_client_keep_their_key_and_salt() {
    let credentials = Credentials::new("merchant-1", KEY, SALT);
    assert!(!leaks(&format!("{credentials:?}")), "credentials leaked");

    let config = Config::new(credentials);
    assert!(!leaks(&format!("{config:?}")), "config leaked");

    let client = PayTr::new(config).expect("client builds");
    assert!(!leaks(&format!("{client:?}")), "client leaked");
}

#[test]
fn the_merchant_id_is_not_a_secret_and_still_reads_back() {
    let credentials = Credentials::new("merchant-1", KEY, SALT);
    // It travels in the clear on every request, so hiding it would be theatre.
    assert_eq!(credentials.merchant_id(), "merchant-1");
    assert!(format!("{credentials:?}").contains("merchant-1"));
}
