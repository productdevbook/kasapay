//! The Stripe adapter against a mock server standing in for the API.
//!
//! `convert`'s unit tests cover the mapping functions in isolation. These cover
//! the part nothing else does: that a `ChargeRequest` becomes the form Stripe
//! expects, and that a PaymentIntent comes back as the right `Charge`.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, IdempotencyKey, Money, NextAction, OrderRef, PaymentId,
    Provider, Status,
};
use kasapay_stripe::{ORDER_METADATA_KEY, RefundState, Stripe};
use serde_json::json;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Stripe {
    // The client builds `{api_base}v1{path}`, so the base has to end in a slash.
    let stripe = stripe::ClientBuilder::new("sk_test_kasapay")
        .url(format!("{}/", server.uri()))
        .build()
        .expect("valid client");
    Stripe::with_client(stripe)
}

/// A PaymentIntent carrying every field `async-stripe` requires.
fn payment_intent(status: &str, client_secret: Option<&str>) -> serde_json::Value {
    json!({
        "id": "pi_kasapay1",
        "object": "payment_intent",
        "allowed_payment_method_types": null,
        "amount": 1999,
        "amount_capturable": 0,
        "amount_received": 0,
        "automatic_payment_methods": null,
        "capture_method": "automatic",
        "client_secret": client_secret,
        "confirmation_method": "automatic",
        "created": 1_770_000_000_i64,
        "currency": "usd",
        "excluded_payment_method_types": null,
        "livemode": false,
        "metadata": { ORDER_METADATA_KEY: "ord-1" },
        "payment_method_configuration_details": null,
        "payment_method_types": ["card"],
        "status": status,
    })
}

fn charge_request() -> ChargeRequest {
    ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("19.99", Currency::Usd).expect("valid amount"),
    )
    .description("one coffee")
    .build()
    .expect("valid request")
}

#[tokio::test]
async fn charge_sends_the_amount_currency_and_order_reference() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(header("authorization", "Bearer sk_test_kasapay"))
        // Minor units on the wire, never 19.99.
        .and(body_string_contains("amount=1999"))
        .and(body_string_contains("currency=usd"))
        .and(body_string_contains("kasapay_order"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment_intent(
            "requires_action",
            Some("pi_kasapay1_secret_x"),
        )))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge(&charge_request())
        .await
        .expect("the intent is created");

    assert_eq!(charge.id, PaymentId::new("pi_kasapay1"));
    assert_eq!(charge.amount.minor_units(), 1999);
    assert_eq!(charge.amount.currency(), Currency::Usd);
    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(
        charge.order.map(|o| o.as_str().to_owned()),
        Some("ord-1".to_owned())
    );
    match charge
        .next_action
        .expect("a stalled intent says what it needs")
    {
        NextAction::ConfirmOnClient { client_secret } => {
            assert_eq!(&*client_secret, "pi_kasapay1_secret_x");
        }
        other => panic!("expected a client-side confirmation, got {other:?}"),
    }
}

#[tokio::test]
async fn a_succeeded_intent_is_captured_and_needs_nothing_further() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment_intent("succeeded", None)))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge(&charge_request())
        .await
        .expect("the intent is created");

    assert_eq!(charge.status, Status::Captured);
    assert!(charge.next_action.is_none());
    assert!(!charge.status.is_open());
}

#[tokio::test]
async fn an_idempotency_key_reaches_stripe_as_a_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(header("idempotency-key", "retry-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment_intent("succeeded", None)))
        .mount(&server)
        .await;

    let request = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("19.99", Currency::Usd).expect("valid amount"),
    )
    .idempotency_key(IdempotencyKey::new("retry-1"))
    .build()
    .expect("valid request");

    client(&server)
        .charge(&request)
        .await
        .expect("the key is accepted");
}

#[tokio::test]
async fn a_refused_card_becomes_a_decline() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(ResponseTemplate::new(402).set_body_json(json!({
            "error": {
                "type": "card_error",
                "code": "card_declined",
                "decline_code": "insufficient_funds",
                "message": "Your card has insufficient funds.",
            }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge(&charge_request())
        .await
        .expect_err("a refused card is not a charge");

    assert_eq!(error.kind(), ErrorKind::Declined);
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn charge_status_reads_an_intent_back() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/payment_intents/pi_kasapay1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_intent("requires_capture", None)),
        )
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&PaymentId::new("pi_kasapay1"))
        .await
        .expect("the intent reads back");

    assert_eq!(charge.status, Status::Authorized);
    assert_eq!(charge.amount.minor_units(), 1999);
}

fn refund_body(amount: i64, status: &str) -> serde_json::Value {
    json!({
        "id": "re_kasapay1",
        "object": "refund",
        "amount": amount,
        "currency": "usd",
        "created": 1_770_000_000_i64,
        "payment_intent": "pi_kasapay1",
        "status": status,
    })
}

#[tokio::test]
async fn a_partial_refund_sends_minor_units_and_the_intent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .and(body_string_contains("amount=500"))
        .and(body_string_contains("payment_intent=pi_kasapay1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(refund_body(500, "succeeded")))
        .mount(&server)
        .await;

    let refund = client(&server)
        .refund(
            &PaymentId::new("pi_kasapay1"),
            Some(Money::parse("5.00", Currency::Usd).expect("valid amount")),
        )
        .await
        .expect("the refund goes through");

    assert_eq!(&*refund.id, "re_kasapay1");
    assert_eq!(refund.amount.minor_units(), 500);
    assert_eq!(refund.amount.currency(), Currency::Usd);
    assert_eq!(refund.status, RefundState::Succeeded);
}

#[tokio::test]
async fn a_full_refund_sends_no_amount_at_all() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(refund_body(1999, "pending")))
        .mount(&server)
        .await;

    let refund = client(&server)
        .refund(&PaymentId::new("pi_kasapay1"), None)
        .await
        .expect("the refund goes through");

    let sent = server.received_requests().await.expect("recorded");
    let body = String::from_utf8(sent[0].body.clone()).expect("utf-8");
    // Sending amount=0 would refund nothing; the field has to be absent.
    assert!(!body.contains("amount="), "unexpected amount in {body}");
    assert_eq!(refund.amount.minor_units(), 1999);
    assert_eq!(refund.status, RefundState::Pending);
}

#[tokio::test]
async fn refunding_in_a_currency_the_payment_was_not_in_is_caught() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        // The caller asked in lira; Stripe applied it to a dollar payment.
        .respond_with(ResponseTemplate::new(200).set_body_json(refund_body(500, "succeeded")))
        .mount(&server)
        .await;

    let error = client(&server)
        .refund(
            &PaymentId::new("pi_kasapay1"),
            Some(Money::parse("5.00", Currency::Try).expect("valid amount")),
        )
        .await
        .expect_err("the currencies do not match");
    assert_eq!(error.kind(), ErrorKind::Malformed);
    // The money has already moved by this point, and the message says so.
    assert!(error.to_string().contains("was not in the currency"));
}

#[tokio::test]
async fn an_unknown_refund_state_is_kept_rather_than_dropped() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .respond_with(ResponseTemplate::new(200).set_body_json(refund_body(500, "reversing")))
        .mount(&server)
        .await;

    let refund = client(&server)
        .refund(
            &PaymentId::new("pi_kasapay1"),
            Some(Money::parse("5.00", Currency::Usd).expect("valid amount")),
        )
        .await
        .expect("the refund goes through");
    assert_eq!(refund.status, RefundState::Other("reversing".into()));
}

#[tokio::test]
async fn cancelling_an_uncaptured_payment_reads_it_back_as_canceled() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents/pi_kasapay1/cancel"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment_intent("canceled", None)))
        .mount(&server)
        .await;

    let charge = client(&server)
        .cancel(&PaymentId::new("pi_kasapay1"))
        .await
        .expect("the payment is withdrawn");
    assert_eq!(charge.status, Status::Canceled);
    assert!(!charge.status.is_open());
}

#[tokio::test]
async fn cancelling_a_captured_payment_is_refused() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents/pi_kasapay1/cancel"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "code": "payment_intent_unexpected_state",
                "message": "You cannot cancel this PaymentIntent because it has a status of succeeded.",
            }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .cancel(&PaymentId::new("pi_kasapay1"))
        .await
        .expect_err("captured money is refunded, not cancelled");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}
