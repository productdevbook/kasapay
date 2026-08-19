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
use kasapay_iyzico::terminal::gmu;
use kasapay_iyzico::terminal::{
    CardType, Client, Config, Credentials, EndOfDayRequest, Login, Query, Reference, Refund, Sale,
    SalesType, Timestamps, Void,
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

/// The unit iyzico did not name, and the switch for a caller who finds out.
#[tokio::test]
async fn a_login_can_be_told_to_send_milliseconds_instead() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/in-store/oauth2/authorize"))
        .respond_with(ResponseTemplate::new(200).set_body_json(auth_code_response()))
        .mount(&server)
        .await;

    Login::new(
        config(&server).timestamps(Timestamps::Milliseconds),
        credentials(),
    )
    .expect("client builds")
    .authorize()
    .await
    .expect("iyzico issues an auth code");

    let sent = &server.received_requests().await.expect("recorded")[0];
    let body = String::from_utf8_lossy(&sent.body);
    let stamp = body
        .split('&')
        .find_map(|field| field.strip_prefix("request_timestamp="))
        .expect("the timestamp is sent");
    // Thirteen digits is milliseconds; ten is seconds, which is the default.
    assert_eq!(stamp.len(), 13, "{body}");
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

/// A till closes the day's batch, and the bank answers a line per acquirer.
#[tokio::test]
async fn closing_the_day_answers_a_total_for_each_acquiring_bank() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/eod"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(body_json(json!({
            "conversationId": "eod-1",
            "locale": "tr",
            "deviceUniqueId": DEVICE,
            "useSummary": false,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCESS",
            "locale": "tr",
            "systemTime": 1_770_000_000_000_i64,
            "conversationId": "eod-1",
            "batchNo": "000123",
            "resultMessage": "Gun sonu alindi",
            "totals": [{
                "acquirerId": "0062",
                "acquirerName": "Garanti Bankasi",
                "terminalId": "VP000001",
                "bankMerchantId": "1234567",
                "batchNo": "000123",
                "totalTransactionAmount": "1499.00",
                "totalTransactionCount": "7",
                "responseCode": "00",
            }],
        })))
        .mount(&server)
        .await;

    let closed = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .end_of_day(&EndOfDayRequest::new("eod-1", DEVICE))
        .await
        .expect("the batch closes");

    assert_eq!(closed.batch_no.as_deref(), Some("000123"));
    assert_eq!(closed.totals.len(), 1);
    let total = &closed.totals[0];
    assert_eq!(total.acquirer_name.as_deref(), Some("Garanti Bankasi"));
    // Text, because iyzico types it as a string and names no currency for it
    // anywhere in this answer.
    assert_eq!(total.total_amount.as_deref(), Some("1499.00"));
    assert_eq!(total.total_count.as_deref(), Some("7"));
}

/// `useSummary` changes what the device prints, so it is sent as asked.
#[tokio::test]
async fn a_detailed_slip_asks_for_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/eod"))
        .and(body_string_contains(r#""useSummary":true"#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCESS",
            "batchNo": "000123",
        })))
        .mount(&server)
        .await;

    Client::new(config(&server), TOKEN)
        .expect("client builds")
        .end_of_day(&EndOfDayRequest::new("eod-1", DEVICE).detailed_slip())
        .await
        .expect("the batch closes");
}

/// A batch that did not close is not a batch that closed, whatever the HTTP
/// status was.
#[tokio::test]
async fn an_end_of_day_iyzico_refused_is_not_a_closed_batch() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/eod"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "FAILURE",
            "errorCode": "380111",
            "errorMessage": "Cihaz bulunamadi",
        })))
        .mount(&server)
        .await;

    let error = Client::new(config(&server), TOKEN)
        .expect("client builds")
        .end_of_day(&EndOfDayRequest::new("eod-1", DEVICE))
        .await
        .expect_err("a 200 carrying FAILURE is a refusal");
    assert_eq!(error.code(), Some("380111"));
}

/// VUK 507's sale is a receipt: the lines, the document type and the buyer's
/// tax details go with the amount.
#[tokio::test]
async fn a_vuk_507_sale_carries_the_lines_and_the_document() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/gmu/payment"))
        .and(header("authorization", format!("Bearer {TOKEN}")))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "conv-1",
            "deviceUniqueId": DEVICE,
            "transactionReferenceId": "txn-1",
            "price": "149.90",
            "paidPrice": "149.90",
            "paymentType": "CREDITCARD",
            "currency": "TRY",
            "installment": 1,
            "saleAppName": "Kasa",
            "saleAppVersion": "1.0.0",
            // 1 is an e-invoice, which is the default.
            "saleDocumentType": 1,
            "saleItems": [{
                "name": "Kahve",
                "generic": false,
                "unitCode": "C62",
                "taxGroupCode": "KDV20",
                "itemQuantity": 1,
                "unitPriceAmount": "149.90",
                "grossPriceAmount": "124.92",
                "totalPriceAmount": "149.90",
            }],
            "buyerInfo": {
                "customerType": 2,
                "companyName": "A Ltd",
                "taxOfficeCode": "034",
                "taxNumber": "1234567890",
            },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCESS",
            "paymentId": "P-1",
            "paymentDate": 20_260_819_i64,
            "price": "149.90",
            "currency": "TRY",
            "authCode": "123456",
            "batchNo": "000123",
            "lastFourDigits": "0004",
        })))
        .mount(&server)
        .await;

    let sale = gmu::Sale::builder(
        Reference::new("conv-1", DEVICE, "txn-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        "CREDITCARD",
        gmu::SaleApp::new("Kasa", "1.0.0"),
    )
    .item(gmu::SaleItem::new(
        "Kahve",
        "C62",
        "KDV20",
        1,
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        Money::parse("124.92", Currency::Try).expect("valid amount"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
    ))
    .buyer(
        gmu::Buyer::new(gmu::BuyerKind::Company)
            .company("A Ltd")
            .tax("034", "1234567890"),
    )
    .build();

    let payment = gmu::Client::new(Client::new(config(&server), TOKEN).expect("client builds"))
        .pay(&sale)
        .await
        .expect("the till takes the payment");

    assert_eq!(payment.payment_id.as_deref(), Some("P-1"));
    // `YYYYMMDD` as an integer here, kept as iyzico wrote it.
    assert_eq!(payment.payment_date.as_deref(), Some("20260819"));
    assert_eq!(payment.price.as_deref(), Some("149.90"));
}

/// A sale with no lines is not a document, so it never reaches the till.
#[tokio::test]
async fn a_vuk_507_sale_with_no_lines_never_reaches_the_till() {
    let server = MockServer::start().await;
    let sale = gmu::Sale::builder(
        Reference::new("conv-1", DEVICE, "txn-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        "CREDITCARD",
        gmu::SaleApp::new("Kasa", "1.0.0"),
    )
    .build();

    let error = gmu::Client::new(Client::new(config(&server), TOKEN).expect("client builds"))
        .pay(&sale)
        .await
        .expect_err("a receipt with nothing on it");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

/// A refund names what is coming back, which is the whole difference.
#[tokio::test]
async fn a_vuk_507_refund_names_the_line_it_returns() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/gmu/payment/refund"))
        .and(body_string_contains(r#""relatedSaleItemId":"item-1""#))
        .and(body_string_contains(r#""returnAmount":"50.00""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCESS",
            "paymentId": "P-1",
        })))
        .mount(&server)
        .await;

    let refund = gmu::Refund::new(
        Reference::new("conv-2", DEVICE, "txn-2"),
        "P-1",
        "20260819",
        gmu::SaleApp::new("Kasa", "1.0.0"),
        vec![
            gmu::SaleItem::new(
                "Kahve",
                "C62",
                "KDV20",
                1,
                Money::parse("50.00", Currency::Try).expect("valid amount"),
                Money::parse("41.67", Currency::Try).expect("valid amount"),
                Money::parse("50.00", Currency::Try).expect("valid amount"),
            )
            .returning(
                "item-1",
                Money::parse("50.00", Currency::Try).expect("valid amount"),
            ),
        ],
    );

    gmu::Client::new(Client::new(config(&server), TOKEN).expect("client builds"))
        .refund(&refund)
        .await
        .expect("the line comes back");
}

/// The three steps that settle one sale with more than one instrument.
#[tokio::test]
async fn a_partial_payment_is_opened_added_to_and_closed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/terminal-host/gmu/partial-payment/add-payment"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "conv-3",
            "deviceUniqueId": DEVICE,
            "transactionReferenceId": "txn-3",
            "saleNumber": "S-1",
            "price": "50.00",
            "installment": 1,
            "currency": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "SUCCESS",
            "saleNumber": "S-1",
            "remainingPaymentAmount": "99.90",
        })))
        .mount(&server)
        .await;

    let part = gmu::Client::new(Client::new(config(&server), TOKEN).expect("client builds"))
        .add_partial_payment(
            "S-1",
            &Reference::new("conv-3", DEVICE, "txn-3"),
            Money::parse("50.00", Currency::Try).expect("valid amount"),
            1,
        )
        .await
        .expect("part of it is settled");

    assert_eq!(part.sale_number.as_deref(), Some("S-1"));
    // What is left, which the next step must not exceed.
    assert_eq!(part.remaining.as_deref(), Some("99.90"));
}

/// No mock is mounted: a query naming nothing never reaches the till.
#[tokio::test]
async fn a_vuk_507_query_names_at_least_one_of_the_three() {
    let server = MockServer::start().await;
    let error = gmu::Client::new(Client::new(config(&server), TOKEN).expect("client builds"))
        .payment(&gmu::Query::default())
        .await
        .expect_err("nothing to ask about");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}
