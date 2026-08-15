//! `intent: AUTHORIZE` and PayPal's Authorizations resource, against a mock
//! server.
//!
//! **Every response body here is one of PayPal's own documented examples**,
//! copied from `checkout_orders_v2.json`'s `authorize` operation —
//! `00_orders_authorize` — and from `payments_payment_v2.json`'s
//! `authorizations/{id}/capture` operation —
//! `authorizations_capture_empty_request`,
//! `authorizations_capture_with_prefer_header`. Where PayPal documents no
//! shape, there is no test for it: this crate does not call
//! `/v2/payments/authorizations/{id}/void`, so there is nothing here for it
//! either.
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
    ChargeRequest, Currency, ErrorKind, IdempotencyKey, Money, OrderRef, PaymentId, Provider,
    Status,
};
use kasapay_paypal::{AuthorizationId, Config, PayPal};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
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
            kasapay_core::Secret::new("client-id"),
            kasapay_core::Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds")
}

fn hundred_dollars() -> Money {
    Money::parse("100.00", Currency::Usd).expect("valid amount")
}

/// PayPal's own minimal "Create Order" example — the same one `tests/orders.rs`
/// uses, since intent does not change this shape at all.
fn created_order() -> serde_json::Value {
    json!({
        "id": "5O190127TN364715T",
        "links": [
            { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" },
            { "href": "https://www.paypal.com/checkoutnow?token=5O190127TN364715T", "rel": "approve", "method": "GET" }
        ]
    })
}

#[tokio::test]
async fn authorize_asks_for_a_hold_rather_than_capture() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(body_string_contains(r#""intent":"AUTHORIZE""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    let request = ChargeRequest::builder(OrderRef::new("ord-1"), hundred_dollars())
        .build()
        .expect("valid request");
    paypal.authorize(&request).await.expect("the order opens");
}

/// PayPal's own `00_orders_authorize` example: "Authorize Order - PayPal
/// Wallet as Payment Source". The authorization's own `CREATED` status reads
/// as `Status::Authorized`, and its `expiration_time` sits on `Charge::raw`
/// rather than a typed field.
#[tokio::test]
async fn placing_the_hold_answers_an_authorized_charge() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/authorize"))
        .and(header("Authorization", format!("Bearer {ACCESS_TOKEN}")))
        .and(header("Prefer", "return=representation"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "5O190127TN364715T",
            "payment_source": {
                "paypal": {
                    "name": { "given_name": "John", "surname": "Doe" },
                    "email_address": "customer@example.com",
                    "account_id": "QYR5Z8XDVJNXQ"
                }
            },
            "purchase_units": [
                {
                    "reference_id": "d9f80740-38f0-11e8-b467-0ed5f89f718b",
                    "payments": {
                        "authorizations": [
                            {
                                "id": "3C679366HH908993F",
                                "status": "CREATED",
                                "amount": { "currency_code": "USD", "value": "100.00" },
                                "seller_protection": {
                                    "status": "ELIGIBLE",
                                    "dispute_categories": ["ITEM_NOT_RECEIVED", "UNAUTHORIZED_TRANSACTION"]
                                },
                                "expiration_time": "2021-10-08T23:37:39Z",
                                "links": [
                                    { "href": "https://api-m.paypal.com/v2/payments/authorizations/5O190127TN364715T", "rel": "self", "method": "GET" },
                                    { "href": "https://api-m.paypal.com/v2/payments/authorizations/5O190127TN364715T/capture", "rel": "capture", "method": "POST" },
                                    { "href": "https://api-m.paypal.com/v2/payments/authorizations/5O190127TN364715T/void", "rel": "void", "method": "POST" },
                                    { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "up", "method": "GET" }
                                ]
                            }
                        ]
                    }
                }
            ],
            "payer": {
                "name": { "given_name": "John", "surname": "Doe" },
                "email_address": "customer@example.com",
                "payer_id": "QYR5Z8XDVJNXQ"
            },
            "links": [
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let charge = paypal
        .authorize_order(&PaymentId::issued("5O190127TN364715T"), None)
        .await
        .expect("the hold is placed");

    assert_eq!(charge.status, Status::Authorized);
    assert_eq!(charge.amount, hundred_dollars());
    assert_eq!(
        charge.id.as_ref().map(PaymentId::as_str),
        Some("5O190127TN364715T")
    );
    // There is no approve link left once a hold is placed, and Status is
    // still "open" for Authorized — this proves next_action does not invent
    // a redirect that is not there.
    assert!(charge.next_action.is_none());

    // The authorization's own id and expiry are both on the raw body,
    // exactly where PayPal put them, rather than on a typed field.
    let authorization = kasapay_paypal::authorization_id(&charge).expect("an authorization id");
    assert_eq!(authorization.as_str(), "3C679366HH908993F");
    assert_eq!(
        charge
            .raw
            .text_at("/purchase_units/0/payments/authorizations/0/expiration_time")
            .as_deref(),
        Some("2021-10-08T23:37:39Z")
    );
}

/// `PayPal::authorize_order` on a `PayPal-Request-Id` travels the same way
/// `PayPal::create_order`'s does.
#[tokio::test]
async fn authorize_orders_own_request_id_travels_as_paypals_header() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/authorize"))
        .and(header(
            "PayPal-Request-Id",
            "7b92603e-77ed-4896-8e78-5dea2050476a",
        ))
        .and(body_string_contains("{}"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "5O190127TN364715T",
            "purchase_units": [
                {
                    "payments": {
                        "authorizations": [
                            {
                                "id": "3C679366HH908993F",
                                "status": "CREATED",
                                "amount": { "currency_code": "USD", "value": "100.00" }
                            }
                        ]
                    }
                }
            ],
            "links": []
        })))
        .mount(&server)
        .await;

    paypal
        .authorize_order(
            &PaymentId::issued("5O190127TN364715T"),
            Some(&IdempotencyKey::new("7b92603e-77ed-4896-8e78-5dea2050476a")),
        )
        .await
        .expect("the hold is placed");
}

/// An order nobody has authorized has nothing for `authorization_id` to read.
///
/// Built by hand, for the same reason the refund test's is: PayPal documents
/// no example of the shape this branch exists to survive.
#[tokio::test]
async fn an_order_with_no_authorization_has_no_authorization_id() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "5O190127TN364715T",
            "status": "CREATED",
            "purchase_units": [
                { "amount": { "currency_code": "USD", "value": "100.00" } }
            ],
            "links": [
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" },
                { "href": "https://www.paypal.com/checkoutnow?token=5O190127TN364715T", "rel": "approve", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let charge = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect("the order reads back");

    let error = kasapay_paypal::authorization_id(&charge).expect_err("nothing has been placed");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// PayPal's own `authorizations_capture_empty_request` example: capturing
/// the whole hold sends `{}`, and the answer is a bare capture object, not an
/// order — `Status::Pending` here because this particular example is
/// PayPal's own documented case of a capture accepted but not yet settled
/// (`status_details.reason: "OTHER"`).
#[tokio::test]
async fn capturing_the_whole_hold_sends_no_amount_at_all() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path(
            "/v2/payments/authorizations/6DR965477U7140544/capture",
        ))
        .and(header("Prefer", "return=representation"))
        .and(body_string_contains("{}"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "7TK53561YB803214S",
            "amount": { "currency_code": "USD", "value": "100.00" },
            "final_capture": true,
            "seller_protection": {
                "status": "ELIGIBLE",
                "dispute_categories": ["ITEM_NOT_RECEIVED", "UNAUTHORIZED_TRANSACTION"]
            },
            "seller_receivable_breakdown": {
                "gross_amount": { "currency_code": "USD", "value": "100.00" },
                "paypal_fee": { "currency_code": "USD", "value": "3.98" },
                "net_amount": { "currency_code": "USD", "value": "96.02" }
            },
            "invoice_id": "OrderInvoice-10_10_2024_12_58_20_pm",
            "status": "PENDING",
            "status_details": { "reason": "OTHER" },
            "create_time": "2024-10-14T21:37:10Z",
            "update_time": "2024-10-14T21:37:10Z",
            "links": [
                { "href": "https://api.msmaster.qa.paypal.com/v2/payments/captures/7TK53561YB803214S", "rel": "self", "method": "GET" },
                { "href": "https://api.msmaster.qa.paypal.com/v2/payments/captures/7TK53561YB803214S/refund", "rel": "refund", "method": "POST" },
                { "href": "https://api.msmaster.qa.paypal.com/v2/payments/authorizations/6DR965477U7140544", "rel": "up", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let authorization = AuthorizationId::issued("6DR965477U7140544");
    let capture = paypal
        .capture_authorization(&authorization, None, None)
        .await
        .expect("the hold captures");

    assert_eq!(capture.id.as_str(), "7TK53561YB803214S");
    assert_eq!(capture.amount, hundred_dollars());
    assert_eq!(capture.status, Status::Pending);
    assert_eq!(capture.authorization, authorization);
}

/// PayPal's own `authorizations_capture_with_prefer_header` example: a
/// partial capture sends the amount, and a `COMPLETED` capture reads as
/// `Status::Captured`.
#[tokio::test]
async fn a_partial_capture_of_the_hold_sends_the_amount() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path(
            "/v2/payments/authorizations/0VF52814937998046/capture",
        ))
        .and(body_string_contains(r#""value":"10.99""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "2GG279541U471931P",
            "status": "COMPLETED",
            "status_details": {},
            "amount": { "value": "10.99", "currency_code": "USD" },
            "final_capture": true,
            "seller_protection": {
                "status": "ELIGIBLE",
                "dispute_categories": ["ITEM_NOT_RECEIVED", "UNAUTHORIZED_TRANSACTION"]
            },
            "seller_receivable_breakdown": {
                "gross_amount": { "value": "10.99", "currency_code": "USD" },
                "paypal_fee": { "value": "0.33", "currency_code": "USD" },
                "net_amount": { "value": "10.66", "currency_code": "USD" }
            },
            "invoice_id": "INV-123",
            "create_time": "2017-09-11T23:24:01Z",
            "update_time": "2017-09-11T23:24:01Z",
            "links": [
                { "rel": "self", "method": "GET", "href": "https://api-m.paypal.com/v2/payments/captures/2GG279541U471931P" },
                { "rel": "refund", "method": "POST", "href": "https://api-m.paypal.com/v2/payments/captures/2GG279541U471931P/refund" },
                { "rel": "up", "method": "GET", "href": "https://api-m.paypal.com/v2/payments/authorizations/0VF52814937998046" }
            ]
        })))
        .mount(&server)
        .await;

    let authorization = AuthorizationId::issued("0VF52814937998046");
    let amount = Money::parse("10.99", Currency::Usd).expect("valid amount");
    let capture = paypal
        .capture_authorization(&authorization, Some(amount), None)
        .await
        .expect("the hold captures");

    assert_eq!(capture.id.as_str(), "2GG279541U471931P");
    assert_eq!(capture.amount, amount);
    assert_eq!(capture.status, Status::Captured);
}

/// A zero amount is refused before a socket opens, the same as everywhere
/// else in this crate an amount is optional but not allowed to be nothing.
#[tokio::test]
async fn a_zero_amount_capture_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            kasapay_core::Secret::new("client-id"),
            kasapay_core::Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let zero = Money::from_minor_units(0, Currency::Usd);
    let error = paypal
        .capture_authorization(
            &AuthorizationId::issued("0VF52814937998046"),
            Some(zero),
            None,
        )
        .await
        .expect_err("zero is not a capturable amount");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}
