//! Saved cards against a mock server standing in for Stripe's API.
//!
//! The fixtures come from Stripe's own documentation: the `PaymentMethod` shape
//! is the example object from `openapi/fixtures3.json` in
//! `stripe/openapi`, and the `authentication_required` error body is the
//! literal example on <https://docs.stripe.com/payments/without-card-authentication>.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, InstrumentId, Money, OrderRef, Provider, Status};
use kasapay_stripe::Stripe;
use kasapay_stripe::saved::{Brand, Funding, Payment};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Stripe {
    // The client builds `{api_base}v1{path}`, so the base has to end in a slash.
    let stripe = stripe::ClientBuilder::new("sk_test_kasapay")
        .url(format!("{}/", server.uri()))
        .build()
        .expect("valid client");
    Stripe::with_client(stripe)
}

/// Stripe's own documented example `PaymentMethod` object, trimmed to what
/// `stripe_shared::PaymentMethod` requires plus the card fields this crate
/// reads — from `openapi/fixtures3.json` in `stripe/openapi`.
fn payment_method(id: &str, brand: &str, last4: &str) -> serde_json::Value {
    json!({
        "id": id,
        "object": "payment_method",
        "billing_details": {
            "address": { "city": "San Francisco", "country": "US" },
            "email": "jenny@example.com",
        },
        "card": {
            "brand": brand,
            "country": "US",
            "exp_month": 8,
            "exp_year": 2030,
            "funding": "credit",
            "last4": last4,
        },
        "created": 1_770_000_000_i64,
        "customer": "cus_kasapay1",
        "livemode": false,
        "metadata": { "order_id": "123456789" },
        "type": "card",
    })
}

/// One page of Stripe's list envelope.
fn payment_method_list(data: &[serde_json::Value], has_more: bool) -> serde_json::Value {
    json!({
        "object": "list",
        "url": "/v1/customers/cus_kasapay1/payment_methods",
        "has_more": has_more,
        "data": data,
    })
}

#[tokio::test]
async fn stored_cards_lists_a_customers_cards_without_a_card_number_in_sight() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .and(query_param("type", "card"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_method_list(
                &[
                    payment_method("pm_1Pgc75B7WZ01zgkWlHVgdEGJ", "visa", "4242"),
                    payment_method("pm_2Pgc75B7WZ01zgkWlHVgdEGJ", "mastercard", "4444"),
                ],
                false,
            )),
        )
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("cus_kasapay1")
        .await
        .expect("the cards list");

    assert_eq!(cards.len(), 2);
    assert_eq!(
        cards[0].token,
        InstrumentId::issued("pm_1Pgc75B7WZ01zgkWlHVgdEGJ")
    );
    assert_eq!(cards[0].brand, Brand::Visa);
    assert_eq!(&*cards[0].last_four, "4242");
    assert_eq!(cards[0].funding, Funding::Credit);
    assert_eq!(cards[0].exp_month, 8);
    assert_eq!(cards[0].exp_year, 2030);
    assert_eq!(cards[1].brand, Brand::Mastercard);

    // Nothing in the request or the fixtures above is shaped like a PAN.
    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent[0].body.is_empty(), "a GET carries no body");
}

#[tokio::test]
async fn stored_cards_keeps_a_brand_it_does_not_recognise_rather_than_dropping_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_method_list(
                &[payment_method(
                    "pm_1Pgc75B7WZ01zgkWlHVgdEGJ",
                    "some_future_network",
                    "4242",
                )],
                false,
            )),
        )
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("cus_kasapay1")
        .await
        .expect("the cards list");
    assert_eq!(cards[0].brand, Brand::Other("some_future_network".into()));
}

/// `Provider::instruments` is `Stripe::stored_cards` behind the shared trait,
/// with the brand and last four turned into a label.
#[tokio::test]
async fn provider_instruments_lists_the_same_cards() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .and(query_param("type", "card"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_method_list(
                &[payment_method(
                    "pm_1Pgc75B7WZ01zgkWlHVgdEGJ",
                    "visa",
                    "4242",
                )],
                false,
            )),
        )
        .mount(&server)
        .await;

    let instruments = client(&server)
        .instruments("cus_kasapay1")
        .await
        .expect("the list reads back");

    assert_eq!(instruments.len(), 1);
    assert_eq!(
        instruments[0].id,
        InstrumentId::issued("pm_1Pgc75B7WZ01zgkWlHVgdEGJ")
    );
    assert_eq!(instruments[0].label.as_deref(), Some("visa •••• 4242"));
}

#[tokio::test]
async fn a_customer_with_no_saved_cards_is_an_empty_list_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment_method_list(&[], false)))
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("cus_kasapay1")
        .await
        .expect("no cards is a valid answer");
    assert!(cards.is_empty());
}

#[tokio::test]
async fn stored_cards_follows_the_cursor_rather_than_stopping_at_the_first_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .and(query_param_is_missing("starting_after"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_method_list(
                &[payment_method("pm_page1", "visa", "4242")],
                true,
            )),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/customers/cus_kasapay1/payment_methods"))
        .and(query_param("starting_after", "pm_page1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(payment_method_list(
                &[payment_method("pm_page2", "mastercard", "4444")],
                false,
            )),
        )
        .mount(&server)
        .await;

    let cards = client(&server)
        .stored_cards("cus_kasapay1")
        .await
        .expect("both pages read back");

    // Stopping at the first page would have missed the second card.
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[1].token, InstrumentId::issued("pm_page2"));
}

fn saved_payment(off_session: bool) -> Payment {
    let builder = Payment::builder(
        OrderRef::new("ord-1"),
        Money::parse("19.99", Currency::Usd).expect("valid amount"),
        "cus_kasapay1",
        InstrumentId::issued("pm_1Pgc75B7WZ01zgkWlHVgdEGJ"),
    );
    let builder = if off_session {
        builder.off_session()
    } else {
        builder
    };
    builder.build().expect("valid saved-card payment")
}

/// A confirmed PaymentIntent, carrying every field `async-stripe` requires.
fn confirmed_intent(status: &str) -> serde_json::Value {
    json!({
        "id": "pi_kasapay1",
        "object": "payment_intent",
        "amount": 1999,
        "amount_capturable": 0,
        "amount_received": if status == "succeeded" { 1999 } else { 0 },
        "capture_method": "automatic",
        "client_secret": null,
        "confirmation_method": "automatic",
        "created": 1_770_000_000_i64,
        "currency": "usd",
        "livemode": false,
        "metadata": { "kasapay_order": "ord-1" },
        "payment_method_types": ["card"],
        "status": status,
    })
}

#[tokio::test]
async fn charging_a_saved_card_confirms_a_payment_intent_and_capabilities_say_so() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(body_string_contains("customer=cus_kasapay1"))
        .and(body_string_contains(
            "payment_method=pm_1Pgc75B7WZ01zgkWlHVgdEGJ",
        ))
        .and(body_string_contains("confirm=true"))
        .and(body_string_contains("kasapay_order"))
        .respond_with(ResponseTemplate::new(200).set_body_json(confirmed_intent("succeeded")))
        .mount(&server)
        .await;

    let stripe = client(&server);
    // A capability that says yes over a call that then fails is a bug in the
    // adapter, so the two are asserted together — the same thing iyzico's
    // saved-card test does.
    assert!(stripe.capabilities().saved_instruments);
    let charge = stripe
        .charge_saved_card(&saved_payment(false))
        .await
        .expect("the saved card is charged");

    assert_eq!(charge.status, Status::Captured);
    assert_eq!(charge.amount.minor_units(), 1999);
}

#[tokio::test]
async fn off_session_reaches_stripe_only_when_the_caller_asks_for_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(confirmed_intent("succeeded")))
        .mount(&server)
        .await;

    client(&server)
        .charge_saved_card(&saved_payment(false))
        .await
        .expect("the charge goes through");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body = String::from_utf8(sent[0].body.clone()).expect("utf-8");
    assert!(
        !body.contains("off_session"),
        "off_session was sent when the caller never asked for it: {body}"
    );
}

#[tokio::test]
async fn asking_for_off_session_sends_it_on_the_wire() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(body_string_contains("off_session=true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(confirmed_intent("succeeded")))
        .mount(&server)
        .await;

    client(&server)
        .charge_saved_card(&saved_payment(true))
        .await
        .expect("the off-session charge goes through");
}

#[tokio::test]
async fn an_off_session_charge_that_needs_authentication_is_an_error_not_a_stalled_charge() {
    let server = MockServer::start().await;
    // Stripe's own documented example, from
    // https://docs.stripe.com/payments/without-card-authentication — the page
    // that covers charging a saved card without the payer present. The nested
    // `payment_intent` on their example is left out: `async-stripe`'s
    // `PaymentIntent` needs fields that page's trimmed example does not carry,
    // and nothing under test reads it.
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(ResponseTemplate::new(402).set_body_json(json!({
            "error": {
                "code": "authentication_required",
                "decline_code": "authentication_not_handled",
                "doc_url": "https://docs.stripe.com/error-codes#authentication-required",
                "message": "This payment required an authentication action to complete, but \
                    `error_on_requires_action` was set. When you're ready, you can upgrade your \
                    integration to handle actions at \
                    https://stripe.com/docs/payments/payment-intents/upgrade-to-handle-actions.",
                "type": "card_error",
            }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_saved_card(&saved_payment(true))
        .await
        .expect_err("a card needing authentication is not a Charge");

    // A card_error is Declined, the same as every other card_error this crate
    // reads — there is no ErrorKind that means "try again with a challenge".
    assert_eq!(error.kind(), ErrorKind::Declined);
    assert_eq!(error.code(), Some("authentication_not_handled"));
    assert!(error.to_string().contains("authentication action"));
}

#[tokio::test]
async fn forgetting_a_card_sends_no_body_and_no_card_data() {
    let server = MockServer::start().await;
    // Detach's own answer: the same PaymentMethod, no longer on a customer.
    let mut detached = payment_method("pm_1Pgc75B7WZ01zgkWlHVgdEGJ", "visa", "4242");
    detached["customer"] = serde_json::Value::Null;
    Mock::given(method("POST"))
        .and(path(
            "/v1/payment_methods/pm_1Pgc75B7WZ01zgkWlHVgdEGJ/detach",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(detached))
        .mount(&server)
        .await;

    client(&server)
        .forget_card(&InstrumentId::issued("pm_1Pgc75B7WZ01zgkWlHVgdEGJ"))
        .await
        .expect("the card is forgotten");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(
        sent[0].body.is_empty(),
        "detach takes no parameters this crate sets"
    );
}

#[tokio::test]
async fn forgetting_a_card_stripe_does_not_recognise_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_methods/pm_gone/detach"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": {
                "type": "invalid_request_error",
                "code": "resource_missing",
                "message": "No such PaymentMethod: 'pm_gone'",
            }
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .forget_card(&InstrumentId::issued("pm_gone"))
        .await
        .expect_err("a card Stripe does not hold cannot be forgotten");
    assert_eq!(error.kind(), ErrorKind::NotFound);
}
