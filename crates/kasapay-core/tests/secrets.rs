//! Nothing that holds a handle may print one.
//!
//! Each adapter has a file with this name, because a secret in a log line is an
//! incident and the usual way one gets there is a `{:?}` on a type somebody
//! added a field to. `kasapay-core` had no such file, and it is where the two
//! worst offenders lived: `NextAction`, which carries Stripe's `client_secret`
//! and iyzico's `paymentSessionToken`, and `Delivery`, which carries a
//! provider's signature and the whole body it signed.
//!
//! Both are reached by `tracing::debug!("{charge:?}")` and
//! `tracing::debug!("{delivery:?}")` — the first thing anybody writes while a
//! payment or a webhook is not working.

use kasapay_core::{Delivery, NextAction, Raw, Secret};

/// Distinctive enough that a substring search cannot miss it.
const HELD: &str = "kasapayMUSTNOTAPPEARinlogs";

#[test]
fn a_client_secret_is_not_printed() {
    let action = NextAction::ConfirmOnClient {
        client_secret: format!("pi_1_secret_{HELD}").into(),
    };
    let shown = format!("{action:?}");

    assert!(
        !shown.contains(HELD),
        "the client secret reached a Debug: {shown}"
    );
    // Stripe's own documentation: it "should not be stored, logged, or exposed
    // to anyone other than the customer" — anybody holding one can confirm or
    // cancel that payment from a browser.
    assert!(
        shown.contains("chars"),
        "and the length is still useful: {shown}"
    );
}

#[test]
fn a_continuation_token_is_not_printed_and_the_address_is() {
    let action = NextAction::Redirect {
        url: "https://provider.test/form/abc".parse().expect("valid url"),
        continuation: Some(HELD.into()),
    };
    let shown = format!("{action:?}");

    assert!(
        !shown.contains(HELD),
        "the continuation token reached a Debug: {shown}"
    );
    // The address is where the payer is sent. A caller who cannot log it
    // cannot log the one thing this variant is for.
    assert!(
        shown.contains("https://provider.test/form/abc"),
        "the address is not printed, which makes this unloggable: {shown}"
    );
}

#[test]
fn a_delivery_prints_which_headers_arrived_and_not_what_they_say() {
    let headers = [
        ("Stripe-Signature", HELD),
        ("Content-Type", "application/json"),
    ];
    let body = format!(r#"{{"secret":"{HELD}"}}"#);
    let delivery = Delivery::new(&headers, body.as_bytes());
    let shown = format!("{delivery:?}");

    assert!(
        !shown.contains(HELD),
        "the signature or the body reached a Debug: {shown}"
    );
    // Which headers arrived is the useful half, and it is safe.
    assert!(
        shown.contains("Stripe-Signature"),
        "the header names are worth keeping: {shown}"
    );
    assert!(shown.contains("bytes"), "and the body's size: {shown}");
}

/// The two that were already right, kept here so the pair reads as a rule.
#[test]
fn a_secret_and_a_raw_body_were_already_silent() {
    assert!(!format!("{:?}", Secret::new(HELD)).contains(HELD));
    assert!(!format!("{:?}", Raw::from_text(HELD)).contains(HELD));
}
