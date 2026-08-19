//! What PayPal's refusals mean.
//!
//! Every body below is one PayPal documents on the operation itself, in
//! `checkout_orders_v2.json`, or on their OAuth2 authentication reference for
//! the token endpoint's own error shape.
//!
//! One exception, and it is deliberate: a test that exercises a branch taken
//! only when PayPal answers something their examples never show cannot use a
//! documented example, because there is not one. Those bodies are built by
//! hand and say so where they are used. They test this crate's own fallback,
//! never a claim about what PayPal sends.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, Money, OrderRef, PaymentId, Provider, Secret,
};
use kasapay_paypal::{Config, PayPal};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACCESS_TOKEN: &str = "A21AAFEpH4PsADK7qSS7pSRsgzfENtu-Q1ysgEDVDESseMHBYXVJYE8ovjj68";

async fn client(server: &MockServer) -> PayPal {
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": ACCESS_TOKEN,
            "token_type": "Bearer",
            "expires_in": 31_668
        })))
        .mount(server)
        .await;
    PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds")
}

fn request() -> ChargeRequest {
    ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("100.00", Currency::Usd).expect("valid amount"),
    )
    .build()
    .expect("valid request")
}

/// PayPal's own generic `401` example, shared across every operation in this
/// crate's document.
#[tokio::test]
async fn missing_or_invalid_credentials_is_an_auth_failure() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "name": "AUTHENTICATION_FAILURE",
            "debug_id": "b1d1f06c7246c",
            "message": "Authentication failed due to missing Authorization header, or invalid authentication credentials."
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("the credentials do not authenticate");
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(!error.is_retryable());
    assert_eq!(error.code(), Some("AUTHENTICATION_FAILURE"));
    assert!(
        error.to_string().contains("Authentication failed"),
        "{error}"
    );
}

/// PayPal's own generic `403` example: valid credentials, insufficient
/// permission — still an authentication-shaped failure in kasapay's terms.
#[tokio::test]
async fn insufficient_permission_is_also_an_auth_failure() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "name": "NOT_AUTHORIZED",
            "debug_id": "b1d1f06c7246c",
            "message": "Authorization failed due to insufficient permissions."
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("insufficient permission");
    assert_eq!(error.kind(), ErrorKind::Auth);
}

/// PayPal's own generic `404` example.
#[tokio::test]
async fn an_order_paypal_does_not_know_is_not_found() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/does-not-exist"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "name": "RESOURCE_NOT_FOUND",
            "debug_id": "b1d1f06c7246c",
            "message": "The specified resource does not exist."
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("does-not-exist"))
        .await
        .expect_err("no such order");
    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(!error.is_retryable());
}

/// PayPal's own `create_422_unprocessable_entity_missing_pickup_address`
/// example. The detail's own description is folded into the message,
/// alongside `name`, which is the only machine-readable part.
#[tokio::test]
async fn a_refused_order_names_the_field_that_caused_it() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "name": "UNPROCESSABLE_ENTITY",
            "details": [
                {
                    "field": "/purchase_units/@reference_id=='PUHF'/shipping/address",
                    "issue": "MISSING_SHIPPING_ADDRESS",
                    "description": "The shipping address is required when `shipping_preference=SET_PROVIDED_ADDRESS`."
                }
            ],
            "message": "The requested action could not be performed, semantically incorrect, or failed business validation.",
            "debug_id": "f200264a4e02a",
            "links": [
                {
                    "href": "https://developer.paypal.com/api/rest/reference/orders/v2/errors/#MISSING_SHIPPING_ADDRESS",
                    "rel": "information_link",
                    "method": "GET"
                }
            ]
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge(&request())
        .await
        .expect_err("PayPal refused the order");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert!(!error.is_retryable());
    assert_eq!(error.code(), Some("UNPROCESSABLE_ENTITY"));
    assert!(
        error.to_string().contains("shipping address is required"),
        "{error}"
    );
    assert!(error.to_string().contains("f200264a4e02a"), "{error}");
}

/// PayPal's own `capture_422_unprocessable_entity` example: capturing an
/// order that was created with `intent=AUTHORIZE`.
#[tokio::test]
async fn capturing_the_wrong_intent_is_an_invalid_request() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/capture"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "name": "UNPROCESSABLE_ENTITY",
            "details": [
                {
                    "location": "body",
                    "issue": "ACTION_DOES_NOT_MATCH_INTENT",
                    "description": "The order was created with an intent of `AUTHORIZE`. To complete authorization, use `/v2/checkout/orders/{order_id}/authorize`. Or, alternately create an order with an intent of `CAPTURE`."
                }
            ],
            "message": "The requested action could not be performed, semantically incorrect, or failed business validation.",
            "debug_id": "90957fca61718"
        })))
        .mount(&server)
        .await;

    let error = paypal
        .capture(&PaymentId::issued("5O190127TN364715T"), None, None)
        .await
        .expect_err("this order was not created with intent CAPTURE");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("UNPROCESSABLE_ENTITY"));
    assert!(
        error.to_string().contains("intent of `AUTHORIZE`"),
        "{error}"
    );
}

/// PayPal's own generic `500` example.
#[tokio::test]
async fn a_server_error_is_worth_retrying() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "name": "INTERNAL_SERVER_ERROR",
            "debug_id": "b1d1f06c7246c",
            "message": "An internal server error has occurred."
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("PayPal failed on its own side");
    assert_eq!(error.kind(), ErrorKind::Provider);
    assert!(error.is_retryable());
}

/// A body that is not the JSON PayPal documents is kept and reported as such,
/// rather than read as an order nobody made.
#[tokio::test]
async fn an_answer_that_is_not_an_order_is_malformed() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>maintenance</html>"))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("that is not an order");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// An order settled in a currency `Currency` cannot name is refused on the
/// way back as well as on the way out.
///
/// Built by hand: PayPal documents no example in a currency kasapay has no
/// name for, which is the whole point of the branch.
#[tokio::test]
async fn an_order_in_a_currency_kasapay_cannot_name_is_not_guessed_at() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "5O190127TN364715T",
            // A top-level status, so resolving the status does not fail
            // first and mask the currency error this test is actually about.
            "status": "APPROVED",
            "purchase_units": [
                { "amount": { "currency_code": "ISK", "value": "100.00" } }
            ],
            "links": [
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("kasapay has no Currency for the króna");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("ISK"), "{error}");
}

/// PayPal's `/v1/oauth2/token` answers a different, plain OAuth2 error shape
/// — `error` and `error_description`, not the `name`/`message`/`debug_id`
/// object every other endpoint here answers with.
#[tokio::test]
async fn wrong_credentials_at_the_token_endpoint_are_an_auth_failure() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": "invalid_client",
            "error_description": "Client Authentication failed"
        })))
        .mount(&server)
        .await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("wrong-id"),
            Secret::new("wrong-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let error = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("the credentials do not authenticate");
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert_eq!(error.code(), Some("invalid_client"));
    assert!(
        error.to_string().contains("Client Authentication failed"),
        "{error}"
    );
}
