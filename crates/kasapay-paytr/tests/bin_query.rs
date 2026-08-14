//! PayTR's BIN service against a mock server, and the hash it signs with.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_paytr::{CardKind, CardScheme, Config, Credentials, PayTr};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> PayTr {
    let config = Config::at(
        &server.uri(),
        Credentials::new("merchant-1", "merchant-key", "merchant-salt"),
    )
    .expect("valid base");
    PayTr::new(config).expect("client builds")
}

/// What `reqwest`'s form encoder does to a base64 token.
fn urlencoding(value: &str) -> String {
    value
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D")
}

#[tokio::test]
async fn a_bin_query_signs_the_bin_before_the_merchant_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/bin-detail"))
        .and(body_string_contains("bin_number=45463800"))
        // Computed independently from PayTR's own formula:
        // base64(hmac_sha256(bin_number + merchant_id + merchant_salt, merchant_key)).
        // The BIN comes first here — every other PayTR call starts with the
        // merchant id, and using this crate's usual order gives
        // hH1RMXtWfsKsUrbOmyjFCDueKzGIxoAV88mECC//ILw= instead.
        .and(body_string_contains(urlencoding(
            "RD8PAgPNhFYB5XX3yVbQOd2e7pUJxDBlox6UsssvASM=",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "cardType": "credit",
            "businessCard": "n",
            "bank": "Yapı Kredi",
            "brand": "world",
            "schema": "MASTERCARD",
            "bankCode": "0067",
            "allow_non3d": "Y",
        })))
        .mount(&server)
        .await;

    let card = client(&server)
        .bin_details("45463800")
        .await
        .expect("the BIN reads back")
        .expect("PayTR knows this BIN");

    assert_eq!(card.kind, CardKind::Credit);
    assert!(!card.business);
    assert_eq!(card.bank.as_ref(), "Yapı Kredi");
    // Kept as text: PayTR calls it an int and gives 0010 as the example.
    assert_eq!(card.bank_code.as_ref(), "0067");
    assert_eq!(card.programme.as_deref(), Some("world"));
    assert_eq!(card.scheme, CardScheme::Mastercard);
    assert!(card.non_3d_allowed);
}

/// A six-digit BIN is the shorter form PayTR still takes.
#[tokio::test]
async fn a_six_digit_bin_signs_the_same_way() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/bin-detail"))
        .and(body_string_contains("bin_number=454638"))
        .and(body_string_contains(urlencoding(
            "hH1RMXtWfsKsUrbOmyjFCDueKzGIxoAV88mECC//ILw=",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "cardType": "debit",
            "businessCard": "y",
            "bank": "Akbank",
            // A card in no instalment programme.
            "brand": "none",
            "schema": "VISA",
            // The int reading of a code with no leading zero.
            "bankCode": 46,
            "allow_non3d": "N",
        })))
        .mount(&server)
        .await;

    let card = client(&server)
        .bin_details("454638")
        .await
        .expect("the BIN reads back")
        .expect("PayTR knows this BIN");

    assert_eq!(card.kind, CardKind::Debit);
    assert!(card.business);
    assert_eq!(card.bank_code.as_ref(), "46");
    assert_eq!(card.scheme, CardScheme::Visa);
    assert!(!card.non_3d_allowed);
    // "none" is not a programme: this card cannot be paid in instalments.
    assert!(card.programme.is_none());
}

#[tokio::test]
async fn a_bin_paytr_does_not_know_is_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/bin-detail"))
        // PayTR's documented answer for a card issued outside Turkey.
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed",
        })))
        .mount(&server)
        .await;

    let card = client(&server)
        .bin_details("42424242")
        .await
        .expect("an unknown BIN is an answer, not a failure");
    assert!(card.is_none());
}

#[tokio::test]
async fn a_refused_bin_query_carries_paytrs_own_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/bin-detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "error",
            // This service reports in err_msg, not reason.
            "err_msg": "Zorunlu alan degeri gecersiz veya gonderilmedi: bin_number",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .bin_details("45463800")
        .await
        .expect_err("a refused query is not a card");
    assert_eq!(error.kind(), kasapay_core::ErrorKind::InvalidRequest);
    assert!(error.to_string().contains("bin_number"));
}

#[tokio::test]
async fn a_bin_that_is_not_six_or_eight_digits_never_leaves_the_process() {
    let server = MockServer::start().await;
    // No mock: a request reaching the network would fail the test.
    for bad in ["4546", "4546380", "454638001", "4546380a", ""] {
        let error = client(&server)
            .bin_details(bad)
            .await
            .expect_err("PayTR takes 6 or 8 digits and nothing else");
        assert_eq!(
            error.kind(),
            kasapay_core::ErrorKind::InvalidRequest,
            "{bad}"
        );
    }
}

#[tokio::test]
async fn a_card_type_paytr_does_not_document_is_malformed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/bin-detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "cardType": "prepaid",
            "businessCard": "n",
            "bank": "Akbank",
            "brand": "axess",
            "schema": "VISA",
            "bankCode": "0046",
            "allow_non3d": "Y",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .bin_details("45463800")
        .await
        .expect_err("credit and debit are the documented values");
    assert_eq!(error.kind(), kasapay_core::ErrorKind::Malformed);
}
