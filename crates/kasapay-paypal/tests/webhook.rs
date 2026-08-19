//! PayPal's webhook, which PayPal verifies itself.
//!
//! **The event bodies here are PayPal's own documented examples** — the
//! `PAYMENT.CAPTURE.COMPLETED` and `CHECKOUT.ORDER.APPROVED` samples from
//! their webhook event reference — cut to the fields this crate reads. The
//! verification answers are the two values their
//! `verify-webhook-signature` operation documents.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, Delivery, ErrorKind, EventKind, IdSource, Secret, Webhook};
use kasapay_paypal::{Config, PayPal, Webhooks};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ACCESS_TOKEN: &str = "A21AAFEpH4PsADK7qSS7pSRsgzfENtu-Q1ysgEDVDESseMHBYXVJYE8ovjj68";
const WEBHOOK_ID: &str = "1JE4291016473214C";

async fn webhooks(server: &MockServer) -> Webhooks {
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": ACCESS_TOKEN,
            "token_type": "Bearer",
            "expires_in": 31_668
        })))
        .mount(server)
        .await;
    let paypal = PayPal::new(
        Config::at(
            &server.uri(),
            Secret::new("client-id"),
            Secret::new("client-secret"),
        )
        .expect("valid base"),
    )
    .expect("client builds");
    Webhooks::new(paypal, WEBHOOK_ID)
}

/// The five headers PayPal signs a delivery with.
fn signed_headers() -> [(&'static str, &'static str); 5] {
    [
        ("PayPal-Auth-Algo", "SHA256withRSA"),
        (
            "PayPal-Cert-Url",
            "https://api.paypal.com/v1/notifications/certs/CERT-360caa42-fca2a594-1d93a270",
        ),
        (
            "PayPal-Transmission-Id",
            "69cd13f0-d67a-11e5-baa3-778b53f4ae55",
        ),
        ("PayPal-Transmission-Sig", "thbaHNjIA9d9lD8...=="),
        ("PayPal-Transmission-Time", "2016-02-18T20:01:35Z"),
    ]
}

const CAPTURE_COMPLETED: &str = r#"{"id":"WH-58D329510W468432D-8HN650336L201105X","event_type":"PAYMENT.CAPTURE.COMPLETED","resource":{"id":"8MC585209K746392H","status":"COMPLETED","amount":{"currency_code":"USD","value":"100.00"},"supplementary_data":{"related_ids":{"order_id":"5O190127TN364715T"}}}}"#;

async fn answering(server: &MockServer, verification: &str) {
    Mock::given(method("POST"))
        .and(path("/v1/notifications/verify-webhook-signature"))
        .and(body_string_contains(WEBHOOK_ID))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "verification_status": verification })),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_delivery_paypal_recognises_is_the_order_it_came_off() {
    let server = MockServer::start().await;
    let webhooks = webhooks(&server).await;
    answering(&server, "SUCCESS").await;

    let headers = signed_headers();
    let event = webhooks
        .verify(&Delivery::new(&headers, CAPTURE_COMPLETED.as_bytes()))
        .await
        .expect("PayPal says this is theirs");

    assert_eq!(event.kind, EventKind::Captured);
    assert_eq!(event.id.as_str(), "WH-58D329510W468432D-8HN650336L201105X");
    assert_eq!(event.id.source(), IdSource::Provider);
    // The capture is not the payment: kasapay names a PayPal payment by its
    // order, and the capture says which order it came off.
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("5O190127TN364715T")
    );
    let amount = event.amount.expect("a capture carries its amount");
    assert_eq!(amount.minor_units(), 10_000);
    assert_eq!(amount.currency(), Currency::Usd);
}

#[tokio::test]
async fn a_delivery_paypal_does_not_recognise_is_refused() {
    let server = MockServer::start().await;
    let webhooks = webhooks(&server).await;
    answering(&server, "FAILURE").await;

    let headers = signed_headers();
    let error = webhooks
        .verify(&Delivery::new(&headers, CAPTURE_COMPLETED.as_bytes()))
        .await
        .expect_err("PayPal says it is not theirs");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// Two of one signed header is not one signed header, and PayPal is never
/// asked to adjudicate: whichever value went out, the other one is still in
/// the delivery and something in front of this believes it signed that.
#[tokio::test]
async fn a_delivery_carrying_a_signed_header_twice_is_refused_before_asking() {
    let server = MockServer::start().await;
    let webhooks = webhooks(&server).await;
    // No verification mock is mounted: asking at all would fail the test.

    let mut headers = signed_headers().to_vec();
    headers.push(("paypal-transmission-sig", "not the one above"));
    let error = webhooks
        .verify(&Delivery::new(&headers, CAPTURE_COMPLETED.as_bytes()))
        .await
        .expect_err("two signatures over one body is not a signature");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
    assert!(error.to_string().contains("arrived 2 times"), "{error}");
}

/// A delivery missing one of the five signed headers never reaches PayPal.
#[tokio::test]
async fn a_delivery_missing_a_signed_header_is_refused_before_asking() {
    let server = MockServer::start().await;
    let webhooks = webhooks(&server).await;
    // No verification mock is mounted: asking at all would fail the test.

    let headers = &signed_headers()[..4];
    let error = webhooks
        .verify(&Delivery::new(headers, CAPTURE_COMPLETED.as_bytes()))
        .await
        .expect_err("four of the five is not a signed delivery");
    assert_eq!(error.kind(), ErrorKind::Untrusted);
}

/// An order event is about the order, and its amounts are a level down.
#[tokio::test]
async fn an_order_event_is_not_a_capture() {
    let server = MockServer::start().await;
    let webhooks = webhooks(&server).await;
    answering(&server, "SUCCESS").await;

    let body = r#"{"id":"WH-2WR32451HC0233532-67976317FL4543714","event_type":"CHECKOUT.ORDER.APPROVED","resource":{"id":"5O190127TN364715T","status":"APPROVED","purchase_units":[{"amount":{"currency_code":"USD","value":"100.00"}}]}}"#;
    let headers = signed_headers();
    let event = webhooks
        .verify(&Delivery::new(&headers, body.as_bytes()))
        .await
        .expect("PayPal says this is theirs");

    // Not Captured: the payer has approved and nobody has taken the money.
    assert_eq!(
        event.kind,
        EventKind::Other("CHECKOUT.ORDER.APPROVED".into())
    );
    assert_eq!(
        event.payment.as_ref().map(kasapay_core::PaymentId::as_str),
        Some("5O190127TN364715T")
    );
    assert!(event.amount.is_none());
}
