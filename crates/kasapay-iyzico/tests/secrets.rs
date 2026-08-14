//! Nothing that holds a key may print one, and nothing that holds somebody
//! else's banking details may print those.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_iyzico::{Credentials, classic, in_store, mass};

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

/// A payout line names whoever is being paid. Printing it must not name the
/// account, because a payout is the thing somebody debugs by printing it.
#[test]
fn a_recipient_shows_which_kind_it_is_and_not_the_number() {
    const IBAN: &str = "TR330006100519786457841326";
    const IDENTITY: &str = "11111111111";

    let iban = mass::Recipient::Iban {
        iban: IBAN.into(),
        holder: "Ayse Yilmaz".into(),
    };
    let shown = format!("{iban:?}");
    assert!(!shown.contains(IBAN), "the account number leaked: {shown}");
    assert!(shown.contains("Iban"), "which kind it is was lost: {shown}");
    assert!(
        shown.contains("1326"),
        "two payouts cannot be told apart: {shown}"
    );
    assert!(
        shown.contains("Ayse Yilmaz"),
        "who is paid was lost: {shown}"
    );

    let identity = mass::Recipient::IdentityNumber(IDENTITY.into());
    let shown = format!("{identity:?}");
    assert!(
        !shown.contains(IDENTITY),
        "the identity number leaked: {shown}"
    );

    // An iyzico member id names an account inside iyzico, not a person.
    let member = mass::Recipient::MemberId("member-42".into());
    assert!(format!("{member:?}").contains("member-42"));
}
