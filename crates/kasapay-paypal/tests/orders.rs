//! PayPal's Orders v2 API against a mock server.
//!
//! **Every response body here is one of PayPal's own documented examples**,
//! copied from `checkout_orders_v2.json` in
//! [paypal/paypal-rest-api-specifications](https://github.com/paypal/paypal-rest-api-specifications) —
//! `orders_create_simple`, `00_orders_get`, `00_orders_capture` — or from
//! their standard `error` object's own documented shape. Where PayPal
//! documents no shape, there is no test for it.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, IdSource, IdempotencyKey, Money, NextAction, OrderRef,
    PaymentId, Provider, Secret, Status,
};
use kasapay_paypal::{Config, PayPal};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// PayPal's own token response example, from their authentication reference.
const ACCESS_TOKEN: &str = "A21AAFEpH4PsADK7qSS7pSRsgzfENtu-Q1ysgEDVDESseMHBYXVJYE8ovjj68";

/// Mounts PayPal's token endpoint and builds a client against the mock server.
///
/// Every call this crate makes fetches a token first, so every test needs
/// this mounted regardless of what it is actually testing.
async fn client(server: &MockServer) -> PayPal {
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "scope": "https://uri.paypal.com/services/invoicing",
            "access_token": ACCESS_TOKEN,
            "token_type": "Bearer",
            "app_id": "APP-80W284485P519543T",
            "expires_in": 31_668,
            "nonce": "2020-04-03T15:35:36ZaYZlGvEkV4yVSz8g6bAKFoGSEzuy3CQcz3ljhibkOHg"
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

fn request(amount: Money) -> ChargeRequest {
    ChargeRequest::builder(OrderRef::new("ord-1"), amount)
        .build()
        .expect("valid request")
}

fn hundred_dollars() -> Money {
    Money::parse("100.00", Currency::Usd).expect("valid amount")
}

/// PayPal's `orders_create_simple` example: "Create Order - Minimal Request
/// and Response". Answers only `id` and `links` — no `purchase_units` at all,
/// even under `Prefer: return=representation`.
fn created_order() -> serde_json::Value {
    json!({
        "id": "5O190127TN364715T",
        "links": [
            { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" },
            { "href": "https://www.paypal.com/checkoutnow?token=5O190127TN364715T", "rel": "approve", "method": "GET" },
            { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "edit", "method": "PATCH" },
            { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T/capture", "rel": "capture", "method": "POST" }
        ]
    })
}

#[tokio::test]
async fn opening_an_order_answers_where_to_send_the_payer() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(header("Authorization", format!("Bearer {ACCESS_TOKEN}")))
        .and(header("Prefer", "return=representation"))
        .and(body_string_contains(r#""intent":"CAPTURE""#))
        .and(body_string_contains(r#""currency_code":"USD""#))
        .and(body_string_contains(r#""value":"100.00""#))
        .and(body_string_contains(r#""custom_id":"ord-1""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    let charge = paypal
        .charge(&request(hundred_dollars()))
        .await
        .expect("the order opens");

    // PayPal's own minimal example carries no purchase_units, so this is the
    // request's own amount and reference read back rather than PayPal's.
    assert_eq!(charge.amount, hundred_dollars());
    assert_eq!(charge.order.as_ref().map(OrderRef::as_str), Some("ord-1"));
    assert!(charge.order_amount.is_none());
    assert_eq!(charge.status, Status::RequiresAction);

    let id = charge.id.as_ref().expect("PayPal names every order");
    assert_eq!(id.as_str(), "5O190127TN364715T");
    assert_eq!(
        id.source(),
        IdSource::Provider,
        "PayPal issues the identifier itself"
    );

    match charge.next_action.expect("an approval address") {
        NextAction::Redirect { url, continuation } => {
            assert_eq!(
                url.as_str(),
                "https://www.paypal.com/checkoutnow?token=5O190127TN364715T"
            );
            assert!(continuation.is_none());
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
}

/// A currency PayPal does not take is refused before anything is sent.
///
/// The mock server answers nothing at all — not even a token — so a request
/// that reaches it fails the test rather than passing quietly. This is the
/// bug that shipped in the PayTR adapter: an unmapped currency went out as an
/// empty string.
#[tokio::test]
async fn a_currency_paypal_does_not_take_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    for currency in [Currency::Try, Currency::Kwd] {
        let amount = Money::from_minor_units(1000, currency);
        let error = paypal
            .charge(&request(amount))
            .await
            .expect_err("PayPal does not settle in it");
        assert_eq!(error.kind(), ErrorKind::Unsupported, "{currency}");
    }
}

/// Yen has no minor unit, and PayPal documents it as a zero-decimal currency.
#[tokio::test]
async fn a_currency_with_no_minor_unit_is_sent_without_a_decimal_point() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(body_string_contains(r#""currency_code":"JPY""#))
        .and(body_string_contains(r#""value":"1200""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    let amount = Money::parse("1200", Currency::Jpy).expect("valid amount");
    paypal
        .charge(&request(amount))
        .await
        .expect("the order opens");
}

/// `ChargeRequest::return_url`, when set, is sent as both PayPal's own
/// `return_url` and `cancel_url` — there is no field on `ChargeRequest` for
/// the second, so this crate reuses the one URL rather than sending a
/// mismatched pair.
#[tokio::test]
async fn a_return_url_is_sent_as_both_return_and_cancel() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(body_string_contains(
            r#""return_url":"https://webshop.example/order/1""#,
        ))
        .and(body_string_contains(
            r#""cancel_url":"https://webshop.example/order/1""#,
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    let request = ChargeRequest::builder(OrderRef::new("ord-1"), hundred_dollars())
        .return_url(
            "https://webshop.example/order/1"
                .parse()
                .expect("valid url"),
        )
        .build()
        .expect("valid request");
    paypal.charge(&request).await.expect("the order opens");
}

/// Leaving `return_url` unset sends no `payment_source` at all — PayPal's own
/// minimal example does the same.
#[tokio::test]
async fn no_return_url_sends_no_payment_source() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    paypal
        .charge(&request(hundred_dollars()))
        .await
        .expect("the order opens");

    let sent = &server.received_requests().await.expect("requests recorded");
    let create = sent
        .iter()
        .find(|r| r.url.path() == "/v2/checkout/orders")
        .expect("the create request was sent");
    let body = String::from_utf8_lossy(&create.body);
    assert!(!body.contains("payment_source"), "{body}");
}

#[tokio::test]
async fn an_idempotency_key_travels_as_paypals_own_header() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(header(
            "PayPal-Request-Id",
            "7b92603e-77ed-4896-8e78-5dea2050476a",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_order()))
        .mount(&server)
        .await;

    let request = ChargeRequest::builder(OrderRef::new("ord-1"), hundred_dollars())
        .idempotency_key(IdempotencyKey::new("7b92603e-77ed-4896-8e78-5dea2050476a"))
        .build()
        .expect("valid request");
    paypal.charge(&request).await.expect("the order opens");
}

/// PayPal's `00_orders_get` example: "Get Order - Show Order Details After
/// Customer Approval".
#[tokio::test]
async fn reading_an_approved_order_back_answers_the_approval_link_again() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/5O190127TN364715T"))
        .and(header("Authorization", format!("Bearer {ACCESS_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
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
                    "amount": { "currency_code": "USD", "value": "100.00" }
                }
            ],
            "payer": {
                "name": { "given_name": "John", "surname": "Doe" },
                "email_address": "customer@example.com",
                "payer_id": "QYR5Z8XDVJNXQ"
            },
            "create_time": "2018-04-01T21:18:49Z",
            "links": [
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" },
                { "href": "https://www.paypal.com/checkoutnow?token=5O190127TN364715T", "rel": "approve", "method": "GET" },
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "edit", "method": "PATCH" },
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T/capture", "rel": "capture", "method": "POST" }
            ]
        })))
        .mount(&server)
        .await;

    let charge = paypal
        .charge_status(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect("the order reads back");

    assert_eq!(charge.amount, hundred_dollars());
    // This example carries no top-level `status` either — read from the
    // approval link, the same as a freshly created order.
    assert_eq!(charge.status, Status::RequiresAction);
    assert!(charge.next_action.is_some());
}

/// PayPal's `00_orders_capture` example: "Capture Order - PayPal Wallet as
/// Payment Source". No top-level `status`; the capture's own status is what
/// says the money moved.
///
/// `capture_order` passes no fallback (unlike `create_order`), so this is the
/// one response body a capture test can use — a `purchase_units`-less answer
/// like `created_order()` fails `resolve_amount` with no fallback to catch it.
fn captured_order() -> serde_json::Value {
    json!({
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
                "shipping": {
                    "address": {
                        "address_line_1": "2211 N First Street",
                        "address_line_2": "Building 17",
                        "admin_area_2": "San Jose",
                        "admin_area_1": "CA",
                        "postal_code": "95131",
                        "country_code": "US"
                    }
                },
                "payments": {
                    "captures": [
                        {
                            "id": "3C679366HH908993F",
                            "status": "COMPLETED",
                            "amount": { "currency_code": "USD", "value": "100.00" },
                            "final_capture": true,
                            "create_time": "2018-04-01T21:20:49Z",
                            "update_time": "2018-04-01T21:20:49Z"
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
    })
}

#[tokio::test]
async fn capturing_an_order_reads_the_captures_own_status() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/capture"))
        .and(header("Authorization", format!("Bearer {ACCESS_TOKEN}")))
        .respond_with(ResponseTemplate::new(201).set_body_json(captured_order()))
        .mount(&server)
        .await;

    let charge = paypal
        .capture(&PaymentId::issued("5O190127TN364715T"), None, None)
        .await
        .expect("the order captures");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount, hundred_dollars());
    // The order carries no `approve` link once it is captured, and Status is
    // no longer open, so there is nowhere left to send the payer.
    assert!(charge.next_action.is_none());
}

#[tokio::test]
async fn an_idempotency_key_travels_as_paypals_own_header_on_capture_too() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/capture"))
        .and(header(
            "PayPal-Request-Id",
            "9c47c9c4-7a2c-4e6b-9c5e-2f6c7d3e9a1b",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(captured_order()))
        .mount(&server)
        .await;

    let key = IdempotencyKey::new("9c47c9c4-7a2c-4e6b-9c5e-2f6c7d3e9a1b");
    paypal
        .capture(&PaymentId::issued("5O190127TN364715T"), None, Some(&key))
        .await
        .expect("the key is accepted");
}

/// PayPal's Orders-level capture takes no amount at all — refused before a
/// socket opens rather than silently taking the whole order.
#[tokio::test]
async fn a_partial_capture_is_refused_before_it_is_sent() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let partial = Money::parse("40.00", Currency::Usd).expect("valid amount");
    let error = paypal
        .capture(&PaymentId::issued("5O190127TN364715T"), Some(partial), None)
        .await
        .expect_err("PayPal's Orders-level capture has no amount field");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

/// PayPal's Orders v2 API has no cancel or void operation at all.
#[tokio::test]
async fn cancel_is_always_refused() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let error = paypal
        .cancel(&PaymentId::issued("5O190127TN364715T"))
        .await
        .expect_err("there is no cancel operation to call");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

/// Vaulting is a separate API this crate does not implement.
#[tokio::test]
async fn instruments_is_always_refused() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let error = paypal
        .instruments("customer@example.com")
        .await
        .expect_err("Vault is out of scope");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn what_the_client_says_it_will_do() {
    let server = MockServer::start().await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");

    let capabilities = paypal.capabilities();
    assert!(capabilities.separate_capture);
    assert!(!capabilities.partial_capture);
    assert!(!capabilities.partial_refund);
    assert!(!capabilities.repeated_refund);
    assert!(!capabilities.saved_instruments);
    assert_eq!(paypal.id().as_str(), "paypal");
}
