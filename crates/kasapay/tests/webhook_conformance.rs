//! One rule about deliveries, checked the same way against every verifier.
//!
//! `conformance.rs` walks `Provider`. `Webhook` is the other half of the
//! shared vocabulary — four crates implement it — and nothing walked it, so
//! money-safety §7's first rule held by convention in four separate files:
//!
//! > an unsigned or wrongly-signed delivery never becomes an `Event`.
//!
//! That is the rule here, and the assertion is deliberately weak: **`verify`
//! does not answer `Ok`**. Not which error, not which kind. Each verifier
//! refuses for its own reason and two of them have no signature to check at
//! all, so anything stronger would be four rules wearing one name.
//!
//! # What this does not prove
//!
//! Nothing about a delivery that *is* signed. Producing one means signing it
//! the way each provider does — Stripe's HMAC over the timestamped body, PayTR's
//! over three form fields, PayPal's by asking PayPal — and each crate's own
//! `tests/webhook.rs` covers that with fixtures traceable to the provider.
//! What this covers is the case those files each had to remember: a delivery
//! carrying nothing that vouches for it must not become an event, and one
//! carrying two contradictory signature headers must not be resolved to
//! either.
//!
//! It also says nothing about `EventKind::Other`, which needs a valid
//! signature to reach.

#![cfg(all(
    feature = "mollie",
    feature = "paypal",
    feature = "paytr",
    feature = "stripe"
))]
#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

mod source_tree;

use kasapay::{Delivery, Secret, Webhook};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// One verifier, the body it would otherwise read, and what signs it.
struct Verifier {
    label: &'static str,
    webhook: Box<dyn Webhook>,
    /// The header a signature arrives in, where it arrives in one. `None` for
    /// PayTR, which signs three fields of the body, and for Mollie, which
    /// signs nothing and reads the payment back instead.
    signature_header: Option<&'static str>,
    /// A body shaped the way this provider posts one, so the refusal is about
    /// the signature rather than about the parse.
    body: &'static [u8],
    /// Kept alive for the two verifiers that make a call while verifying.
    _server: Option<MockServer>,
}

/// A server that answers 500 to everything, so a verifier that reaches the
/// network fails there rather than being told a payment is real.
async fn dead_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    server
}

async fn every_verifier() -> Vec<Verifier> {
    let stripe = Verifier {
        label: "stripe",
        webhook: Box::new(kasapay::stripe::Webhooks::new(Secret::new("whsec_kasapay"))),
        signature_header: Some("Stripe-Signature"),
        body: br#"{"id":"evt_1","type":"payment_intent.succeeded","data":{"object":{}}}"#,
        _server: None,
    };

    let server = dead_server().await;
    let paypal = kasapay::paypal::PayPal::new(
        kasapay::paypal::Config::at(&server.uri(), Secret::new("id"), Secret::new("secret"))
            .expect("valid base"),
    )
    .expect("client builds");
    let paypal = Verifier {
        label: "paypal",
        webhook: Box::new(kasapay::paypal::Webhooks::new(paypal, "WH-KASAPAY")),
        signature_header: Some("PAYPAL-TRANSMISSION-SIG"),
        body: br#"{"id":"WH-1","event_type":"PAYMENT.CAPTURE.COMPLETED","resource":{}}"#,
        _server: Some(server),
    };

    let server = dead_server().await;
    let mollie = Verifier {
        label: "mollie",
        webhook: Box::new(
            kasapay::mollie::Mollie::new(
                kasapay::mollie::Config::at(&server.uri(), Secret::new("test_kasapay"))
                    .expect("valid base"),
            )
            .expect("client builds"),
        ),
        signature_header: None,
        body: b"id=tr_5B8cwPMGnU6qLbRvo7qEZo",
        _server: Some(server),
    };

    let paytr = Verifier {
        label: "paytr",
        webhook: Box::new(
            kasapay::paytr::PayTr::new(
                kasapay::paytr::Config::at(
                    "https://paytr.test",
                    kasapay::paytr::Credentials::new("merchant-1", "merchant-key", "merchant-salt"),
                )
                .expect("valid base"),
            )
            .expect("client builds"),
        ),
        signature_header: None,
        body: b"merchant_oid=ord-1&status=success&total_amount=14990&hash=bm90LWEtaGFzaA%3D%3D",
        _server: None,
    };

    vec![stripe, paypal, mollie, paytr]
}

/// A delivery nobody signed does not become an event.
///
/// Each refuses for its own reason, and the reasons are worth naming. Stripe
/// and PayPal have no signature header to check. PayTR's body carries a hash
/// that is not the one its key produces. Mollie signs nothing at all, so the
/// answer has to come from Mollie — and against a server that answers 500,
/// it does not come. In none of the four does the delivery's own contents
/// decide anything, which is the rule.
#[tokio::test]
async fn a_delivery_nobody_signed_is_never_an_event() {
    for subject in every_verifier().await {
        let delivery = Delivery::new(&[], subject.body);
        let verified = subject.webhook.verify(&delivery).await;
        assert!(
            verified.is_err(),
            "{} read an event out of a delivery nothing vouched for",
            subject.label
        );
    }
}

/// Two signature headers are refused rather than resolved to one.
///
/// Two claims about one delivery mean something in front of the verifier read
/// it differently, and picking either is picking whichever the attacker put
/// where this code looks. `Delivery::signed_header` is the shared refusal;
/// this asserts the verifiers that read a header actually use it.
#[tokio::test]
async fn two_signature_headers_are_refused() {
    let mut checked = 0;
    for subject in every_verifier().await {
        let Some(header) = subject.signature_header else {
            continue;
        };
        let headers = [(header, "one"), (header, "two")];
        let delivery = Delivery::new(&headers, subject.body);
        let verified = subject.webhook.verify(&delivery).await;
        assert!(
            verified.is_err(),
            "{} resolved two signature headers instead of refusing them",
            subject.label
        );
        checked += 1;
    }
    assert_eq!(
        checked, 2,
        "the two header-signed verifiers are Stripe and PayPal; \
         if a third arrived it belongs in this walk"
    );
}

/// Every `Webhook` in the workspace is one of the four walked above.
///
/// The same reason `conformance.rs` counts its own roster: the list is
/// complete because somebody remembered. A fifth verifier would get
/// money-safety §7 by convention, which is what this file exists to stop.
///
/// # What this does not prove
///
/// Only that the counts agree — it prints both lists and leaves *which* to a
/// reader — and it cannot see a `Webhook` implemented outside this workspace.
#[tokio::test]
async fn every_webhook_in_the_workspace_is_walked() {
    let written = source_tree::implementations_of("Webhook");
    let walked: Vec<&str> = every_verifier().await.iter().map(|v| v.label).collect();
    assert_eq!(
        written.len(),
        walked.len(),
        "{} `impl Webhook` in the workspace, {} walked by this file.\n\nwritten:\n  {}\n\nwalked:\n  {}",
        written.len(),
        walked.len(),
        written.join("\n  "),
        walked.join("\n  "),
    );
}
