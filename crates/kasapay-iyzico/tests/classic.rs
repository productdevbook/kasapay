//! The classic API against a mock server, and the signing it carries.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::ErrorKind;
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic::{Association, CardType, Client, Config};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    let config =
        Config::new(&server.uri(), Credentials::new("api-key", "secret-key")).expect("valid base");
    Client::new(config).expect("client builds")
}

fn bin_response() -> serde_json::Value {
    json!({
        "status": "success",
        "locale": "tr",
        "systemTime": 1_770_000_000_i64,
        "binNumber": "535805",
        "cardType": "CREDIT_CARD",
        "cardAssociation": "MASTER_CARD",
        "cardFamily": "Bonus",
        "bankName": "Garanti Bankasi",
        "bankCode": 62,
        "commercial": 0,
    })
}

#[tokio::test]
async fn bin_check_sends_the_documented_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/bin/check"))
        .and(header("content-type", "application/json"))
        .and(header_exists("x-iyzi-rnd"))
        .and(body_json(json!({ "locale": "tr", "binNumber": "535805" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(bin_response()))
        .mount(&server)
        .await;

    let card = client(&server)
        .bin_check("535805")
        .await
        .expect("the BIN resolves");

    assert_eq!(&*card.bin, "535805");
    assert_eq!(card.card_type, Some(CardType::Credit));
    assert_eq!(card.association, Some(Association::MasterCard));
    assert_eq!(card.family.as_deref(), Some("Bonus"));
    assert_eq!(card.bank_code, Some(62));
    assert!(!card.commercial);
    assert_eq!(
        card.raw.text_at("/bankName").as_deref(),
        Some("Garanti Bankasi")
    );
}

#[tokio::test]
async fn the_authorization_header_signs_the_body_that_was_sent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/bin/check"))
        .respond_with(ResponseTemplate::new(200).set_body_json(bin_response()))
        .mount(&server)
        .await;

    client(&server).bin_check("535805").await.expect("resolves");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let request = sent.first().expect("one request");

    let header = request
        .headers
        .get("authorization")
        .expect("an Authorization header")
        .to_str()
        .expect("ascii");
    let random_key = request
        .headers
        .get("x-iyzi-rnd")
        .expect("the random key travels in its own header")
        .to_str()
        .expect("ascii");

    let encoded = header
        .strip_prefix("IYZWSv2 ")
        .expect("the scheme, then one space");
    let decoded =
        String::from_utf8(BASE64.decode(encoded).expect("valid base64")).expect("valid utf-8");

    // The signature has to cover the bytes the server received, not a
    // re-serialisation of them: a mismatch here reads as bad credentials.
    let body = String::from_utf8(request.body.clone()).expect("valid utf-8");
    let expected = Credentials::new("api-key", "secret-key").signature(
        random_key,
        "/payment/bin/check",
        &body,
    );

    assert_eq!(
        decoded,
        format!("apiKey:api-key&randomKey:{random_key}&signature:{expected}")
    );
    // And the key in the header is the one that was signed with.
    assert!(decoded.contains(&format!("randomKey:{random_key}&")));
}

#[tokio::test]
async fn a_failure_status_carries_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/bin/check"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5008",
            "errorMessage": "Gecersiz bin numarasi",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .bin_check("000000")
        .await
        .expect_err("a failure status is not a card");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5008"));
}

#[tokio::test]
async fn a_rejected_signature_is_an_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/bin/check"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let error = client(&server)
        .bin_check("535805")
        .await
        .expect_err("401 is not a card");
    assert_eq!(error.kind(), ErrorKind::Auth);
}

#[tokio::test]
async fn an_unknown_card_type_is_kept_rather_than_dropped() {
    let server = MockServer::start().await;
    let mut body = bin_response();
    body["cardType"] = json!("VIRTUAL_CARD");
    body["commercial"] = json!(1);
    Mock::given(method("POST"))
        .and(path("/payment/bin/check"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let card = client(&server)
        .bin_check("535805")
        .await
        .expect("the BIN resolves");
    assert_eq!(card.card_type, Some(CardType::Other("VIRTUAL_CARD".into())));
    assert!(card.commercial);
}

#[tokio::test]
async fn stored_cards_come_back_without_a_card_number_in_sight() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cardstorage/cards"))
        .and(body_json(
            json!({ "locale": "tr", "cardUserKey": "user-key-1" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "cardUserKey": "user-key-1",
            "cardDetails": [
                {
                    "cardToken": "tok-1",
                    "cardAlias": "Bonus kartim",
                    "binNumber": "552879",
                    "lastFourDigits": "0004",
                    "cardType": "CREDIT_CARD",
                    "cardAssociation": "MASTER_CARD",
                    "cardFamily": "Bonus",
                    "cardBankName": "Garanti Bankasi",
                },
                { "cardToken": "tok-2", "cardType": "DEBIT_CARD" },
            ],
        })))
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("user-key-1")
        .await
        .expect("the cards list");

    assert_eq!(cards.len(), 2);
    assert_eq!(&*cards[0].token, "tok-1");
    assert_eq!(cards[0].alias.as_deref(), Some("Bonus kartim"));
    assert_eq!(cards[0].last_four.as_deref(), Some("0004"));
    assert_eq!(cards[0].card_type, Some(CardType::Credit));
    assert_eq!(cards[0].association, Some(Association::MasterCard));
    // A card iyzico knows little about is still a card.
    assert_eq!(&*cards[1].token, "tok-2");
    assert!(cards[1].alias.is_none());
    assert_eq!(cards[1].card_type, Some(CardType::Debit));
}

#[tokio::test]
async fn a_user_with_no_stored_cards_is_an_empty_list_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/cardstorage/cards"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("user-key-1")
        .await
        .expect("no cards is a valid answer");
    assert!(cards.is_empty());
}

#[tokio::test]
async fn forgetting_a_card_sends_a_delete_with_a_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/cardstorage/card"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "locale": "tr",
            "cardUserKey": "user-key-1",
            "cardToken": "tok-1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .mount(&server)
        .await;

    client(&server)
        .forget_card("user-key-1", "tok-1")
        .await
        .expect("the card is forgotten");
}

#[tokio::test]
async fn a_refused_delete_is_an_error_carrying_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/cardstorage/card"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5107",
            "errorMessage": "Kart bulunamadi",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .forget_card("user-key-1", "tok-gone")
        .await
        .expect_err("a failure status is not a deletion");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5107"));
}
