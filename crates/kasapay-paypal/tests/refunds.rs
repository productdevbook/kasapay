//! Refunds against a mock server.
//!
//! **Every response body here is one of PayPal's own documented examples**,
//! copied from `payments_payment_v2.json`'s
//! `POST /v2/payments/captures/{id}/refund` operation —
//! `captures_refund_empty_request`, `captures_refund_200_idempotent_response`
//! — and from `checkout_orders_v2.json`'s `00_orders_capture` example, which
//! is what [`kasapay_paypal::capture_id`] reads a capture id out of. Where
//! PayPal documents no shape, there is no test for it.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, IdempotencyKey, Money, PaymentId, Provider};
use kasapay_paypal::{CaptureId, Config, PayPal, RefundState};
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

/// PayPal's own `captures_refund_empty_request` example: an empty body
/// refunds `captured amount - previous refunds`, which is the whole capture
/// the first time.
#[tokio::test]
async fn a_full_refund_sends_no_amount_at_all() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/payments/captures/7TK53561YB803214S/refund"))
        .and(header("Authorization", format!("Bearer {ACCESS_TOKEN}")))
        .and(header("Prefer", "return=representation"))
        .and(body_string_contains("{}"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "58K15806CS993444T",
            "amount": { "currency_code": "USD", "value": "89.00" },
            "seller_payable_breakdown": {
                "gross_amount": { "currency_code": "USD", "value": "89.00" },
                "paypal_fee": { "currency_code": "USD", "value": "0.00" },
                "net_amount": { "currency_code": "USD", "value": "89.00" },
                "total_refunded_amount": { "currency_code": "USD", "value": "100.00" }
            },
            "invoice_id": "OrderInvoice-10_10_2024_12_58_20_pm",
            "status": "COMPLETED",
            "create_time": "2024-10-14T15:03:29-07:00",
            "update_time": "2024-10-14T15:03:29-07:00",
            "links": [
                { "href": "https://api.msmaster.qa.paypal.com/v2/payments/refunds/58K15806CS993444T", "rel": "self", "method": "GET" },
                { "href": "https://api.msmaster.qa.paypal.com/v2/payments/captures/7TK53561YB803214S", "rel": "up", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let capture = CaptureId::issued("7TK53561YB803214S");
    let refund = paypal
        .refund(&capture, None, None)
        .await
        .expect("the refund is created");

    assert_eq!(refund.id.as_str(), "58K15806CS993444T");
    assert_eq!(refund.amount.minor_units(), 8900);
    assert_eq!(refund.state, RefundState::Completed);
    assert_eq!(refund.capture, capture);
}

/// PayPal's own `captures_refund_200_idempotent_response` example: a
/// partial refund carries `amount`, and the running total is on
/// `seller_payable_breakdown.total_refunded_amount` — read here off
/// `Charge::raw`'s namesake, `Refund::raw`, rather than a typed field.
#[tokio::test]
async fn a_partial_refund_sends_the_amount() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/payments/captures/2GG279541U471931P/refund"))
        .and(body_string_contains(r#""value":"1.00""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "0K35355239430361V",
            "amount": { "currency_code": "USD", "value": "1.00" },
            "seller_payable_breakdown": {
                "gross_amount": { "currency_code": "USD", "value": "1.00" },
                "paypal_fee": { "currency_code": "USD", "value": "0.00" },
                "net_amount": { "currency_code": "USD", "value": "1.00" },
                "total_refunded_amount": { "currency_code": "USD", "value": "11.00" }
            },
            "invoice_id": "RefundInvoice-14_10_2024_4_58_32_pm",
            "custom_id": "RefundCustom-14_10_2024_4_58_32_pm",
            "status": "COMPLETED",
            "create_time": "2024-10-14T14:58:34-07:00",
            "update_time": "2024-10-14T14:58:34-07:00",
            "links": [
                { "href": "https://api.paypal.com/v2/payments/refunds/0K35355239430361V", "rel": "self", "method": "GET" },
                { "href": "https://api.paypal.com/v2/payments/captures/7TK53561YB803214S", "rel": "up", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let capture = CaptureId::issued("2GG279541U471931P");
    let amount = Money::parse("1.00", Currency::Usd).expect("valid amount");
    let refund = paypal
        .refund(&capture, Some(amount), None)
        .await
        .expect("the refund is created");

    assert_eq!(refund.id.as_str(), "0K35355239430361V");
    assert_eq!(refund.amount, amount);
    assert_eq!(refund.state, RefundState::Completed);
    // A running total across every refund on this capture, not only this one.
    assert_eq!(
        refund
            .raw
            .text_at("/seller_payable_breakdown/total_refunded_amount/value"),
        Some("11.00".to_string())
    );
}

/// Money leaving needs a key: `PayPal-Request-Id` travels the same way it
/// does on `PayPal::create_order` and `PayPal::capture_order`.
#[tokio::test]
async fn a_request_id_travels_as_paypals_own_header() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/payments/captures/7TK53561YB803214S/refund"))
        .and(header(
            "PayPal-Request-Id",
            "7b92603e-77ed-4896-8e78-5dea2050476a",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "58K15806CS993444T",
            "amount": { "currency_code": "USD", "value": "89.00" },
            "status": "COMPLETED"
        })))
        .mount(&server)
        .await;

    paypal
        .refund(
            &CaptureId::issued("7TK53561YB803214S"),
            None,
            Some(&IdempotencyKey::new("7b92603e-77ed-4896-8e78-5dea2050476a")),
        )
        .await
        .expect("the refund is created");
}

/// A zero amount is refused before a socket opens.
#[tokio::test]
async fn a_zero_amount_refund_never_reaches_the_wire() {
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
        .refund(&CaptureId::issued("7TK53561YB803214S"), Some(zero), None)
        .await
        .expect_err("zero is not a refundable amount");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

/// PayPal's own `00_orders_capture` example — the one `tests/orders.rs`
/// reads a `Status::Captured` charge from — is also what `capture_id` reads
/// a `CaptureId` out of, for `PayPal::refund`.
#[tokio::test]
async fn capture_id_reads_the_capture_off_a_captured_order() {
    let server = MockServer::start().await;
    let paypal = client(&server).await;

    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/5O190127TN364715T/capture"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "5O190127TN364715T",
            "purchase_units": [
                {
                    "reference_id": "d9f80740-38f0-11e8-b467-0ed5f89f718b",
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
            "links": [
                { "href": "https://api-m.paypal.com/v2/checkout/orders/5O190127TN364715T", "rel": "self", "method": "GET" }
            ]
        })))
        .mount(&server)
        .await;

    let charge = paypal
        .capture(&PaymentId::issued("5O190127TN364715T"), None)
        .await
        .expect("the order captures");

    let capture = kasapay_paypal::capture_id(&charge).expect("a capture id");
    assert_eq!(capture.as_str(), "3C679366HH908993F");
}

/// An order that has not been captured has nothing for `capture_id` to read.
#[tokio::test]
async fn an_uncaptured_order_has_no_capture_id() {
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

    let error = kasapay_paypal::capture_id(&charge).expect_err("nothing has been captured");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// Every value PayPal's `refund_status` schema documents.
#[test]
fn each_documented_refund_status_lands_somewhere_it_belongs() {
    assert_eq!(RefundState::from("PENDING"), RefundState::Pending);
    assert_eq!(RefundState::from("COMPLETED"), RefundState::Completed);
    assert_eq!(RefundState::from("FAILED"), RefundState::Failed);
    assert_eq!(RefundState::from("CANCELLED"), RefundState::Cancelled);
}

#[tokio::test]
async fn what_the_client_says_it_will_do_now_that_refunds_exist() {
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

    let capabilities = paypal.capabilities();
    assert!(capabilities.partial_refund);
    assert!(capabilities.repeated_refund);
}
