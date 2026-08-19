//! The classic API against a mock server, and the signing it carries.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{
    Currency, ErrorKind, IdempotencyKey, InstrumentId, Money, NextAction, OrderRef, PaymentId,
    Provider, RefundReason, RefundRequest, RefundStatus, Status,
};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic::{
    Association, CardType, Client, Config, FormToken, Reason, ReasonCode, checkout, saved,
};
use serde_json::json;
use wiremock::matchers::{
    body_json, body_string_contains, header, header_exists, method, path, query_param,
};
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
    assert_eq!(cards[0].token, InstrumentId::issued("tok-1"));
    assert_eq!(cards[0].alias.as_deref(), Some("Bonus kartim"));
    assert_eq!(cards[0].last_four.as_deref(), Some("0004"));
    assert_eq!(cards[0].card_type, Some(CardType::Credit));
    assert_eq!(cards[0].association, Some(Association::MasterCard));
    // A card iyzico knows little about is still a card.
    assert_eq!(cards[1].token, InstrumentId::issued("tok-2"));
    assert!(cards[1].alias.is_none());
    assert_eq!(cards[1].card_type, Some(CardType::Debit));
}

/// `Provider::instruments` is the same `/cardstorage/cards` call, with the
/// alias — or, failing that, the last four digits — as the label, and each
/// card's own JSON kept on `raw` rather than the list envelope.
#[tokio::test]
async fn provider_instruments_lists_the_same_cards() {
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
                },
                { "cardToken": "tok-2", "cardType": "DEBIT_CARD", "lastFourDigits": "1111" },
            ],
        })))
        .mount(&server)
        .await;

    let instruments = client(&server)
        .instruments("user-key-1")
        .await
        .expect("the list reads back");

    assert_eq!(instruments.len(), 2);
    assert_eq!(instruments[0].id, InstrumentId::issued("tok-1"));
    assert_eq!(instruments[0].label.as_deref(), Some("Bonus kartim"));
    assert_eq!(
        instruments[0].raw.text_at("/binNumber").as_deref(),
        Some("552879")
    );
    // No alias on the second card: the last four digits stand in for it.
    assert_eq!(instruments[1].id, InstrumentId::issued("tok-2"));
    assert_eq!(instruments[1].label.as_deref(), Some("•••• 1111"));
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
        .forget_card("user-key-1", &InstrumentId::issued("tok-1"))
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
        .forget_card("user-key-1", &InstrumentId::issued("tok-gone"))
        .await
        .expect_err("a failure status is not a deletion");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5107"));
}

fn buyer() -> checkout::Buyer {
    checkout::Buyer {
        id: "buyer-1".into(),
        name: "Ayse".into(),
        surname: "Yilmaz".into(),
        identity_number: "11111111111".into(),
        email: "ayse@example.test".into(),
        phone: "+905350000000".into(),
        registration_address: "Bagdat Cad. 1".into(),
        city: "Istanbul".into(),
        country: "Turkey".into(),
        zip_code: None,
        ip: None,
    }
}

fn address() -> checkout::Address {
    checkout::Address {
        contact_name: "Ayse Yilmaz".into(),
        address: "Bagdat Cad. 1".into(),
        city: "Istanbul".into(),
        country: "Turkey".into(),
        zip_code: None,
    }
}

fn item(price: &str) -> checkout::BasketItem {
    checkout::BasketItem {
        id: "item-1".into(),
        name: "Kahve".into(),
        category: "Icecek".into(),
        kind: checkout::ItemKind::Physical,
        price: Money::parse(price, Currency::Try).expect("valid amount"),
    }
}

fn saved_card() -> saved::Card {
    saved::Card::new("card-user-key-1", InstrumentId::issued("card-token-1"))
        .expect("two handles name a stored card")
}

fn saved_payment() -> saved::Payment {
    saved::Payment::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        buyer(),
        saved_card(),
    )
    .billing_address(address())
    .item(item("149.90"))
    .build()
    .expect("valid payment")
}

/// The body iyzico is sent, with the two handles where a card would be.
fn saved_card_body() -> serde_json::Value {
    json!({
        "locale": "tr",
        "conversationId": "ord-1",
        "price": "149.90",
        "paidPrice": "149.90",
        "currency": "TRY",
        "basketId": "ord-1",
        "paymentCard": {
            "cardUserKey": "card-user-key-1",
            "cardToken": "card-token-1",
        },
        "buyer": {
            "id": "buyer-1", "name": "Ayse", "surname": "Yilmaz",
            "identityNumber": "11111111111", "email": "ayse@example.test",
            "gsmNumber": "+905350000000", "registrationAddress": "Bagdat Cad. 1",
            "city": "Istanbul", "country": "Turkey",
        },
        "billingAddress": {
            "contactName": "Ayse Yilmaz", "address": "Bagdat Cad. 1",
            "city": "Istanbul", "country": "Turkey",
        },
        "shippingAddress": {
            "contactName": "Ayse Yilmaz", "address": "Bagdat Cad. 1",
            "city": "Istanbul", "country": "Turkey",
        },
        "basketItems": [{
            "id": "item-1", "name": "Kahve", "category1": "Icecek",
            "itemType": "PHYSICAL", "price": "149.90",
        }],
    })
}

/// A stored-card payment as iyzico answers it: no `paymentStatus`, a
/// `fraudStatus`, and a signature over the payment's own six fields.
fn saved_card_response(fraud_status: i64) -> serde_json::Value {
    json!({
        "status": "success",
        "paymentId": "12345678",
        "currency": "TRY",
        "basketId": "ord-1",
        "conversationId": "ord-1",
        "paidPrice": "149.90",
        "price": "149.90",
        "fraudStatus": fraud_status,
        // HMAC-SHA256("12345678:TRY:ord-1:ord-1:149.9:149.9", "secret-key")
        "signature": "d5595c2f02e49a4e81dd1cdc9b03d2b9d9bd90910b9f6ab004abfce1247b5440",
    })
}

#[tokio::test]
async fn charging_a_stored_card_sends_two_handles_and_no_card_number() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/auth"))
        .and(header_exists("authorization"))
        .and(body_json(saved_card_body()))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_card_response(1)))
        .mount(&server)
        .await;

    let iyzipay = client(&server);
    // A capability that says yes over a call that then fails is a bug in the
    // adapter, so the two are asserted together.
    assert!(iyzipay.capabilities().saved_instruments);
    let charge = iyzipay
        .pay_with_saved_card(&saved_payment())
        .await
        .expect("the stored card is charged");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.id, Some(PaymentId::issued("12345678")));
    assert_eq!(charge.amount.minor_units(), 14_990);

    // The body matcher above pins the shape; this pins the thing that matters
    // about it, and would fail the day a card field is added to the request.
    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body =
        String::from_utf8(sent.first().expect("one request").body.clone()).expect("valid utf-8");
    for field in [
        "cardNumber",
        "cvc",
        "expireMonth",
        "expireYear",
        "cardHolderName",
    ] {
        assert!(!body.contains(field), "{field} reached iyzico: {body}");
    }
}

/// The pre-authorisation is `/payment/auth`'s own body at another path, and
/// what comes back is money held rather than money taken.
#[tokio::test]
async fn holding_money_on_a_stored_card_is_authorized_rather_than_captured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/preauth"))
        .and(body_json(saved_card_body()))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_card_response(1)))
        .mount(&server)
        .await;

    let iyzipay = client(&server);
    assert!(iyzipay.capabilities().separate_capture);
    let charge = iyzipay
        .preauth_with_saved_card(&saved_payment())
        .await
        .expect("the hold is placed");

    assert_eq!(charge.status, Status::Authorized);
    // Held is not settled: the money moves when the capture does.
    assert!(charge.status.is_open());
    assert_eq!(charge.amount.minor_units(), 14_990);
}

/// A hold iyzico's fraud filters are still reading is not a hold either.
#[tokio::test]
async fn a_preauth_under_fraud_review_is_not_reported_as_a_hold() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/preauth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_card_response(0)))
        .mount(&server)
        .await;

    let charge = client(&server)
        .preauth_with_saved_card(&saved_payment())
        .await
        .expect("the request itself worked");
    assert_eq!(charge.status, Status::Pending);
}

/// `paidPrice` is what postauth collects, and it may be less than was held.
#[tokio::test]
async fn capturing_part_of_a_hold_sends_the_amount_to_postauth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/postauth"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            "paidPrice": "100.00",
            "currency": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "currency": "TRY",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "100.00",
            "price": "149.90",
            "fraudStatus": 1,
            // HMAC-SHA256("12345678:TRY:ord-1:ord-1:100:149.9", "secret-key")
            "signature": "f3336037887854fee0c200de41e7e07adef6acf2b74687de6f1c3afe4c114c54",
        })))
        .mount(&server)
        .await;

    let iyzipay = client(&server);
    assert!(iyzipay.capabilities().partial_capture);
    let charge = iyzipay
        .capture(
            &PaymentId::issued("12345678"),
            Some(Money::parse("100.00", Currency::Try).expect("valid amount")),
            None,
        )
        .await
        .expect("the hold is taken");

    assert_eq!(charge.status, Status::Captured);
    assert!(!charge.status.is_open());
    // What was taken, with the basket total beside it.
    assert_eq!(charge.amount.minor_units(), 10_000);
    assert_eq!(charge.order_amount.map(Money::minor_units), Some(14_990));
}

/// iyzico's `paidPrice` is required, so a capture with no amount has to find
/// out what was held before it can ask for it.
#[tokio::test]
async fn a_capture_with_no_amount_reads_the_payment_first() {
    let server = MockServer::start().await;
    let mut authorised = saved_card_response(1);
    authorised["paymentStatus"] = "SUCCESS".into();
    Mock::given(method("POST"))
        .and(path("/payment/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(authorised))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/payment/postauth"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            // The figure the payment answered, not a guess.
            "paidPrice": "149.90",
            "currency": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_card_response(1)))
        .mount(&server)
        .await;

    let charge = client(&server)
        .capture(&PaymentId::issued("12345678"), None, None)
        .await
        .expect("the whole hold is taken");
    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount.minor_units(), 14_990);
}

/// No mock is mounted: a key iyzico cannot honour never reaches the network.
#[tokio::test]
async fn a_capture_carrying_an_idempotency_key_is_refused() {
    let server = MockServer::start().await;
    let error = client(&server)
        .capture(
            &PaymentId::issued("12345678"),
            Some(Money::parse("100.00", Currency::Try).expect("valid amount")),
            Some(&IdempotencyKey::new("capture-1")),
        )
        .await
        .expect_err("iyzico accepts no idempotency key");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn a_payment_iyzicos_fraud_filters_are_still_reading_is_not_money_taken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved_card_response(0)))
        .mount(&server)
        .await;

    let charge = client(&server)
        .pay_with_saved_card(&saved_payment())
        .await
        .expect("the request itself worked");
    // iyzico says to wait for their notification, so this is not Captured.
    assert_eq!(charge.status, Status::Pending);
    assert!(charge.status.is_open());
}

#[tokio::test]
async fn a_stored_card_iyzico_no_longer_holds_carries_its_own_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5107",
            "errorMessage": "Kart bulunamadi",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .pay_with_saved_card(&saved_payment())
        .await
        .expect_err("a failure status is not a payment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5107"));
}

#[tokio::test]
async fn a_forged_stored_card_payment_is_refused() {
    let server = MockServer::start().await;
    let mut body = saved_card_response(1);
    // Ten times the amount, the signature left as it was.
    body["paidPrice"] = json!("1499.00");
    Mock::given(method("POST"))
        .and(path("/payment/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let error = client(&server)
        .pay_with_saved_card(&saved_payment())
        .await
        .expect_err("a tampered amount must not become a Charge");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

#[tokio::test]
async fn an_instalment_surcharge_is_charged_and_reported_apart_from_the_basket() {
    let server = MockServer::start().await;
    let mut expected = saved_card_body();
    expected["paidPrice"] = json!("164.90");
    expected["installment"] = json!(3);
    Mock::given(method("POST"))
        .and(path("/payment/auth"))
        .and(body_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "currency": "TRY",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "164.90",
            "price": "149.90",
            "installment": 3,
            "fraudStatus": 1,
            // HMAC-SHA256("12345678:TRY:ord-1:ord-1:164.9:149.9", "secret-key")
            "signature": "2e929a2f5c4730d20969459f82d9cc8d1dbc9302d8381b4223117e68c06a5ca1",
        })))
        .mount(&server)
        .await;

    let payment = saved::Payment::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        buyer(),
        saved_card(),
    )
    .paid_price(Money::parse("164.90", Currency::Try).expect("valid amount"))
    .billing_address(address())
    .item(item("149.90"))
    .instalment(3)
    .build()
    .expect("valid payment");

    let charge = client(&server)
        .pay_with_saved_card(&payment)
        .await
        .expect("the stored card is charged");
    // What moves is the surcharged amount; the basket is what the goods came to.
    assert_eq!(charge.amount.minor_units(), 16_490);
    assert_eq!(charge.order_amount.map(Money::minor_units), Some(14_990));
}

#[test]
fn a_stored_card_payment_is_checked_the_way_a_form_is() {
    let bare = || {
        saved::Payment::builder(
            OrderRef::new("ord-1"),
            Money::parse("149.90", Currency::Try).expect("valid amount"),
            buyer(),
            saved_card(),
        )
    };
    assert_eq!(
        bare().item(item("149.90")).build().expect_err("no address"),
        saved::PaymentError::NoBillingAddress
    );
    assert_eq!(
        bare()
            .billing_address(address())
            .build()
            .expect_err("no basket"),
        saved::PaymentError::EmptyBasket
    );
    assert!(matches!(
        bare()
            .billing_address(address())
            .item(item("100.00"))
            .build()
            .expect_err("the lines do not come to the total"),
        saved::PaymentError::BasketDoesNotAddUp { .. }
    ));
    assert_eq!(
        bare()
            .billing_address(address())
            .item(item("149.90"))
            .instalment(0)
            .build()
            .expect_err("zero instalments takes no money"),
        saved::PaymentError::NoInstalments
    );
}

fn form() -> checkout::CheckoutForm {
    checkout::CheckoutForm::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        "https://merchant.test/callback".parse().expect("valid url"),
        buyer(),
    )
    .billing_address(address())
    .item(item("149.90"))
    .build()
    .expect("valid form")
}

#[tokio::test]
async fn opening_a_form_gives_a_page_to_send_the_payer_to() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/initialize/auth/ecom"))
        .and(header_exists("authorization"))
        // Decimal strings, never floats: 149.90 and not 149.90000000000001.
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "ord-1",
            "price": "149.90",
            "paidPrice": "149.90",
            "currency": "TRY",
            "basketId": "ord-1",
            "callbackUrl": "https://merchant.test/callback",
            "buyer": {
                "id": "buyer-1", "name": "Ayse", "surname": "Yilmaz",
                "identityNumber": "11111111111", "email": "ayse@example.test",
                "gsmNumber": "+905350000000", "registrationAddress": "Bagdat Cad. 1",
                "city": "Istanbul", "country": "Turkey",
            },
            "billingAddress": {
                "contactName": "Ayse Yilmaz", "address": "Bagdat Cad. 1",
                "city": "Istanbul", "country": "Turkey",
            },
            "shippingAddress": {
                "contactName": "Ayse Yilmaz", "address": "Bagdat Cad. 1",
                "city": "Istanbul", "country": "Turkey",
            },
            "basketItems": [{
                "id": "item-1", "name": "Kahve", "category1": "Icecek",
                "itemType": "PHYSICAL", "price": "149.90",
            }],
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "conversationId": "ord-1",
            "token": "cf-token-1",
            "paymentPageUrl": "https://sandbox-cpp.iyzipay.com/?token=cf-token-1",
            // HMAC-SHA256("ord-1:cf-token-1", "secret-key")
            "signature": "f853d25b67c4d33bc566e9265922dcc1b83f6d980652f4463435b35044ef3f76",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .start_checkout_form(&form())
        .await
        .expect("the form opens");

    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(charge.amount.minor_units(), 14_990);
    // iyzico names no payment until the payer finishes, and the form's token is
    // not a payment id.
    assert_eq!(charge.id, None);
    match charge.next_action.expect("a form to send the payer to") {
        NextAction::Redirect { url, continuation } => {
            assert_eq!(
                url.as_str(),
                "https://sandbox-cpp.iyzipay.com/?token=cf-token-1"
            );
            // The token has to survive: the callback carries nothing else.
            assert_eq!(continuation.as_deref(), Some("cf-token-1"));
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
}

/// The form is the pan-free way into iyzico's vault, and the key decides whose.
///
/// The test above pins the body exactly and carries no `cardUserKey`, so the
/// pair of them says the field goes out when it is set and is absent when it is
/// not — an empty one would file the payer's card under a key of iyzico's
/// choosing.
#[tokio::test]
async fn a_form_offers_the_payer_the_cards_iyzico_already_holds_for_them() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/initialize/auth/ecom"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "conversationId": "ord-1",
            "token": "cf-token-1",
            "paymentPageUrl": "https://sandbox-cpp.iyzipay.com/?token=cf-token-1",
            "signature": "f853d25b67c4d33bc566e9265922dcc1b83f6d980652f4463435b35044ef3f76",
        })))
        .mount(&server)
        .await;

    let with_key = checkout::CheckoutForm::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        "https://merchant.test/callback".parse().expect("valid url"),
        buyer(),
    )
    .billing_address(address())
    .item(item("149.90"))
    .card_user_key("card-user-key-1")
    .build()
    .expect("valid form");

    client(&server)
        .start_checkout_form(&with_key)
        .await
        .expect("the form opens");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body: serde_json::Value =
        serde_json::from_slice(&sent.first().expect("one request").body).expect("valid json");
    assert_eq!(body["cardUserKey"], json!("card-user-key-1"));
}

/// The pair iyzico answers when the payer saved a card, and where to read it.
#[tokio::test]
async fn a_form_the_payer_saved_a_card_on_answers_the_handles_for_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "12345678",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "149.90",
            "price": "149.90",
            "currency": "TRY",
            "token": "cf-token-1",
            "cardUserKey": "card-user-key-1",
            "cardToken": "card-token-1",
            "lastFourDigits": "0004",
            "signature": "b929da899af8c2c2bc4de9cc44791977115a937c4ea712fa9256ef34a35fa946",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect("the form reads back");

    // Not on the Charge: a saved card is one provider's idea, and the shared
    // type has no field for it.
    let key = charge.raw.text_at("/cardUserKey").expect("the vault's key");
    let token = charge.raw.text_at("/cardToken").expect("the card's token");
    let card = saved::Card::new(key, InstrumentId::issued(token)).expect("a card to charge again");
    assert_eq!(card.user_key(), "card-user-key-1");
    assert_eq!(card.token().as_str(), "card-token-1");
}

/// The same form at another path, and the caller says which when they read it
/// back — iyzico's answer does not.
#[tokio::test]
async fn a_form_that_holds_the_money_opens_at_the_preauth_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/payment/iyzipos/checkoutform/initialize/preauth/ecom",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "conversationId": "ord-1",
            "token": "cf-token-1",
            "paymentPageUrl": "https://sandbox-cpp.iyzipay.com/?token=cf-token-1",
            // HMAC-SHA256("ord-1:cf-token-1", "secret-key")
            "signature": "f853d25b67c4d33bc566e9265922dcc1b83f6d980652f4463435b35044ef3f76",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .start_checkout_form_preauth(&form())
        .await
        .expect("the form opens");

    // Nothing has happened yet either way: the payer has not finished.
    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(charge.id, None);
}

/// iyzico answers `SUCCESS` for a payment taken and for one only held, so the
/// caller who opened the form says which they opened.
#[tokio::test]
async fn a_finished_preauth_form_reports_money_held_rather_than_taken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "12345678",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "149.90",
            "price": "149.90",
            "currency": "TRY",
            "token": "cf-token-1",
            "signature": "b929da899af8c2c2bc4de9cc44791977115a937c4ea712fa9256ef34a35fa946",
        })))
        .mount(&server)
        .await;

    let iyzipay = client(&server);
    let held = iyzipay
        .checkout_result_preauth(&FormToken::issued("cf-token-1"))
        .await
        .expect("the form reads back");
    assert_eq!(held.status, Status::Authorized);

    // The same bytes read as the other kind of form say the money was taken.
    let taken = iyzipay
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect("the form reads back");
    assert_eq!(taken.status, Status::Captured);
}

#[tokio::test]
async fn a_finished_form_reports_the_payment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .and(body_json(json!({ "locale": "tr", "token": "cf-token-1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "12345678",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "149.90",
            "price": "149.90",
            "currency": "TRY",
            "token": "cf-token-1",
            // Signed over the amounts without their trailing zeros: 149.9.
            "signature": "b929da899af8c2c2bc4de9cc44791977115a937c4ea712fa9256ef34a35fa946",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect("the form reads back");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.id, Some(PaymentId::issued("12345678")));
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert_eq!(
        charge.order.map(|o| o.as_str().to_owned()),
        Some("ord-1".to_owned())
    );
}

#[tokio::test]
async fn a_query_that_worked_can_still_report_a_refused_card() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "FAILURE",
            "paymentId": "12345678",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            "paidPrice": "149.90",
            "price": "149.90",
            "currency": "TRY",
            "token": "cf-token-1",
            "signature": "aa62235f8aa986f865101ab8329d38b9a5a5a2c914fd819c1690ac6083afd983",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect("the query itself worked");
    // status: success means the query worked, not that the payment did.
    assert_eq!(charge.status, Status::Failed);
}

#[tokio::test]
async fn a_form_the_payer_has_not_finished_is_still_pending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .mount(&server)
        .await;

    // Nothing is signed yet either, so this is the one case that needs the
    // opt-out. The test below is what happens without it.
    let config = Config::new(&server.uri(), Credentials::new("api-key", "secret-key"))
        .expect("valid base")
        .allow_unsigned();
    let charge = Client::new(config)
        .expect("client builds")
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect("the query worked");
    assert_eq!(charge.status, Status::Pending);
    assert!(charge.status.is_open());
    // iyzico has issued no paymentId yet, and an empty one would be a handle
    // to a payment nobody made.
    assert_eq!(charge.id, None);
}

#[tokio::test]
async fn an_unsigned_response_is_refused_by_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "12345678",
            "paidPrice": "149.90",
            "currency": "TRY",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect_err("an unsigned payment must not become a Charge");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn a_forged_result_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/checkoutform/auth/ecom/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "12345678",
            "basketId": "ord-1",
            "conversationId": "ord-1",
            // The amount raised tenfold, the signature left as it was.
            "paidPrice": "1499.00",
            "price": "1499.00",
            "currency": "TRY",
            "token": "cf-token-1",
            "signature": "b929da899af8c2c2bc4de9cc44791977115a937c4ea712fa9256ef34a35fa946",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .checkout_result(&FormToken::issued("cf-token-1"))
        .await
        .expect_err("a tampered amount must not become a Charge");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

#[test]
fn a_basket_that_does_not_add_up_is_refused_before_sending() {
    let error = checkout::CheckoutForm::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        "https://merchant.test/callback".parse().expect("valid url"),
        buyer(),
    )
    .billing_address(address())
    .item(item("100.00"))
    .build()
    .expect_err("the lines do not come to the total");

    // iyzico refuses this with an error code nobody can read; better to say so
    // here, where the two numbers are in hand.
    assert!(matches!(
        error,
        checkout::CheckoutFormError::BasketDoesNotAddUp { .. }
    ));
}

#[test]
fn a_form_needs_a_billing_address_and_a_basket() {
    let bare = || {
        checkout::CheckoutForm::builder(
            OrderRef::new("ord-1"),
            Money::parse("149.90", Currency::Try).expect("valid amount"),
            "https://merchant.test/callback".parse().expect("valid url"),
            buyer(),
        )
    };
    assert_eq!(
        bare().item(item("149.90")).build().expect_err("no address"),
        checkout::CheckoutFormError::NoBillingAddress
    );
    assert_eq!(
        bare()
            .billing_address(address())
            .build()
            .expect_err("no basket"),
        checkout::CheckoutFormError::EmptyBasket
    );
}

#[test]
fn shipping_defaults_to_the_billing_address() {
    let form = form();
    assert_eq!(form.shipping_address.address, form.billing_address.address);
    // And paid_price defaults to the basket total when nothing is added.
    assert_eq!(form.paid_price, form.price);
}

#[tokio::test]
async fn a_refund_takes_an_amount_back_and_is_verified() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            "price": "50.00",
            "currency": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "conversationId": "12345678",
            // iyzico answers the amount without its trailing zeros, and signs
            // it that way too.
            "price": "50",
            "currency": "TRY",
            "hostReference": "host-ref-1",
            "signature": "57967ab442a60d7d8e44162f5f9807680a9eaa94d41421f5b0b52b9a4a0609a8",
        })))
        .mount(&server)
        .await;

    let reversal = client(&server)
        .refund(
            &PaymentId::issued("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            None,
        )
        .await
        .expect("the refund goes through");

    assert_eq!(reversal.payment, Some(PaymentId::issued("12345678")));
    assert_eq!(reversal.amount.minor_units(), 5000);
    assert_eq!(reversal.host_reference.as_deref(), Some("host-ref-1"));
}

/// The shared refund is the same call, with the reason mapped onto iyzico's.
#[tokio::test]
async fn the_shared_refund_sends_iyzicos_own_reason_and_answers_a_settled_refund() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            "price": "50.00",
            "currency": "TRY",
            "reason": "OTHER",
            "description": "shop closed",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "conversationId": "12345678",
            "price": "50",
            "currency": "TRY",
            "hostReference": "host-ref-1",
            "signature": "57967ab442a60d7d8e44162f5f9807680a9eaa94d41421f5b0b52b9a4a0609a8",
        })))
        .mount(&server)
        .await;

    let request = RefundRequest::builder(PaymentId::issued("12345678"))
        .amount(Money::parse("50.00", Currency::Try).expect("valid amount"))
        .reason(RefundReason::Other("shop closed".into()))
        .build()
        .expect("valid request");
    let refund = Provider::refund(&client(&server), &request)
        .await
        .expect("the refund goes through");

    assert_eq!(refund.amount.minor_units(), 5000);
    assert_eq!(refund.status, RefundStatus::Succeeded);
    assert!(!refund.status.is_open());
    // iyzico names no refund; the bank reference on the body is all there is.
    assert!(refund.id.is_none());
    assert_eq!(
        refund.raw.text_at("/hostReference").as_deref(),
        Some("host-ref-1")
    );
}

/// Both of these are refused before a socket opens, so no mock is mounted.
#[tokio::test]
async fn the_shared_refund_refuses_what_iyzico_cannot_do() {
    let server = MockServer::start().await;
    let client = client(&server);

    let no_amount = RefundRequest::builder(PaymentId::issued("12345678"))
        .build()
        .expect("valid request");
    let error = Provider::refund(&client, &no_amount)
        .await
        .expect_err("iyzico has no refund-the-rest request");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let with_key = RefundRequest::builder(PaymentId::issued("12345678"))
        .amount(Money::parse("50.00", Currency::Try).expect("valid amount"))
        .idempotency_key(IdempotencyKey::new("refund-1"))
        .build()
        .expect("valid request");
    let error = Provider::refund(&client, &with_key)
        .await
        .expect_err("a key iyzico cannot honour is not silently dropped");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

/// The one endpoint in this API that types an amount as a number.
#[tokio::test]
async fn instalments_send_the_price_as_a_number_and_read_it_back_as_money() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/installment"))
        // A number, not a string: iyzico types this field as one here and as a
        // decimal string on every other endpoint of the same API.
        .and(body_string_contains(r#""price":149.90"#))
        .and(body_string_contains(r#""binNumber":"55440400""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
            "installmentDetails": [{
                "binNumber": "55440400",
                "price": 149.90,
                "cardType": "CREDIT_CARD",
                "cardAssociation": "MASTER_CARD",
                "cardFamilyName": "Bonus",
                "force3ds": 0,
                "bankCode": 62,
                "bankName": "Garanti Bankasi",
                "forceCvc": 0,
                "commercial": 0,
                "dccEnabled": 0,
                "agricultureEnabled": 0,
                "installmentPrices": [
                    { "installmentPrice": 149.90, "totalPrice": 149.90, "installmentNumber": 1 },
                    { "installmentPrice": 27.13, "totalPrice": 162.78, "installmentNumber": 6 },
                ],
            }],
        })))
        .mount(&server)
        .await;

    let asked = Money::parse("149.90", Currency::Try).expect("valid amount");
    let options = client(&server)
        .instalments(asked, Some("55440400"))
        .await
        .expect("iyzico answers");

    let card = options.cards.first().expect("one card family");
    assert_eq!(card.card_type, Some(CardType::Credit));
    assert_eq!(card.association, Some(Association::MasterCard));
    assert_eq!(card.family.as_deref(), Some("Bonus"));
    assert!(!card.force_3ds);
    assert_eq!(card.price.minor_units(), 14_990);

    // A single payment is one of the options rather than the absence of one.
    let single = card.option(1).expect("paying it all at once");
    assert_eq!(single.total.minor_units(), 14_990);
    assert_eq!(single.surcharge(asked).map(Money::minor_units), Some(0));

    let six = card.largest().expect("the longest iyzico allows");
    assert_eq!(six.count, 6);
    assert_eq!(six.each.minor_units(), 2713);
    // What the payment opens for, and what it costs over the basket.
    assert_eq!(six.total.minor_units(), 16_278);
    assert_eq!(six.surcharge(asked).map(Money::minor_units), Some(1288));
}

/// Asking about the amount alone is what a checkout draws before the payer has
/// typed a card number — which this crate never sees anyway.
#[tokio::test]
async fn instalments_without_a_bin_ask_about_the_amount_alone() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/installment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "installmentDetails": [
                { "cardFamilyName": "Bonus", "price": 100, "installmentPrices": [] },
                { "cardFamilyName": "Axess", "price": 100, "installmentPrices": [
                    { "installmentPrice": 34.0, "totalPrice": 102.0, "installmentNumber": 3 },
                ]},
            ],
        })))
        .mount(&server)
        .await;

    let options = client(&server)
        .instalments(
            Money::parse("100.00", Currency::Try).expect("valid amount"),
            None,
        )
        .await
        .expect("iyzico answers");

    assert_eq!(options.cards.len(), 2);
    // A family with no options is a card that cannot be paid in instalments
    // here, which is an answer rather than a failure.
    assert!(options.cards[0].options.is_empty());
    assert!(options.cards[0].largest().is_none());
    assert_eq!(options.cards[1].options.len(), 1);

    let sent = &server.received_requests().await.expect("recorded")[0];
    let body = String::from_utf8_lossy(&sent.body);
    assert!(!body.contains("binNumber"), "{body}");
}

/// Neither of these reaches the network, so no mock is mounted.
#[tokio::test]
async fn instalments_refuse_what_iyzico_would_refuse() {
    let server = MockServer::start().await;
    let iyzipay = client(&server);

    assert_eq!(
        iyzipay
            .instalments(Money::from_minor_units(0, Currency::Try), None)
            .await
            .expect_err("nothing is paid in instalments")
            .kind(),
        ErrorKind::InvalidRequest
    );
    assert_eq!(
        iyzipay
            .instalments(
                Money::parse("100.00", Currency::Try).expect("valid amount"),
                Some("5544"),
            )
            .await
            .expect_err("a BIN is 6 or 8 digits")
            .kind(),
        ErrorKind::InvalidRequest
    );
}

/// The question a caller asks when a charge timed out: did it land?
#[tokio::test]
async fn a_payment_is_found_by_the_conversation_id_it_was_made_with() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/reporting/payment/details"))
        .and(query_param("paymentConversationId", "ord-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payments": [{
                "paymentId": "12345678",
                "paymentStatus": 1,
                "price": "50.00",
                "paidPrice": "50.00",
                "currency": "TRY",
                "paymentConversationId": "ord-1",
            }],
        })))
        .mount(&server)
        .await;

    let found = client(&server)
        .lookup(&OrderRef::new("ord-1"))
        .await
        .expect("iyzico answers")
        .expect("a payment under this reference");

    assert_eq!(found.status, Status::Captured);
    assert_eq!(found.amount.minor_units(), 5000);
    assert_eq!(found.id.as_ref().map(PaymentId::as_str), Some("12345678"));
    assert_eq!(found.order.as_ref().map(OrderRef::as_str), Some("ord-1"));
}

/// Nothing under the reference: the charge never landed, and sending it again
/// is safe.
#[tokio::test]
async fn a_conversation_id_iyzico_has_no_payment_for_is_no_payment() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/reporting/payment/details"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payments": [],
        })))
        .mount(&server)
        .await;

    let found = client(&server)
        .lookup(&OrderRef::new("ord-1"))
        .await
        .expect("iyzico answers");
    assert!(found.is_none());
}

/// `2` is a refusal **or** a payment still at 3-D Secure, and iyzico does not
/// say which. Reading it as failed is how a caller sends a second payment
/// after the first one's payer.
#[tokio::test]
async fn a_payment_that_may_still_be_at_three_d_secure_is_not_reported_as_failed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/reporting/payment/details"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payments": [{
                "paymentId": "12345678",
                "paymentStatus": 2,
                "price": "50.00",
                "paidPrice": "50.00",
                "currency": "TRY",
            }],
        })))
        .mount(&server)
        .await;

    let found = client(&server)
        .lookup(&OrderRef::new("ord-1"))
        .await
        .expect("iyzico answers")
        .expect("a payment under this reference");
    assert_eq!(found.status, Status::Pending);
    assert!(found.status.is_open());
}

#[tokio::test]
async fn a_forged_refund_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "conversationId": "12345678",
            // Ten times the amount, the signature left as it was.
            "price": "500",
            "currency": "TRY",
            "signature": "57967ab442a60d7d8e44162f5f9807680a9eaa94d41421f5b0b52b9a4a0609a8",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund(
            &PaymentId::issued("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            None,
        )
        .await
        .expect_err("a tampered refund must not be believed");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

#[tokio::test]
async fn iyzicos_own_retryable_flag_decides_whether_a_failure_is_worth_repeating() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5088",
            "errorMessage": "Sistem hatasi",
            "retryable": true,
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund(
            &PaymentId::issued("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            None,
        )
        .await
        .expect_err("a failure status is not a refund");
    assert_eq!(error.code(), Some("5088"));
    // iyzico said so itself, rather than us guessing from the message.
    assert!(error.is_retryable());
}

#[tokio::test]
async fn a_refund_iyzico_will_never_accept_is_not_retryable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5090",
            "errorMessage": "Iade tutari odeme tutarindan buyuk olamaz",
            "retryable": false,
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund(
            &PaymentId::issued("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            None,
        )
        .await
        .expect_err("refunding more than was taken is not a refund");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn a_cancel_is_accepted_unsigned_because_iyzico_does_not_sign_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/cancel"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "price": "149.90",
            "currency": "TRY",
        })))
        .mount(&server)
        .await;

    // The default client requires a signature everywhere else; cancel is the
    // one operation iyzico documents no signature for.
    let reversal = client(&server)
        .cancel(&PaymentId::issued("12345678"), None)
        .await
        .expect("the cancel goes through");
    assert_eq!(reversal.amount.minor_units(), 14_990);
}

#[tokio::test]
async fn refunding_one_line_of_a_basket_names_the_transaction_not_the_payment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/refund"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "txn-9",
            "paymentTransactionId": "txn-9",
            "price": "20.00",
            "currency": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5092",
            "errorMessage": "Islem bulunamadi",
            "retryable": false,
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund_transaction(
            "txn-9",
            Money::parse("20.00", Currency::Try).expect("valid"),
            None,
        )
        .await
        .expect_err("no such transaction");
    assert_eq!(error.code(), Some("5092"));
}

#[tokio::test]
async fn a_reason_and_its_description_both_go_out_on_a_refund() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payment/refund"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            "price": "50.00",
            "currency": "TRY",
            "reason": "BUYER_REQUEST",
            "description": "returned unopened",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "conversationId": "12345678",
            "price": "50",
            "currency": "TRY",
            "signature": "57967ab442a60d7d8e44162f5f9807680a9eaa94d41421f5b0b52b9a4a0609a8",
        })))
        .mount(&server)
        .await;

    let reason = Reason::new(ReasonCode::BuyerRequest).describe("returned unopened");
    let reversal = client(&server)
        .refund(
            &PaymentId::issued("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            Some(&reason),
        )
        .await
        .expect("the refund goes through");

    assert_eq!(reversal.amount.minor_units(), 5000);
}

#[tokio::test]
async fn a_reason_without_a_description_sends_no_description_field() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/cancel"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "12345678",
            "paymentId": "12345678",
            "reason": "FRAUD",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": "12345678",
            "price": "149.90",
            "currency": "TRY",
        })))
        .mount(&server)
        .await;

    let reversal = client(&server)
        .cancel(
            &PaymentId::issued("12345678"),
            Some(&Reason::new(ReasonCode::Fraud)),
        )
        .await
        .expect("the cancel goes through");
    assert_eq!(reversal.amount.minor_units(), 14_990);
}

#[tokio::test]
async fn refunding_one_line_carries_the_reason_too() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/refund"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "txn-9",
            "paymentTransactionId": "txn-9",
            "price": "20.00",
            "currency": "TRY",
            "reason": "DOUBLE_PAYMENT",
            "description": "charged twice by the till",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5092",
            "errorMessage": "Islem bulunamadi",
            "retryable": false,
        })))
        .mount(&server)
        .await;

    let reason = Reason::new(ReasonCode::DoublePayment).describe("charged twice by the till");
    let error = client(&server)
        .refund_transaction(
            "txn-9",
            Money::parse("20.00", Currency::Try).expect("valid"),
            Some(&reason),
        )
        .await
        .expect_err("no such transaction");
    assert_eq!(error.code(), Some("5092"));
}

#[test]
fn every_reason_renders_the_word_iyzico_documents() {
    assert_eq!(ReasonCode::Other.to_string(), "OTHER");
    assert_eq!(ReasonCode::Fraud.to_string(), "FRAUD");
    assert_eq!(ReasonCode::BuyerRequest.to_string(), "BUYER_REQUEST");
    assert_eq!(ReasonCode::DoublePayment.to_string(), "DOUBLE_PAYMENT");
    assert_eq!(Reason::from(ReasonCode::Other).code(), ReasonCode::Other);
}

#[tokio::test]
async fn the_classic_client_refuses_to_start_a_payment_through_the_trait() {
    let server = MockServer::start().await;
    // No mock: a request reaching the network would fail the test.
    let request = kasapay_core::ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
    )
    .build()
    .expect("valid request");

    let error = client(&server)
        .charge(&request)
        .await
        .expect_err("the hosted form needs more than a ChargeRequest carries");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("start_checkout_form"));
}

#[tokio::test]
async fn a_payment_is_read_back_by_its_id_and_its_signature_checked() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/detail"))
        .and(body_json(json!({ "locale": "tr", "paymentId": "pay-1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "pay-1",
            "currency": "TRY",
            "basketId": "ord-1",
            "conversationId": "conv-1",
            "paidPrice": "149.90",
            "price": "149.90",
            // HMAC-SHA256("pay-1:TRY:ord-1:conv-1:149.9:149.9", "secret-key"): an
            // amount is signed with its trailing zeros gone.
            "signature": "100ab6291038e56be64ac141fcbde4e8aea003a02e6e9d201cceda8a75efb18c",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&PaymentId::issued("pay-1"))
        .await
        .expect("the payment reads back");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.id, Some(PaymentId::issued("pay-1")));
    assert_eq!(
        charge.amount,
        Money::parse("149.90", Currency::Try).expect("valid amount")
    );
    assert_eq!(charge.order.as_ref().map(OrderRef::as_str), Some("ord-1"));
}

/// The signature is over the payment's six fields, not the form's eight.
///
/// Sending the form's own list would verify against the wrong thing, and the
/// two calls answer the same shape — so nothing but the signature would say.
#[tokio::test]
async fn a_payment_signed_as_though_it_were_a_form_is_untrusted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentStatus": "SUCCESS",
            "paymentId": "pay-1",
            "currency": "TRY",
            "basketId": "ord-1",
            "conversationId": "conv-1",
            "paidPrice": "149.90",
            "price": "149.90",
            // HMAC-SHA256("SUCCESS:pay-1:TRY:ord-1:conv-1:149.9:149.9:", "secret-key"),
            // which is the checkout form's eight fields with no token to end them.
            "signature": "6bb878e62a7484d27a6b97eebb12cca3c652eb7adc3a9871c8a2f369016dae43",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("pay-1"))
        .await
        .expect_err("a signature over the wrong fields is not a payment");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}
