//! The In-Store flow against a mock server standing in for iyzico.
#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use std::time::Duration;

use kasapay_core::{
    Charge, ChargeRequest, Currency, ErrorKind, IdempotencyKey, Money, OrderRef, PaymentId,
    Provider, Status,
};
use kasapay_iyzico::in_store::{Client, Config};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    configured(server, Config::DEFAULT_TIMEOUT)
}

fn configured(server: &MockServer, timeout: Duration) -> Client {
    let base = format!("{}/v3/in-store/", server.uri());
    let config = Config::new(&base, "api-key", "secret-key", "merchant-id")
        .expect("valid base")
        .timeout(timeout);
    Client::new(config).expect("client builds")
}

fn charge_request() -> ChargeRequest {
    ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
    )
    .customer("kasiyer-7")
    .return_url("https://merchant.test/callback".parse().expect("valid url"))
    .build()
    .expect("valid request")
}

#[tokio::test]
async fn init_sends_the_documented_body_and_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/init"))
        .and(header("x-api-key", "api-key"))
        .and(header("x-secret-key", "secret-key"))
        .and(header("x-merchant-id", "merchant-id"))
        .and(header("x-callback-url", "https://merchant.test/callback"))
        // 149.90 as a bare JSON number, not a string and not 149.90000000000001.
        .and(body_json(json!({
            "userId": "kasiyer-7",
            "orderId": "ord-1",
            "amount": 149.90,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "errorCode": null,
            "errorMessage": null,
            "deepLinkUrl": "https://iyzi.link/session-abc",
            "paymentSessionToken": "tok-abc",
            "paymentId": 4_242_424_242_i64,
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge(&charge_request())
        .await
        .expect("init succeeds");

    assert_eq!(charge.id.as_str(), "4242424242");
    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(charge.amount.minor_units(), 14_990);
    match charge.next_action.expect("a deep link is required") {
        kasapay_core::NextAction::Redirect { url, continuation } => {
            assert_eq!(url.as_str(), "https://iyzi.link/session-abc");
            assert_eq!(continuation.as_deref(), Some("tok-abc"));
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
}

#[tokio::test]
async fn a_failure_status_becomes_a_decline_carrying_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5001",
            "errorMessage": "islem yapilamadi",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge(&charge_request())
        .await
        .expect_err("a failure status is not a charge");

    assert_eq!(error.kind(), ErrorKind::Declined);
    assert_eq!(error.code(), Some("5001"));
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn an_unauthorized_response_is_an_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/init"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "status": "failure",
            "errorCode": "1001",
            "errorMessage": "gecersiz header",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge(&charge_request())
        .await
        .expect_err("401 is not a charge");

    assert_eq!(error.kind(), ErrorKind::Auth);
    assert_eq!(error.code(), Some("1001"));
}

#[tokio::test]
async fn query_reads_an_approved_payment_as_captured() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3/in-store/payment/query"))
        .and(query_param("paymentId", "4242424242"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentId": 4_242_424_242_i64,
            "orderId": "ord-1",
            "userId": "kasiyer-7",
            "transactionDetail": {
                "amount": 149.90,
                "currencyCode": "TRY",
                "isRefundable": true,
                "receipt": { "approved": true },
            },
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&PaymentId::new("4242424242"))
        .await
        .expect("query succeeds");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert_eq!(
        charge.order.map(|o| o.to_string()),
        Some("ord-1".to_owned())
    );
    assert_eq!(
        charge
            .raw
            .text_at("/transactionDetail/currencyCode")
            .as_deref(),
        Some("TRY")
    );
}

#[tokio::test]
async fn a_currency_the_in_store_api_cannot_settle_never_reaches_the_network() {
    let server = MockServer::start().await;
    // No mock is mounted: any request at all would fail the test.
    let request = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("19.99", Currency::Usd).expect("valid amount"),
    )
    .customer("kasiyer-7")
    .return_url("https://merchant.test/callback".parse().expect("valid url"))
    .build()
    .expect("valid request");

    let error = client(&server)
        .charge(&request)
        .await
        .expect_err("USD is not settleable in-store");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn a_charge_without_a_callback_url_is_refused_before_sending() {
    let server = MockServer::start().await;
    let request = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("10.00", Currency::Try).expect("valid amount"),
    )
    .customer("kasiyer-7")
    .build()
    .expect("valid request");

    let error = client(&server)
        .charge(&request)
        .await
        .expect_err("the callback URL is required");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

#[tokio::test]
async fn an_idempotency_key_is_refused_rather_than_dropped() {
    let server = MockServer::start().await;
    // No mock is mounted: a request reaching the network would fail the test.
    let request = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("10.00", Currency::Try).expect("valid amount"),
    )
    .customer("kasiyer-7")
    .return_url("https://merchant.test/callback".parse().expect("valid url"))
    .idempotency_key(IdempotencyKey::new("retry-1"))
    .build()
    .expect("valid request");

    let error = client(&server)
        .charge(&request)
        .await
        .expect_err("a key this API cannot honour is not silently dropped");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn a_provider_that_never_answers_gives_up_rather_than_hanging() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/init"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&server)
        .await;

    let error = configured(&server, Duration::from_millis(150))
        .charge(&charge_request())
        .await
        .expect_err("a request past its timeout is not a charge");

    assert_eq!(error.kind(), ErrorKind::Transport);
    assert!(error.is_retryable());
}

fn decrypted(approved: bool, refund_approved: bool, void_approved: bool) -> serde_json::Value {
    json!({
        "status": "success",
        "systemTime": 1_770_000_000_i64,
        "inStoreCompleteOperation": {
            "transaction": {
                "transactionDate": "2026-08-14 12:00:00",
                "rrn": "622812345678",
                "amount": 149.90,
                "currencyCode": "TRY",
                "maskedPan": "552879******0004",
                "receipt": {
                    "approved": approved,
                    "refundApproved": refund_approved,
                    "voidApproved": void_approved,
                    "schemaName": "MASTERCARD",
                },
            }
        }
    })
}

async fn decrypt(
    server: &MockServer,
    body: serde_json::Value,
) -> Result<Charge, kasapay_core::Error> {
    Mock::given(method("POST"))
        .and(path("/v3/in-store/crypt/decrypt"))
        .and(header("x-api-key", "api-key"))
        .and(body_json(json!({
            "data": "ZW5jcnlwdGVk",
            "paymentSessionToken": "tok-abc",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;

    client(server)
        .decrypt_callback(&PaymentId::new("4242424242"), "ZW5jcnlwdGVk", "tok-abc")
        .await
}

#[tokio::test]
async fn a_decrypted_callback_settles_the_payment_it_was_started_for() {
    let server = MockServer::start().await;
    let charge = decrypt(&server, decrypted(true, false, false))
        .await
        .expect("the callback decrypts");

    // The decrypted body carries a recordId, not a paymentId, so the id has to
    // be the one the charge was started with.
    assert_eq!(charge.id.as_str(), "4242424242");
    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert!(charge.next_action.is_none());
    assert_eq!(
        charge
            .raw
            .text_at("/inStoreCompleteOperation/transaction/rrn")
            .as_deref(),
        Some("622812345678")
    );
}

#[tokio::test]
async fn an_approved_void_reads_as_cancelled() {
    let server = MockServer::start().await;
    let charge = decrypt(&server, decrypted(false, false, true))
        .await
        .expect("the callback decrypts");
    assert_eq!(charge.status, Status::Canceled);
    assert!(!charge.status.is_open());
}

#[tokio::test]
async fn a_callback_approving_nothing_is_a_failure() {
    let server = MockServer::start().await;
    let charge = decrypt(&server, decrypted(false, false, false))
        .await
        .expect("the callback decrypts");
    assert_eq!(charge.status, Status::Failed);
}

#[tokio::test]
async fn a_failure_status_on_decrypt_is_an_error_not_a_charge() {
    let server = MockServer::start().await;
    let error = decrypt(
        &server,
        json!({
            "status": "failure",
            "errorCode": "6001",
            "errorMessage": "gecersiz paymentSessionToken",
        }),
    )
    .await
    .expect_err("a refused decrypt is not a settled payment");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("6001"));
}

#[tokio::test]
async fn a_bank_timeout_is_retryable_and_a_declined_card_is_not() {
    // Both are iyzico's own codes and their own messages.
    for (code, message, retryable) in [
        (
            "10219",
            "Banka tarafinda hata olustu, lutfen tekrar deneyin.",
            true,
        ),
        (
            "10214",
            "Banka tarafinda hata olustu, lutfen tekrar deneyin.",
            true,
        ),
        ("10051", "Kart limiti yetersiz, bakiye yetersiz.", false),
        (
            "10209",
            "Kartiniz bloke, lutfen bankanizla iletisime gecin.",
            false,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/in-store/payment/init"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "failure",
                "errorCode": code,
                "errorMessage": message,
            })))
            .mount(&server)
            .await;

        let error = client(&server)
            .charge(&charge_request())
            .await
            .expect_err("a failure status is not a charge");
        assert_eq!(error.code(), Some(code));
        assert_eq!(
            error.is_retryable(),
            retryable,
            "{code}: {}",
            if retryable {
                "the bank asked us to come back"
            } else {
                "the bank said no"
            }
        );
    }
}
