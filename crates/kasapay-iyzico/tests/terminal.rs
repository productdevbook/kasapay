//! The Terminal API against a mock server.
//!
//! The response fixtures here are iyzico's own. The authorization, token and
//! sale bodies are the worked examples printed on the Terminal API Integration
//! overview page, field for field, with the values that page prints; the
//! failure bodies are `TerminalFailureResponse` filled with codes and groups
//! off their Terminal API error-codes page. Nothing is invented, because a
//! fixture somebody made up only proves the code agrees with whoever made it
//! up.
//!
//! What is not here is a live account. There is no Terminal API sandbox
//! without a merchant agreement and a Pavo device, so these tests say what
//! this crate sends and reads, not what iyzico does with it.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{Currency, ErrorKind, Money, Secret};
use kasapay_iyzico::terminal::{
    CardType, Client, Config, Credentials, Login, Query, Reference, Refund, Sale, SalesType, Void,
};
use serde_json::json;
use wiremock::matchers::{body_json, body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEVICE: &str = "PAV860047264";
const PAYMENT: &str = "30001";
const TOKEN: &str = "eyJraWQiOiJ0ZXN0.access.token";

fn config(server: &MockServer) -> Config {
    Config::new(&format!("{}/", server.uri())).expect("valid base")
}

fn credentials() -> Credentials {
    Credentials::new("client-id", "client-secret", "kasiyer", "parola")
}

fn reference() -> Reference {
    Reference::new("conversation1", DEVICE, "string30")
}

/// iyzico's own example of what `/authorize` answers.
fn auth_code_response() -> serde_json::Value {
    json!({
        "code": "FYEff_koHQnoX9vIMo5icTrqmSsbIOvD6xz8KAEEjnvj652",
        "issuedAt": "2025-12-25T14:05:49.379055098+03:00",
        "expiredAt": "2025-12-25T14:15:49.379055098+03:00",
    })
}

/// iyzico's own example of what `/token` answers, `expires_in` included.
fn token_response() -> serde_json::Value {
    json!({
        "access_token": TOKEN,
        "refresh_token": "eiHg.refresh.BBp",
        "scope": "iyzipayApiGateway",
        "token_type": "Bearer",
        "expires_in": 7199,
    })
}

/// iyzico's own example of what a completed sale answers, with the amount and
/// the card of the sale these tests send.
fn sale_response() -> serde_json::Value {
    json!({
        "conversationId": "conversation1",
        "locale": "tr",
        "deviceUniqueId": DEVICE,
        "transactionReferenceId": "string30",
        "status": "SUCCESS",
        "errorCode": "",
        "errorMessage": "",
        "errorGroup": "",
        "systemTime": 1_770_000_000_i64,
        "transactionDateTime": "2025-11-20T12:03:05.096Z",
        "authCode": "123456",
        "paymentId": PAYMENT,
        "paymentDate": "20251120",
        "price": 149.90,
        "installment": 0,
        "currency": "TRY",
        "binNumber": "552879",
        "lastFourDigits": "0008",
        "hostReference": "host-1",
        "cardType": "CREDIT_CARD",
        "acquirerId": "acq-1",
        "issuerId": "iss-1",
        "bankMerchantId": "merchant-1",
        "bankTerminalId": "terminal-1",
        "batchNo": "17",
        "stanNo": "000123",
        "posEntryModeCode": "051",
    })
}

/// `TerminalFailureResponse`, as its schema documents it.
fn failure(group: &str, code: &str, message: &str) -> serde_json::Value {
    json!({
        "status": "FAILURE",
        "errorCode": code,
        "errorMessage": message,
        "errorGroup": group,
        "systemTime": 1_770_000_000_i64,
        "consumerErrorMessage": "İşlem tamamlanamadı",
    })
}

/// Only `/authorize`. A second mock on `/token` would shadow the one a test
/// is asserting against, and the assertions would never run.
async fn mount_authorize(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/authorize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(auth_code_response()))
        .mount(server)
        .await;
}

#[tokio::test]
async fn authorizing_sends_the_seven_documented_fields_as_a_form() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/authorize"))
        .and(header("content-type", "application/x-www-form-urlencoded"))
        .and(body_string_contains("scope=iyzipayApiGateway"))
        .and(body_string_contains("response_type=code"))
        .and(body_string_contains("client_id=client-id"))
        .and(body_string_contains("client_secret=client-secret"))
        .and(body_string_contains("username=kasiyer"))
        .and(body_string_contains("password=parola"))
        .and(body_string_contains("request_timestamp="))
        .respond_with(ResponseTemplate::new(200).set_body_json(auth_code_response()))
        .mount(&server)
        .await;

    let code = Login::new(config(&server), credentials())
        .expect("client builds")
        .authorize()
        .await
        .expect("iyzico issues an auth code");

    assert!(code.code.expose().starts_with("FYEff_koHQ"));
    // ISO-8601 with a nanosecond fraction and an offset, kept as iyzico wrote it.
    assert_eq!(
        code.expired_at.as_deref(),
        Some("2025-12-25T14:15:49.379055098+03:00")
    );
}

#[tokio::test]
async fn the_token_call_carries_the_client_pair_as_basic_auth() {
    let server = MockServer::start().await;
    let expected = format!("Basic {}", BASE64.encode("client-id:client-secret"));
    mount_authorize(&server).await;
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/token"))
        .and(header("authorization", expected.as_str()))
        .and(body_string_contains("grant_type=authorization_code"))
        .and(body_string_contains("code=FYEff_koHQ"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response()))
        .mount(&server)
        .await;

    let token = Login::new(config(&server), credentials())
        .expect("client builds")
        .log_in()
        .await
        .expect("iyzico issues a token");

    assert_eq!(token.access_token.expose(), TOKEN);
    assert_eq!(token.expires_in, Some(Duration::from_secs(7199)));
    assert_eq!(token.token_type.as_deref(), Some("Bearer"));
    assert!(!token.is_expired());
    // Two hours from now a two-hour token is gone, which is what a caller
    // renewing ahead of a sale has to be able to see.
    assert!(token.expires_within(Duration::from_secs(7200)));
    assert!(token.refresh_token.is_some());
}

#[tokio::test]
async fn renewing_uses_the_dedicated_refresh_address() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/token/refresh"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=eiHg.refresh.BBp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(token_response()))
        .mount(&server)
        .await;

    let token = Login::new(config(&server), credentials())
        .expect("client builds")
        .refresh(&Secret::new("eiHg.refresh.BBp"))
        .await
        .expect("iyzico issues a fresh token");

    assert_eq!(token.access_token.expose(), TOKEN);
}

#[tokio::test]
async fn a_login_iyzico_refuses_is_an_auth_failure_and_not_a_bad_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/authorize"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "errorCode": "1000",
            "description": "invalid credentials",
        })))
        .mount(&server)
        .await;

    let error = Login::new(config(&server), credentials())
        .expect("client builds")
        .log_in()
        .await
        .expect_err("iyzico refuses the login");

    assert_eq!(error.kind(), ErrorKind::Auth);
    assert_eq!(error.code(), Some("1000"));
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn a_sale_sends_the_documented_body_and_reads_the_answer_back() {
    let server = MockServer::start().await;
    let bearer = format!("Bearer {TOKEN}");
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .and(header("authorization", bearer.as_str()))
        .and(body_json(json!({
            "conversationId": "conversation1",
            // Lowercase, per the fragment's enum rather than the "TR" of
            // iyzico's own sample.
            "locale": "tr",
            "deviceUniqueId": DEVICE,
            "transactionReferenceId": "string30",
            "price": 149.90,
            "currency": "TRY",
            // salesType, not the saleType of the worked example.
            "salesType": "SALE",
            "installment": 0,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let sale = Sale::builder(
        reference(),
        Money::parse("149.90", Currency::Try).expect("a valid amount"),
    )
    .build()
    .expect("a sale iyzico documents");

    let payment = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&sale)
        .await
        .expect("the terminal takes the payment");

    assert_eq!(payment.payment_id.as_deref(), Some(PAYMENT));
    assert_eq!(payment.payment_date.as_deref(), Some("20251120"));
    assert_eq!(
        payment.amount,
        Some(Money::parse("149.90", Currency::Try).expect("a valid amount"))
    );
    assert_eq!(payment.card_type, Some(CardType::Credit));
    assert_eq!(payment.last_four_digits.as_deref(), Some("0008"));
    assert_eq!(payment.installments, Some(0));
    // Everything this crate does not model is still readable.
    assert_eq!(
        payment.raw.text_at("/posEntryModeCode").as_deref(),
        Some("051")
    );
}

#[tokio::test]
async fn an_amount_goes_out_as_the_decimal_it_is() {
    let server = MockServer::start().await;
    // 10.10 is not representable in binary floating point. Sending it through
    // an f64 is how it leaves as 10.099999999999999.
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .and(body_string_contains("\"price\":10.10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let sale = Sale::builder(
        reference(),
        Money::parse("10.10", Currency::Try).expect("a valid amount"),
    )
    .build()
    .expect("a sale iyzico documents");

    Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&sale)
        .await
        .expect("the terminal takes the payment");
}

#[tokio::test]
async fn a_provision_closing_sale_names_the_provision_it_closes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .and(body_json(json!({
            "conversationId": "conversation1",
            "locale": "tr",
            "deviceUniqueId": DEVICE,
            "transactionReferenceId": "string30",
            "price": 149.90,
            "currency": "TRY",
            "salesType": "POST_AUTH",
            "paymentId": PAYMENT,
            "installment": 0,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let sale = Sale::builder(
        reference(),
        Money::parse("149.90", Currency::Try).expect("a valid amount"),
    )
    .sales_type(SalesType::PostAuth)
    .payment_id(PAYMENT)
    .build()
    .expect("a post-auth that names its provision");

    Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&sale)
        .await
        .expect("the provision closes");
}

#[tokio::test]
async fn a_query_by_payment_alone_leaves_the_other_two_fields_out() {
    let server = MockServer::start().await;
    // The schema marks deviceUniqueId and transactionReferenceId required and
    // the note beside it says otherwise. body_json is exact, so an extra field
    // fails this.
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment/query-transaction-status"))
        .and(body_json(json!({
            "conversationId": "conversation1",
            "locale": "tr",
            "paymentId": PAYMENT,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let payment = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .payment(&Query::payment("conversation1", PAYMENT))
        .await
        .expect("iyzico answers the query");

    assert_eq!(payment.payment_id.as_deref(), Some(PAYMENT));
}

#[tokio::test]
async fn a_query_by_transaction_carries_the_terminal_and_not_the_payment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment/query-transaction-status"))
        .and(body_json(json!({
            "conversationId": "conversation1",
            "locale": "tr",
            "deviceUniqueId": DEVICE,
            "transactionReferenceId": "string30",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    Client::new(config(&server), TOKEN)
        .expect("client builds")
        .payment(&Query::transaction("conversation1", DEVICE, "string30"))
        .await
        .expect("iyzico answers the query");
}

#[tokio::test]
async fn a_refund_carries_the_amount_and_the_day_the_payment_was_posted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment/refund"))
        .and(body_json(json!({
            "conversationId": "conversation1",
            "locale": "tr",
            "paymentId": PAYMENT,
            "deviceUniqueId": DEVICE,
            "price": 50.00,
            "transactionReferenceId": "string30",
            "paymentDate": "20251120",
            "reason": "musteri vazgecti",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let refund = Refund::builder(
        reference(),
        PAYMENT,
        "20251120",
        Money::parse("50.00", Currency::Try).expect("a valid amount"),
    )
    .reason("musteri vazgecti")
    .build()
    .expect("a refund iyzico documents");

    Client::new(config(&server), TOKEN)
        .expect("client builds")
        .refund(&refund)
        .await
        .expect("the terminal gives the money back");
}

#[tokio::test]
async fn a_void_refused_with_422_is_read_from_its_body_and_not_its_status() {
    let server = MockServer::start().await;
    // 422 is what iyzico documents for a void and 400 for a refund, for the
    // same failure shape.
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment/void"))
        .respond_with(ResponseTemplate::new(422).set_body_json(failure(
            "BUSINESS_ERROR",
            "380107",
            "Payment not found with paymentId: 30001",
        )))
        .mount(&server)
        .await;

    let void = Void::builder(reference(), PAYMENT, "20251120")
        .build()
        .expect("a void iyzico documents");

    let error = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .void(&void)
        .await
        .expect_err("iyzico refuses the void");

    // The status alone would have said InvalidRequest; the body says the
    // payment is not there.
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert_eq!(error.code(), Some("380107"));
}

#[tokio::test]
async fn an_expired_token_is_an_auth_failure_and_never_renewed_behind_the_caller() {
    let server = MockServer::start().await;
    // A refusal arriving with HTTP 200 and status FAILURE, which is the shape
    // the success schema allows.
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(failure(
            "SYSTEM_ERROR",
            "100311",
            "Token kullanım süresi dolmuştur!",
        )))
        .mount(&server)
        .await;

    let sale = Sale::builder(
        reference(),
        Money::parse("149.90", Currency::Try).expect("a valid amount"),
    )
    .build()
    .expect("a sale iyzico documents");

    let error = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&sale)
        .await
        .expect_err("iyzico refuses the sale");

    assert_eq!(error.kind(), ErrorKind::Auth);
    assert_eq!(error.code(), Some("100311"));
    // Retrying a sale on a dead token cannot succeed, and this client does not
    // fetch another and try again — see the module documentation.
    assert!(!error.is_retryable());
    // One request was made, not two.
    assert_eq!(server.received_requests().await.expect("recorded").len(), 1);
}

#[tokio::test]
async fn a_busy_terminal_is_worth_trying_again_and_a_bank_saying_no_is_not() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .respond_with(ResponseTemplate::new(400).set_body_json(failure(
            "DEVICE_ERROR",
            "380201",
            "Terminal şu anda meşgul bir önceki işlemin bitmesini bekleyin",
        )))
        .mount(&server)
        .await;
    let busy = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&a_sale())
        .await
        .expect_err("the terminal is busy");
    assert_eq!(busy.kind(), ErrorKind::Provider);
    assert!(busy.is_retryable());

    let other = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .respond_with(ResponseTemplate::new(400).set_body_json(failure(
            "BANK_ERROR",
            "51",
            "Yetersiz bakiye",
        )))
        .mount(&other)
        .await;
    let declined = Client::new(config(&other), TOKEN)
        .expect("client builds")
        .pay(&a_sale())
        .await
        .expect_err("the bank says no");
    assert_eq!(declined.kind(), ErrorKind::Declined);
    assert!(!declined.is_retryable());
}

#[tokio::test]
async fn an_answer_that_does_not_say_whether_it_worked_is_not_read_as_a_payment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "conversationId": "conversation1",
            "paymentId": PAYMENT,
            "price": 149.90,
            "currency": "TRY",
        })))
        .mount(&server)
        .await;

    let error = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .pay(&a_sale())
        .await
        .expect_err("a payment with no status is not a payment");

    assert_eq!(error.kind(), ErrorKind::Malformed);
}

#[tokio::test]
async fn a_fresher_token_reaches_a_client_that_is_already_built() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/payment"))
        .and(header("authorization", "Bearer second.token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(sale_response()))
        .mount(&server)
        .await;

    let till = Client::new(config(&server), "first.token").expect("client builds");
    // Every clone shares the token, so a task renewing it and a till spending
    // it can be the same client.
    let renewer = till.clone();
    renewer.set_access_token(Secret::new("second.token"));

    till.pay(&a_sale())
        .await
        .expect("the fresher token is the one sent");
}

fn a_sale() -> Sale {
    Sale::builder(
        reference(),
        Money::parse("149.90", Currency::Try).expect("a valid amount"),
    )
    .build()
    .expect("a sale iyzico documents")
}
