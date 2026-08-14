//! What Mollie's refusals mean.
//!
//! Mollie has no error codes. Its error object is `status`, `title`, `detail`
//! and sometimes `field`, so the HTTP status is the whole of the machine-
//! readable part and every body below is one Mollie documents — on the
//! "Handling errors" page, or as the example on the operation itself.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, Money, OrderRef, PaymentId, Provider, Secret,
};
use kasapay_mollie::{Config, Mollie};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Mollie {
    Mollie::new(Config::at(&server.uri(), Secret::new("test_kasapay")).expect("valid base"))
        .expect("client builds")
}

fn request() -> ChargeRequest {
    ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("10.00", Currency::Eur).expect("valid amount"),
    )
    .description("Order #12345")
    .return_url("https://example.com".parse().expect("valid url"))
    .build()
    .expect("valid request")
}

/// Mollie's own example for a key that does not authenticate.
#[tokio::test]
async fn a_key_that_does_not_authenticate_is_an_auth_failure() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "status": 401,
            "title": "Unauthorized Request",
            "detail": "Missing authentication, or failed to authenticate",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect_err("the key does not authenticate");
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(!error.is_retryable());
    assert!(
        error.to_string().contains("failed to authenticate"),
        "{error}"
    );
    // Mollie sends no code of any kind, and nothing here invents one.
    assert!(error.code().is_none());
}

/// Mollie's own example for a payment that does not exist.
#[tokio::test]
async fn a_payment_mollie_does_not_know_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_I_dont_exist"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "status": 404,
            "title": "Not Found",
            "detail": "No payment exists with token tr_I_dont_exist.",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("tr_I_dont_exist"))
        .await
        .expect_err("no such payment");
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(!error.is_retryable());
}

/// Mollie's own 422 example on `create-payment`. The field that caused it is the
/// only machine-readable thing in the body, and it goes in the message.
#[tokio::test]
async fn a_refused_request_names_the_field_that_caused_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "status": 422,
            "title": "Unprocessable Entity",
            "detail": "The 'description' field is missing",
            "field": "description",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge(&request())
        .await
        .expect_err("Mollie refused the payment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert!(!error.is_retryable());
    assert!(error.to_string().contains("field: description"), "{error}");
}

/// Mollie's own 503 example: the payment method's supplier is down, which is a
/// failure that goes away by itself.
#[tokio::test]
async fn a_payment_method_supplier_that_is_down_is_worth_retrying() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "status": 503,
            "title": "Service Unavailable",
            "detail": "Payment platform for this payment method temporarily not available",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge(&request())
        .await
        .expect_err("the supplier is down");
    assert_eq!(error.kind(), ErrorKind::Provider);
    assert!(error.is_retryable());
}

/// Mollie's own `429-rate-limit` response.
#[tokio::test]
async fn a_rate_limit_is_worth_retrying_too() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "3")
                .set_body_json(json!({
                    "status": 429,
                    "title": "Too Many Requests",
                    "detail": "You have exceeded the rate limit. Please slow down your requests.",
                    "_links": {
                        "documentation": {
                            "href": "https://docs.mollie.com/overview/handling-errors",
                            "type": "text/html"
                        }
                    }
                })),
        )
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect_err("too many requests");
    assert_eq!(error.kind(), ErrorKind::RateLimited);
    assert!(error.is_retryable());
}

/// Mollie's own 422 for a payment that can no longer be cancelled — which is
/// also what an authorised payment answers, because releasing a hold is a
/// different call.
#[tokio::test]
async fn cancelling_a_payment_that_cannot_be_cancelled_is_a_bad_request() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "status": 422,
            "title": "Unprocessable Entity",
            "detail": "The payment cannot be cancelled",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .cancel(&PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect_err("it cannot be cancelled");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

/// Mollie's own 409 on `create-refund`.
#[tokio::test]
async fn a_duplicate_refund_is_refused_rather_than_repeated() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments/tr_WDqYK6vllg/refunds"))
        .respond_with(ResponseTemplate::new(409).set_body_json(json!({
            "status": 409,
            "title": "Conflict",
            "detail": "A duplicate refund has been detected",
            "_links": { "documentation": { "href": "...", "type": "text/html" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund(
            &PaymentId::issued("tr_WDqYK6vllg"),
            Money::parse("5.95", Currency::Eur).expect("valid amount"),
        )
        .await
        .expect_err("Mollie saw this refund already");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    // Retrying is what got here.
    assert!(!error.is_retryable());
}

/// A body that is not the JSON Mollie documents is kept and reported as such,
/// rather than read as a payment nobody made.
#[tokio::test]
async fn an_answer_that_is_not_a_payment_is_malformed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect_err("that is not a payment");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// A payment settled in a currency `Currency` cannot name is refused on the
/// way back as well as on the way out. Mollie takes twenty-seven and kasapay
/// names nine.
#[tokio::test]
async fn a_payment_in_a_currency_kasapay_cannot_name_is_not_guessed_at() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": "payment",
            "id": "tr_WDqYK6vllg",
            "mode": "live",
            "amount": { "value": "100.00", "currency": "SEK" },
            "description": "Order 33",
            "status": "paid",
            "createdAt": "2024-03-20T09:13:37+00:00",
            "profileId": "pfl_QkEhN94Ba",
            "sequenceType": "oneoff",
            "_links": { "self": { "href": "...", "type": "application/hal+json" } }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect_err("kasapay has no Currency for krona");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("SEK"), "{error}");
}
