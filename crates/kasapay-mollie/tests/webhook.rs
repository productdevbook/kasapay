//! Mollie's webhook, which signs nothing and is verified by reading back.
//!
//! The payment bodies are Mollie's own documented examples, the same ones
//! `payments.rs` uses.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, Delivery, ErrorKind, EventKind, IdSource, Secret, Webhook};
use kasapay_mollie::{Config, Mollie};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Mollie {
    let config = Config::at(&server.uri(), Secret::new("test_kasapay")).expect("valid base");
    Mollie::new(config).expect("client builds")
}

/// Mollie's `get-payment` example, cut to what is read here.
fn payment(status: &str) -> serde_json::Value {
    json!({
        "resource": "payment",
        "id": "tr_5B8cwPMGnU6qLbRvo7qEZo",
        "mode": "live",
        "amount": { "value": "10.00", "currency": "EUR" },
        "description": "Order #12345",
        "status": status,
        "createdAt": "2024-03-20T09:13:37+00:00",
    })
}

#[tokio::test]
async fn the_delivery_names_a_payment_and_mollie_says_what_became_of_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment("paid")))
        .mount(&server)
        .await;

    let event = client(&server)
        .verify(&Delivery::new(&[], b"id=tr_5B8cwPMGnU6qLbRvo7qEZo"))
        .await
        .expect("Mollie answers for its own payment");

    assert_eq!(event.kind, EventKind::Captured);
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("tr_5B8cwPMGnU6qLbRvo7qEZo")
    );
    let amount = event.amount.expect("the payment carries its amount");
    assert_eq!(amount.minor_units(), 1000);
    assert_eq!(amount.currency(), Currency::Eur);
    // Mollie names no delivery, so the identifier is composed and says so.
    assert!(matches!(event.id.source(), IdSource::Derived(_)));
    assert_eq!(event.id.as_str(), "tr_5B8cwPMGnU6qLbRvo7qEZo:paid");
}

/// A payment that has not decided yet is a real delivery with no shared word.
#[tokio::test]
async fn a_status_with_no_shared_word_keeps_mollies_own() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(payment("open")))
        .mount(&server)
        .await;

    let event = client(&server)
        .verify(&Delivery::new(&[], b"id=tr_5B8cwPMGnU6qLbRvo7qEZo"))
        .await
        .expect("an undecided payment still verifies");
    assert_eq!(event.kind, EventKind::Other("open".into()));
}

/// Anybody can post to a webhook address; what they cannot do is make Mollie
/// answer for a payment that is not there.
#[tokio::test]
async fn an_identifier_mollie_has_never_heard_of_is_not_an_event() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_invented"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "status": 404,
            "title": "Not Found",
            "detail": "No payment exists with token tr_invented.",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .verify(&Delivery::new(&[], b"id=tr_invented"))
        .await
        .expect_err("Mollie has no such payment");
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn a_body_that_is_not_the_form_mollie_posts_is_malformed() {
    let server = MockServer::start().await;
    // No mock is mounted: nothing should be fetched for a body like this.
    let error = client(&server)
        .verify(&Delivery::new(&[], b"\xff\xfe"))
        .await
        .expect_err("a body that is not UTF-8");
    assert_eq!(error.kind(), ErrorKind::Malformed);

    let error = client(&server)
        .verify(&Delivery::new(&[], b"nothing-here"))
        .await
        .expect_err("a body with no id in it");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// An `id` that is not an identifier never reaches Mollie.
///
/// The posted string becomes a path segment on a request carrying the
/// merchant's key, so the assertion that matters is not the error kind but
/// that no request was made at all.
#[tokio::test]
async fn an_id_that_is_not_an_identifier_opens_no_socket() {
    let server = MockServer::start().await;

    for posted in [
        "../settlements/next",
        "..%2Fsettlements%2Fnext",
        "tr_1/../../v2/settlements/open",
        "tr_1?embed=refunds",
        "tr 1",
        "",
    ] {
        let body = format!("id={posted}");
        let error = client(&server)
            .verify(&Delivery::new(&[], body.as_bytes()))
            .await
            .expect_err("an id that is not an identifier is refused");
        assert_eq!(error.kind(), ErrorKind::Malformed, "for {posted:?}");
    }

    let seen = server.received_requests().await.expect("requests recorded");
    assert!(seen.is_empty(), "refusing must cost no request: {seen:?}");
}

/// A read-back that did not finish is not a delivery that failed to verify.
///
/// Mollie signs nothing, so `verify` asks Mollie what the payment is — and
/// when Mollie answers 429 or 503, nothing was read. The kind has to say so,
/// because the handler's whole decision is `Error::is_retryable`: acknowledge
/// what was read, and let Mollie redeliver what was not. Answering 200 here
/// is a payment taken that the shop never hears about.
#[tokio::test]
async fn a_read_back_that_did_not_finish_is_retryable() {
    for (code, kind) in [(429, ErrorKind::RateLimited), (503, ErrorKind::Provider)] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/payments/tr_5B8cwPMGnU6qLbRvo7qEZo"))
            .respond_with(ResponseTemplate::new(code))
            .mount(&server)
            .await;

        let error = client(&server)
            .verify(&Delivery::new(&[], b"id=tr_5B8cwPMGnU6qLbRvo7qEZo"))
            .await
            .expect_err("Mollie did not answer");

        assert_eq!(error.kind(), kind, "for HTTP {code}");
        assert!(
            error.kind().is_retryable(),
            "HTTP {code} must not be acknowledged as a delivery that was read"
        );
    }
}

/// The other half, so the pair is a rule rather than one example.
///
/// An id Mollie has never heard of *was* read: Mollie answered, and the answer
/// is that there is no such payment. Acknowledging it is right, and
/// `is_retryable` is what says so.
#[tokio::test]
async fn an_id_mollie_does_not_know_is_not_retryable() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/tr_invented"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "status": 404,
            "title": "Not Found",
            "detail": "No payment exists with token tr_invented.",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .verify(&Delivery::new(&[], b"id=tr_invented"))
        .await
        .expect_err("Mollie has no such payment");

    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(!error.kind().is_retryable());
}
