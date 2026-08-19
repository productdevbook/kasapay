//! The In-Store flow against a mock server standing in for iyzico.
#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use std::time::Duration;

use kasapay_core::{
    Charge, ChargeRequest, Currency, ErrorKind, IdempotencyKey, Money, NextAction, OrderRef,
    PaymentId, Provider, RefundRequest, RefundStatus, Status,
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

    assert_eq!(charge.id, Some(PaymentId::issued("4242424242")));
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
        .charge_status(&PaymentId::issued("4242424242"))
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
    // "0949" is what iyzico's own published response carries: ISO 4217's
    // numeric code for lira, not "TRY".
    json!({
        "status": "success",
        "systemTime": 1_770_000_000_i64,
        "inStoreCompleteOperation": {
            "transaction": {
                "transactionDate": "2026-08-14 12:00:00",
                "rrn": "622812345678",
                "amount": 149.90,
                "currencyCode": "0949",
                "maskedPan": "552879******0004",
                "receipt": {
                    "approved": approved,
                    "refundApproved": refund_approved,
                    "voidApproved": void_approved,
                    "schemaName": "MASTERCARD",
                },
            },
            "paymentFailedResult": null,
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
        .decrypt_callback(&PaymentId::issued("4242424242"), "ZW5jcnlwdGVk", "tok-abc")
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
    assert_eq!(charge.id, Some(PaymentId::issued("4242424242")));
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
async fn a_currency_code_is_read_as_a_number_or_as_letters() {
    let server = MockServer::start().await;
    let charge = decrypt(&server, decrypted(true, false, false))
        .await
        .expect("the callback decrypts");
    assert_eq!(charge.amount.currency(), Currency::Try);

    let mut body = decrypted(true, false, false);
    body["inStoreCompleteOperation"]["transaction"]["currencyCode"] = json!("TRY");
    let other = MockServer::start().await;
    let charge = decrypt(&other, body).await.expect("the callback decrypts");
    assert_eq!(charge.amount.currency(), Currency::Try);
}

#[tokio::test]
async fn a_number_that_names_no_currency_is_not_guessed_at() {
    let server = MockServer::start().await;
    let mut body = decrypted(true, false, false);
    // 999 is ISO 4217's code for "no currency". Nothing may be inferred from it.
    body["inStoreCompleteOperation"]["transaction"]["currencyCode"] = json!("0999");

    let error = decrypt(&server, body)
        .await
        .expect_err("a code naming no currency is not a charge");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// A numeric code that does name a currency is read as that currency.
///
/// This API settles in lira and iyzico publishes no other code for it, so the
/// case is hypothetical — but `0840` is USD unambiguously, and reporting what
/// they said beats refusing it as unrecognised, which is what happened before
/// `Currency` learned the numeric codes.
#[tokio::test]
async fn a_numeric_code_for_another_currency_is_read_as_that_currency() {
    let server = MockServer::start().await;
    let mut body = decrypted(true, false, false);
    body["inStoreCompleteOperation"]["transaction"]["currencyCode"] = json!("0840");

    let charge = decrypt(&server, body).await.expect("the callback decrypts");
    assert_eq!(charge.amount.currency(), Currency::Usd);
}

#[tokio::test]
async fn a_payment_the_payer_did_not_complete_is_a_failed_charge() {
    let server = MockServer::start().await;
    // The fields are those of iyzico's PaymentFailedResult schema, which sits
    // beside `transaction` rather than inside it.
    let charge = decrypt(
        &server,
        json!({
            "status": "success",
            "systemTime": 1_770_000_000_i64,
            "inStoreCompleteOperation": {
                "transaction": null,
                "paymentFailedResult": {
                    "transactionAmount": 149.90,
                    "paymentResultText": "Odeme basarisiz",
                    "screenMessageText": "ISLEM ONAYLANMADI",
                    "date": "2026-08-14 12:00:00",
                },
            }
        }),
    )
    .await
    .expect("a refused payment is still a decrypted callback");

    assert_eq!(charge.status, Status::Failed);
    assert_eq!(charge.amount.minor_units(), 14_990);
    assert_eq!(charge.id, Some(PaymentId::issued("4242424242")));
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

#[tokio::test]
async fn capture_is_refused_rather_than_answered_as_a_no_op() {
    // No mock is mounted: a request that reached iyzico would fail this.
    let server = MockServer::start().await;

    let error = client(&server)
        .capture(&PaymentId::issued("1234567890"), None, None)
        .await
        .expect_err("the In-Store API has no capture step");

    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn cancel_is_refused_because_there_is_no_authorisation_to_release() {
    let server = MockServer::start().await;

    let error = client(&server)
        .cancel(&PaymentId::issued("1234567890"))
        .await
        .expect_err("the In-Store API holds nothing to release");

    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn listing_saved_instruments_is_refused_because_there_is_no_vault() {
    // No mock is mounted: a request that reached iyzico would fail this.
    let server = MockServer::start().await;

    let error = client(&server)
        .instruments("user-1")
        .await
        .expect_err("the In-Store API keeps no vault");

    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn capabilities_match_what_the_methods_actually_do() {
    let server = MockServer::start().await;
    let client = client(&server);
    let capabilities = client.capabilities();

    assert!(!capabilities.separate_capture);
    assert!(!capabilities.partial_capture);
    assert!(capabilities.partial_refund);
    assert!(!capabilities.lookup_by_order);

    // The capability and the refusal have to agree, or the capability is a lie.
    assert_eq!(
        client
            .capture(&PaymentId::issued("1234567890"), None, None)
            .await
            .expect_err("separate_capture is false")
            .kind(),
        ErrorKind::Unsupported
    );
    assert_eq!(
        client
            .lookup(&OrderRef::new("ord-1"))
            .await
            .expect_err("lookup_by_order is false")
            .kind(),
        ErrorKind::Unsupported
    );
}

#[tokio::test]
async fn a_partial_refund_carries_its_amount_under_both_documented_names() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/refund"))
        // iyzico's prose says refundAmount and the schema beside it says
        // refundPrice. The field is optional, so the wrong name alone is not
        // an error — it is a full refund where a part was asked for.
        .and(body_json(json!({
            "userId": "kasiyer-7",
            "paymentId": 4_242_424_242_i64,
            "refundAmount": 50.00,
            "refundPrice": 50.00,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "deepLinkUrl": "https://iyzi.link/refund-abc",
            "paymentSessionToken": "tok-refund",
            "paymentId": 4_242_424_242_i64,
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .refund(
            "kasiyer-7",
            &PaymentId::issued("4242424242"),
            Some(Money::parse("50.00", Currency::Try).expect("valid amount")),
            &"https://merchant.test/callback".parse().expect("valid url"),
        )
        .await
        .expect("the refund starts");
    assert_eq!(charge.status, Status::RequiresAction);
}

#[tokio::test]
async fn a_full_refund_names_no_amount_at_all() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/refund"))
        // Neither name appears: iyzico reads an absent amount as "all of it",
        // and sending a zero would mean something else entirely.
        .and(body_json(json!({
            "userId": "kasiyer-7",
            "paymentId": 4_242_424_242_i64,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "deepLinkUrl": "https://iyzi.link/refund-abc",
            "paymentSessionToken": "tok-refund",
            "paymentId": 4_242_424_242_i64,
        })))
        .mount(&server)
        .await;

    client(&server)
        .refund(
            "kasiyer-7",
            &PaymentId::issued("4242424242"),
            None,
            &"https://merchant.test/callback".parse().expect("valid url"),
        )
        .await
        .expect("the refund starts");
}

/// The shared refund with no amount reads the payment first to find one.
#[tokio::test]
async fn the_shared_refund_reads_the_payment_to_learn_what_a_full_refund_is() {
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
    Mock::given(method("POST"))
        .and(path("/v3/in-store/payment/refund"))
        .and(header("x-callback-url", "https://merchant.test/callback"))
        // The figure the query answered, under both documented names.
        .and(body_json(json!({
            "userId": "kasiyer-7",
            "paymentId": 4_242_424_242_i64,
            "refundAmount": 149.90,
            "refundPrice": 149.90,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "deepLinkUrl": "https://iyzi.link/refund-abc",
            "paymentSessionToken": "tok-refund",
            "paymentId": 4_242_424_242_i64,
        })))
        .mount(&server)
        .await;

    let request = RefundRequest::builder(PaymentId::issued("4242424242"))
        .customer("kasiyer-7")
        .return_url("https://merchant.test/callback".parse().expect("valid url"))
        .build()
        .expect("valid request");
    let refund = Provider::refund(&client(&server), &request)
        .await
        .expect("the refund starts");

    // The money has not moved: the payer approves it in iyzico's app.
    assert_eq!(refund.status, RefundStatus::RequiresAction);
    assert!(refund.status.is_open());
    assert_eq!(refund.amount.minor_units(), 14_990);
    assert!(matches!(
        refund.next_action,
        Some(NextAction::Redirect { .. })
    ));
    assert!(refund.id.is_none());
}

/// Each of these is refused before a socket opens, so no mock is mounted.
#[tokio::test]
async fn the_shared_refund_needs_a_payer_and_somewhere_to_hear_back() {
    let server = MockServer::start().await;
    let client = client(&server);
    let no_customer = RefundRequest::builder(PaymentId::issued("4242424242"))
        .amount(Money::parse("50.00", Currency::Try).expect("valid amount"))
        .return_url("https://merchant.test/callback".parse().expect("valid url"))
        .build()
        .expect("valid request");
    assert_eq!(
        Provider::refund(&client, &no_customer)
            .await
            .expect_err("there is no refund without iyzico's userId")
            .kind(),
        ErrorKind::InvalidRequest
    );

    let no_callback = RefundRequest::builder(PaymentId::issued("4242424242"))
        .amount(Money::parse("50.00", Currency::Try).expect("valid amount"))
        .customer("kasiyer-7")
        .build()
        .expect("valid request");
    assert_eq!(
        Provider::refund(&client, &no_callback)
            .await
            .expect_err("there is no refund with nowhere to hear back")
            .kind(),
        ErrorKind::InvalidRequest
    );

    let with_key = RefundRequest::builder(PaymentId::issued("4242424242"))
        .amount(Money::parse("50.00", Currency::Try).expect("valid amount"))
        .customer("kasiyer-7")
        .return_url("https://merchant.test/callback".parse().expect("valid url"))
        .idempotency_key(IdempotencyKey::new("refund-1"))
        .build()
        .expect("valid request");
    assert_eq!(
        Provider::refund(&client, &with_key)
            .await
            .expect_err("a key iyzico cannot honour is not silently dropped")
            .kind(),
        ErrorKind::Unsupported
    );
}

#[tokio::test]
async fn creating_a_user_answers_the_banks_they_are_enrolled_with() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/user"))
        .and(header("x-api-key", "api-key"))
        .and(body_json(json!({ "userId": "kasiyer-7" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "userId": "kasiyer-7",
            "enrollments": [
                {
                    "enrolledBank": "Garanti Bankasi",
                    "enrolledTerminalId": "TERM-1",
                    "enrollmentStatus": "ACTIVE",
                },
            ],
        })))
        .mount(&server)
        .await;

    let user = client(&server)
        .create_user("kasiyer-7")
        .await
        .expect("the user is registered");

    assert_eq!(&*user.id, "kasiyer-7");
    assert_eq!(user.enrollments.len(), 1);
    assert_eq!(user.enrollments[0].bank.as_deref(), Some("Garanti Bankasi"));
    assert!(user.can_take_payment());
}

#[tokio::test]
async fn a_user_with_no_enrolment_exists_and_cannot_charge() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "userId": "kasiyer-8",
        })))
        .mount(&server)
        .await;

    let user = client(&server)
        .create_user("kasiyer-8")
        .await
        .expect("the user is registered");

    // The distinction the type exists for: registered is not enrolled, and
    // iyzico reports the difference as a failed payment rather than as a bad
    // request.
    assert!(user.enrollments.is_empty());
    assert!(!user.can_take_payment());
}

#[tokio::test]
async fn a_passive_enrolment_does_not_count_as_one_that_can_charge() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "userId": "kasiyer-9",
            "enrollments": [
                { "enrolledBank": "Garanti Bankasi", "enrollmentStatus": "PASSIVE" },
            ],
        })))
        .mount(&server)
        .await;

    let user = client(&server)
        .create_user("kasiyer-9")
        .await
        .expect("the user is registered");
    assert!(!user.can_take_payment());
    // And the raw word is still there for a caller who wants to be certain,
    // since iyzico documents no set of values for it.
    assert_eq!(user.enrollments[0].status.as_deref(), Some("PASSIVE"));
}

#[tokio::test]
async fn the_user_list_is_paged_the_way_iyzico_counts() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v3/in-store/user/list"))
        // iyzico counts pages from one, not zero.
        .and(query_param("pageNumber", "1"))
        .and(query_param("pageCount", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "userList": [
                { "userId": "kasiyer-7", "enrollments": [{ "enrolledBank": "A" }] },
                { "userId": "kasiyer-8" },
            ],
        })))
        .mount(&server)
        .await;

    let users = client(&server).users(1, 50).await.expect("the list reads");
    assert_eq!(users.len(), 2);
    assert!(users[0].can_take_payment());
    assert!(!users[1].can_take_payment());
}

#[tokio::test]
async fn forgetting_a_user_sends_a_delete_with_a_body() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/in-store/user"))
        .and(body_json(json!({ "userId": "kasiyer-7" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "userId": "kasiyer-7",
        })))
        .mount(&server)
        .await;

    client(&server)
        .forget_user("kasiyer-7")
        .await
        .expect("the user is forgotten");
}

#[tokio::test]
async fn a_refused_user_carries_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v3/in-store/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5201",
            "errorMessage": "Kullanici zaten kayitli",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .create_user("kasiyer-7")
        .await
        .expect_err("a failure status is not a user");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5201"));
}

#[tokio::test]
async fn forgetting_a_different_user_than_was_asked_for_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v3/in-store/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            // Not the one that was asked for.
            "userId": "kasiyer-9",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .forget_user("kasiyer-7")
        .await
        .expect_err("a different user coming back is not a success");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}
