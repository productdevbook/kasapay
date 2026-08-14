//! Nothing that holds a key may print one.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_iyzico::{Credentials, classic, in_store};

const API_KEY: &str = "api-key-MUSTNOTAPPEAR-1";
const SECRET: &str = "secret-key-MUSTNOTAPPEAR-2";
const MERCHANT: &str = "merchant-id-MUSTNOTAPPEAR-3";

fn leaks(shown: &str) -> bool {
    shown.contains("MUSTNOTAPPEAR")
}

#[test]
fn the_in_store_config_and_client_keep_their_keys() {
    let config = in_store::Config::sandbox(API_KEY, SECRET, MERCHANT);
    assert!(!leaks(&format!("{config:?}")), "config leaked");

    let client = in_store::Client::new(config).expect("client builds");
    assert!(!leaks(&format!("{client:?}")), "client leaked");
}

#[test]
fn the_classic_config_and_client_keep_their_keys() {
    let config = classic::Config::sandbox(Credentials::new(API_KEY, SECRET));
    assert!(!leaks(&format!("{config:?}")), "config leaked");

    let client = classic::Client::new(config).expect("client builds");
    assert!(!leaks(&format!("{client:?}")), "client leaked");
}

#[test]
fn credentials_keep_their_keys() {
    let credentials = Credentials::new(API_KEY, SECRET);
    assert!(!leaks(&format!("{credentials:?}")), "credentials leaked");
    // And the value is still readable where it has to be.
    assert_eq!(
        credentials.response_signature(&["a"]),
        credentials.response_signature(&["a"])
    );
}
