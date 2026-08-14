//! Nothing that holds a key may print one.
//!
//! A secret in a log line is an incident, and the usual way one gets there is
//! a `{:?}` on a client or a config that somebody added a field to.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::Secret;
use kasapay_stripe::Stripe;

/// Distinctive enough that a substring search cannot miss it.
const KEY: &str = "sk_test_kasapayMUSTNOTAPPEARinlogs";

#[test]
fn debugging_a_client_does_not_print_the_secret_key() {
    let stripe = Stripe::new(&Secret::new(KEY));
    let shown = format!("{stripe:?}");

    // This holds because async-stripe marks the Authorization header
    // sensitive, and hyper's HeaderValue Debug prints `Sensitive` for one that
    // is. Neither is ours, which is why it is pinned here: if a future
    // async-stripe stops setting the flag, this catches it rather than a log.
    assert!(
        !shown.contains("kasapayMUSTNOTAPPEAR"),
        "the secret key reached a Debug: {shown}"
    );
    assert!(
        !shown.contains(KEY),
        "the secret key reached a Debug: {shown}"
    );
}

#[test]
fn debugging_the_underlying_client_does_not_print_it_either() {
    let stripe = Stripe::new(&Secret::new(KEY));
    // Stripe::client is an escape hatch a caller may well log.
    let shown = format!("{:?}", stripe.client());
    assert!(
        !shown.contains("kasapayMUSTNOTAPPEAR"),
        "the secret key reached a Debug: {shown}"
    );
}
