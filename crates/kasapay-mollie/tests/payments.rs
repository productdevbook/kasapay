//! Mollie against a mock server.
//!
//! **Every response body here is one of Mollie's own documented examples**,
//! copied from their OpenAPI document — `create-payment-201-2`, `get-payment`,
//! `cancel-payment`, `get-capture`, `get-refund` — or from the "Handling
//! errors" page. Where Mollie documents no shape, there is no test for it.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, IdSource, IdempotencyKey, Money, NextAction, OrderRef,
    Provider, Secret, Status,
};
use kasapay_mollie::{CaptureState, Config, Mollie, RefundState};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Mollie {
    let config = Config::at(&server.uri(), Secret::new("test_kasapay")).expect("valid base");
    Mollie::new(config).expect("client builds")
}

/// Mollie's `create-payment-201-2`, "Minimum viable request".
fn created_payment() -> serde_json::Value {
    json!({
        "resource": "payment",
        "id": "tr_KQb4E3WfpA",
        "mode": "test",
        "createdAt": "2021-12-14T12:53:25+00:00",
        "amount": { "value": "10.00", "currency": "EUR" },
        "description": "I can do payments now",
        "method": "ideal",
        "metadata": { "kasapay_order": "ord-1" },
        "status": "open",
        "isCancelable": false,
        "expiresAt": "2021-12-14T13:08:25+00:00",
        "profileId": "pfl_fFdmWR2qQU",
        "sequenceType": "oneoff",
        "redirectUrl": "https://example.com",
        "_links": {
            "self": { "href": "...", "type": "application/hal+json" },
            "checkout": {
                "href": "https://www.mollie.com/checkout/select-method/KQb4E3WfpA",
                "type": "text/html"
            },
            "dashboard": {
                "href": "https://www.mollie.com/dashboard/org_9253561/payments/tr_KQb4E3WfpA",
                "type": "text/html"
            },
            "documentation": { "href": "...", "type": "text/html" }
        }
    })
}

fn request(amount: Money) -> ChargeRequest {
    ChargeRequest::builder(OrderRef::new("ord-1"), amount)
        .description("I can do payments now")
        .return_url("https://example.com".parse().expect("valid url"))
        .build()
        .expect("valid request")
}

fn ten_euro() -> Money {
    Money::parse("10.00", Currency::Eur).expect("valid amount")
}

#[tokio::test]
async fn opening_a_payment_answers_where_to_send_the_payer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .and(header("Authorization", "Bearer test_kasapay"))
        // The amount is an object with a decimal string in it, written from
        // the integer count of minor units. Never a number, never a float.
        .and(body_string_contains(r#""currency":"EUR""#))
        .and(body_string_contains(r#""value":"10.00""#))
        // The order reference has no Mollie field of its own.
        .and(body_string_contains(r#""kasapay_order":"ord-1""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_payment()))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge(&request(ten_euro()))
        .await
        .expect("the payment opens");

    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(charge.amount, ten_euro());
    // Mollie has no second figure: the amount is the order and the charge at once.
    assert!(charge.order_amount.is_none());
    assert_eq!(charge.order.as_ref().map(OrderRef::as_str), Some("ord-1"));

    let id = charge.id.as_ref().expect("Mollie names every payment");
    assert_eq!(id.as_str(), "tr_KQb4E3WfpA");
    assert_eq!(
        id.source(),
        IdSource::Provider,
        "Mollie issues the identifier itself"
    );

    match charge.next_action.expect("a checkout to send the payer to") {
        NextAction::Redirect { url, continuation } => {
            assert_eq!(
                url.as_str(),
                "https://www.mollie.com/checkout/select-method/KQb4E3WfpA"
            );
            // Mollie wants nothing back: the payer returns to the merchant's
            // own address and the payment is read by its id.
            assert!(continuation.is_none());
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
}

/// A currency Mollie does not take is refused before anything is sent.
///
/// The mock server answers nothing at all, so a request that reaches it fails
/// the test rather than passing quietly. This is the bug that shipped in the
/// PayTR adapter: an unmapped currency went out as an empty string.
#[tokio::test]
async fn a_currency_mollie_does_not_take_never_reaches_the_wire() {
    let server = MockServer::start().await;
    for currency in [Currency::Try, Currency::Kwd] {
        let amount = Money::from_minor_units(1000, currency);
        let error = client(&server)
            .charge(&request(amount))
            .await
            .expect_err("Mollie does not settle in it");
        assert_eq!(error.kind(), ErrorKind::Unsupported, "{currency}");
    }
}

/// Yen has no minor unit, and Mollie documents it as taking none.
#[tokio::test]
async fn a_currency_with_no_minor_unit_is_sent_without_a_decimal_point() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .and(body_string_contains(r#""currency":"JPY""#))
        .and(body_string_contains(r#""value":"1200""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_payment()))
        .mount(&server)
        .await;

    let amount = Money::parse("1200", Currency::Jpy).expect("valid amount");
    client(&server)
        .charge(&request(amount))
        .await
        .expect("the payment opens");
}

/// Mollie requires both, and `ChargeRequest` calls both optional.
#[tokio::test]
async fn a_request_missing_what_mollie_requires_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let without_description = ChargeRequest::builder(OrderRef::new("ord-1"), ten_euro())
        .return_url("https://example.com".parse().expect("valid url"))
        .build()
        .expect("valid request");
    let without_return_url = ChargeRequest::builder(OrderRef::new("ord-1"), ten_euro())
        .description("Order #12345")
        .build()
        .expect("valid request");

    for (request, wanted) in [
        (without_description, "description"),
        (without_return_url, "return_url"),
    ] {
        let error = client(&server)
            .charge(&request)
            .await
            .expect_err("Mollie requires it");
        assert_eq!(error.kind(), ErrorKind::InvalidRequest);
        assert!(error.to_string().contains(wanted), "{error}");
    }
}

#[tokio::test]
async fn an_idempotency_key_travels_as_mollies_own_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .and(header("Idempotency-Key", "123e4567-e89b-12d3-a456-426"))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_payment()))
        .mount(&server)
        .await;

    let request = ChargeRequest::builder(OrderRef::new("ord-1"), ten_euro())
        .description("Order #12345")
        .return_url("https://example.com".parse().expect("valid url"))
        // The value from Mollie's own `idempotency-key` parameter.
        .idempotency_key(IdempotencyKey::new("123e4567-e89b-12d3-a456-426"))
        .build()
        .expect("valid request");

    client(&server)
        .charge(&request)
        .await
        .expect("the payment opens");
}

/// Mollie's `get-payment` example.
#[tokio::test]
async fn reading_a_payment_back_reads_the_order_out_of_its_metadata() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": "payment",
            "id": "tr_5B8cwPMGnU6qLbRvo7qEZo",
            "mode": "live",
            "amount": { "value": "10.00", "currency": "EUR" },
            "description": "Order #12345",
            "sequenceType": "oneoff",
            "redirectUrl": "https://webshop.example.org/order/12345/",
            "webhookUrl": "https://webshop.example.org/payments/webhook/",
            "metadata": { "kasapay_order": "ord-1" },
            "profileId": "pfl_QkEhN94Ba",
            "status": "paid",
            "isCancelable": false,
            "createdAt": "2024-03-20T09:13:37+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "dashboard": { "href": "...", "type": "text/html" },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&kasapay_core::PaymentId::issued(
            "tr_5B8cwPMGnU6qLbRvo7qEZo",
        ))
        .await
        .expect("the payment reads back");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount, ten_euro());
    assert_eq!(charge.order.as_ref().map(OrderRef::as_str), Some("ord-1"));
    // A payment that is settled has nowhere left to send anybody.
    assert!(charge.next_action.is_none());
}

/// Mollie's `expired` is neither a refusal nor a withdrawal, and `Status` has
/// no third word for it. The real one stays on the raw body.
#[tokio::test]
async fn an_expired_payment_is_final_and_says_so_in_its_raw_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_KQb4E3WfpA"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": "payment",
            "id": "tr_KQb4E3WfpA",
            "mode": "test",
            "amount": { "value": "10.00", "currency": "EUR" },
            "description": "I can do payments now",
            "status": "expired",
            "createdAt": "2021-12-14T12:53:25+00:00",
            "expiredAt": "2021-12-14T13:08:25+00:00",
            "profileId": "pfl_fFdmWR2qQU",
            "sequenceType": "oneoff",
            "_links": { "self": { "href": "...", "type": "application/hal+json" } }
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&kasapay_core::PaymentId::issued("tr_KQb4E3WfpA"))
        .await
        .expect("the payment reads back");

    assert_eq!(charge.status, Status::Failed);
    assert!(!charge.status.is_open());
    assert_eq!(charge.raw.text_at("/status").as_deref(), Some("expired"));
}

/// Mollie's `cancel-payment` example.
#[tokio::test]
async fn cancelling_a_payment_answers_the_cancelled_payment() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v2/payments/tr_WDqYK6vllg"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": "payment",
            "id": "tr_WDqYK6vllg",
            "mode": "live",
            "amount": { "value": "35.07", "currency": "EUR" },
            "description": "Order 33",
            "method": "banktransfer",
            "sequenceType": "oneoff",
            "redirectUrl": "https://webshop.example.org/order/33/",
            "metadata": null,
            "profileId": "pfl_QkEhN94Ba",
            "status": "canceled",
            "createdAt": "2024-03-20T09:13:37+00:00",
            "canceledAt": "2024-03-20T10:19:15+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "dashboard": { "href": "...", "type": "text/html" },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .cancel(&kasapay_core::PaymentId::issued("tr_WDqYK6vllg"))
        .await
        .expect("the payment cancels");

    assert_eq!(charge.status, Status::Canceled);
    assert_eq!(charge.amount.minor_units(), 3507);
    // A payment created outside kasapay carries no order reference of ours.
    assert!(charge.order.is_none());
}

/// Mollie's `get-capture` example, which is what a created capture answers.
#[tokio::test]
async fn a_capture_is_its_own_object_with_its_own_identifier() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo/captures"))
        .and(body_string_contains(r#""value":"35.95""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "resource": "capture",
            "id": "cpt_vytxeTZskVKR7C7WgdSP3d",
            "mode": "live",
            "description": "Capture for cart #12345",
            "amount": { "currency": "EUR", "value": "35.95" },
            "status": "pending",
            "paymentId": "tr_5B8cwPMGnU6qLbRvo7qEZo",
            "createdAt": "2023-08-02T09:29:56+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let payment = kasapay_core::PaymentId::issued("tr_5B8cwPMGnU6qLbRvo7qEZo");
    let amount = Money::parse("35.95", Currency::Eur).expect("valid amount");
    let capture = client(&server)
        .capture_payment(&payment, Some(amount))
        .await
        .expect("the capture is created");

    assert_eq!(capture.id.as_str(), "cpt_vytxeTZskVKR7C7WgdSP3d");
    assert_eq!(capture.payment, payment);
    assert_eq!(capture.amount, amount);
    assert_eq!(capture.state, CaptureState::Pending);

    // As a Charge: the amount captured rather than the amount authorised, and
    // a status that is the capture's own — Mollie has accepted it and not yet
    // settled it, while the payment still reads `authorized`.
    let charge = capture.into_charge();
    assert_eq!(charge.amount, amount);
    assert_eq!(charge.status, Status::Pending);
    assert_eq!(
        charge.id.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("tr_5B8cwPMGnU6qLbRvo7qEZo")
    );
}

/// A capture with no amount takes the lot, and Mollie's own words for it are
/// "if no amount is provided".
#[tokio::test]
async fn a_capture_for_the_whole_hold_sends_no_amount_at_all() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo/captures"))
        .and(body_string_contains("{}"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "resource": "capture",
            "id": "cpt_vytxeTZskVKR7C7WgdSP3d",
            "mode": "live",
            "amount": { "currency": "EUR", "value": "35.95" },
            "status": "succeeded",
            "paymentId": "tr_5B8cwPMGnU6qLbRvo7qEZo",
            "createdAt": "2023-08-02T09:29:56+00:00",
            "_links": { "self": { "href": "...", "type": "application/hal+json" } }
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .capture(
            &kasapay_core::PaymentId::issued("tr_5B8cwPMGnU6qLbRvo7qEZo"),
            None,
        )
        .await
        .expect("the capture is created");
    assert_eq!(charge.status, Status::Captured);
}

/// Asking for a hold sends `captureMode: manual`, which is the only way Mollie
/// will hold funds and which `ChargeRequest` has no field for.
#[tokio::test]
async fn authorising_asks_mollie_to_hold_the_funds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .and(body_string_contains(r#""captureMode":"manual""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_payment()))
        .mount(&server)
        .await;

    client(&server)
        .authorize(&request(ten_euro()))
        .await
        .expect("the payment opens");
}

/// And a plain charge does not: Mollie captures it the moment the payer
/// finishes.
#[tokio::test]
async fn a_plain_charge_does_not_ask_for_a_hold() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(created_payment()))
        .mount(&server)
        .await;

    client(&server)
        .charge(&request(ten_euro()))
        .await
        .expect("the payment opens");

    let sent = &server.received_requests().await.expect("requests recorded")[0];
    let body = String::from_utf8_lossy(&sent.body);
    assert!(!body.contains("captureMode"), "{body}");
}

/// Mollie's `get-refund` example.
#[tokio::test]
async fn a_fresh_refund_is_not_a_refunded_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo/refunds"))
        .and(body_string_contains(r#""value":"5.95""#))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "resource": "refund",
            "id": "re_4qqhO89gsT",
            "mode": "live",
            "description": "Order",
            "amount": { "currency": "EUR", "value": "5.95" },
            "status": "pending",
            "metadata": "{\"bookkeeping_id\":12345}",
            "paymentId": "tr_5B8cwPMGnU6qLbRvo7qEZo",
            "createdAt": "2023-03-14T17:09:02+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let refund = client(&server)
        .refund(
            &kasapay_core::PaymentId::issued("tr_5B8cwPMGnU6qLbRvo7qEZo"),
            Money::parse("5.95", Currency::Eur).expect("valid amount"),
        )
        .await
        .expect("the refund is created");

    assert_eq!(refund.id.as_str(), "re_4qqhO89gsT");
    assert_eq!(refund.amount.minor_units(), 595);
    // The money has not moved yet, and Mollie says so rather than pretending.
    assert_eq!(refund.state, RefundState::Pending);
    assert_ne!(refund.state, RefundState::Refunded);
}

/// Mollie answers this one 202 with nothing in it, which is why it is not
/// `Provider::cancel`.
#[tokio::test]
async fn releasing_a_hold_is_accepted_rather_than_answered() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(
            "/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo/release-authorization",
        ))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;

    client(&server)
        .release_authorization(&kasapay_core::PaymentId::issued(
            "tr_5B8cwPMGnU6qLbRvo7qEZo",
        ))
        .await
        .expect("Mollie accepted the release");
}

#[tokio::test]
async fn what_the_client_says_it_will_do() {
    let server = MockServer::start().await;
    let capabilities = client(&server).capabilities();
    assert!(capabilities.separate_capture);
    assert!(capabilities.partial_capture);
    assert!(capabilities.partial_refund);
    assert!(capabilities.repeated_refund);
    // Mollie has a vault — mandates — and this crate charges nothing out of
    // it, which is the answer a checkout needs before it offers a saved card.
    assert!(!capabilities.saved_instruments);
    assert_eq!(client(&server).id().as_str(), "mollie");
}
