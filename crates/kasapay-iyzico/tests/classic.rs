//! The classic API against a mock server, and the signing it carries.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{Currency, ErrorKind, Money, NextAction, OrderRef, PaymentId, Status};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic::{Association, CardType, Client, Config, checkout};
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
        .checkout_result("cf-token-1")
        .await
        .expect("the form reads back");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.id.as_str(), "12345678");
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
        .checkout_result("cf-token-1")
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
        .checkout_result("cf-token-1")
        .await
        .expect("the query worked");
    assert_eq!(charge.status, Status::Pending);
    assert!(charge.status.is_open());
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
        .checkout_result("cf-token-1")
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
        .checkout_result("cf-token-1")
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
            &PaymentId::new("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
        )
        .await
        .expect("the refund goes through");

    assert_eq!(reversal.payment.as_str(), "12345678");
    assert_eq!(reversal.amount.minor_units(), 5000);
    assert_eq!(reversal.host_reference.as_deref(), Some("host-ref-1"));
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
            &PaymentId::new("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
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
            &PaymentId::new("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
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
            &PaymentId::new("12345678"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
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
        .cancel(&PaymentId::new("12345678"))
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
        )
        .await
        .expect_err("no such transaction");
    assert_eq!(error.code(), Some("5092"));
}

#[tokio::test]
async fn the_classic_client_answers_charge_status_through_the_shared_trait() {
    use kasapay_core::Provider as _;

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
            "signature": "b929da899af8c2c2bc4de9cc44791977115a937c4ea712fa9256ef34a35fa946",
        })))
        .mount(&server)
        .await;

    // A caller holding Arc<dyn Provider> can read a classic payment back.
    let provider: Box<dyn kasapay_core::Provider> = Box::new(client(&server));
    // The id is the form's token, which is what start_checkout_form put there.
    let charge = provider
        .charge_status(&PaymentId::new("cf-token-1"))
        .await
        .expect("the payment reads back");
    assert_eq!(charge.status, Status::Captured);
    assert_eq!(provider.id(), kasapay_core::ProviderId::IYZICO);
}

#[tokio::test]
async fn the_classic_client_refuses_to_start_a_payment_through_the_trait() {
    use kasapay_core::Provider as _;

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
