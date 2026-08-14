//! The In-Store flow against a mock server standing in for iyzico.
#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{ChargeRequest, Currency, ErrorKind, Money, OrderRef, PaymentId, Provider};
use kasapay_iyzico::{Config, Iyzico};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client(server: &MockServer) -> Iyzico {
    let base = format!("{}/v3/in-store/", server.uri());
    let config = Config::new(&base, "api-key", "secret-key", "merchant-id").expect("valid base");
    Iyzico::new(config).expect("client builds")
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
        .await
        .charge(&charge_request())
        .await
        .expect("init succeeds");

    assert_eq!(charge.id.as_str(), "4242424242");
    assert_eq!(charge.status, kasapay_core::Status::RequiresAction);
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
        .await
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
        .await
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
        .await
        .charge_status(&PaymentId::new("4242424242"))
        .await
        .expect("query succeeds");

    assert_eq!(charge.status, kasapay_core::Status::Captured);
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert_eq!(
        charge.order.map(|o| o.to_string()),
        Some("ord-1".to_owned())
    );
    assert_eq!(charge.raw["transactionDetail"]["currencyCode"], "TRY");
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
        .await
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
        .await
        .charge(&request)
        .await
        .expect_err("the callback URL is required");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}
