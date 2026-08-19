//! One contract, checked the same way against every adapter in the workspace.
//!
//! `Capabilities` is a promise about behaviour: `separate_capture: false` says
//! this provider has no capture step, and a checkout reads it to decide what
//! to offer before it has a payment. Nothing made the promise and the
//! behaviour agree. Each adapter had its own tests, each asserted its own
//! answers, and a flag flipped in one file while the method below it kept
//! doing what it always did would have passed every one of them.
//!
//! So this asks the same questions of all six clients, through nothing but
//! `dyn Provider`:
//!
//! 1. Where a capability is `false`, the paired call answers `Unsupported`.
//! 2. Where it is `true`, that call does **not** — it goes to the provider,
//!    and what comes back is the provider's answer rather than a refusal.
//! 3. A refusal costs nothing: not one byte reaches the network.
//! 4. Every error names the provider it came from.
//! 5. A charge in a currency the provider settles in is never `Unsupported`.
//!
//! The mock server answers 500 to everything, which is the point: a call that
//! is genuinely implemented fails *there*, and one that is refused never
//! arrives. The two are told apart by counting requests.
//!
//! Six clients, five `ProviderId`s: `classic` and `in_store` are two APIs of
//! iyzico's and both answer `iyzico`, so each subject carries its own label
//! for the assertion messages.

#![cfg(all(
    feature = "iyzico",
    feature = "mollie",
    feature = "paypal",
    feature = "paytr",
    feature = "stripe"
))]
#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay::{ChargeRequest, Currency, ErrorKind, Money, OrderRef, PaymentId, Provider, Secret};
use serde_json::json;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// One adapter, and the server it is pointed at.
struct Subject {
    /// Which client this is — `classic` and `in_store` share a `ProviderId`.
    label: &'static str,
    provider: Box<dyn Provider>,
    server: MockServer,
    /// A currency this provider settles in. Asking one of the Turkish two for
    /// euro, or Mollie for lira, is `Unsupported` on purpose and would say
    /// nothing about whether a charge can be started at all.
    currency: Currency,
    /// Requests the server had already taken before the test began — PayPal
    /// fetches a token when its client is built.
    baseline: usize,
}

impl Subject {
    /// How many requests have reached the server since the client was built.
    async fn reached(&self) -> usize {
        self.server
            .received_requests()
            .await
            .expect("the mock server records requests")
            .len()
            - self.baseline
    }
}

/// A server that answers every call with a 500 it has no body for.
///
/// PayPal is the exception: its client asks for a token before anything else,
/// and a client that will not build cannot be asked about its capabilities.
async fn dead_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "A21AA",
            "token_type": "Bearer",
            "expires_in": 31_668,
        })))
        .mount(&server)
        .await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    server
}

async fn subject(
    label: &'static str,
    provider: Box<dyn Provider>,
    server: MockServer,
    currency: Currency,
) -> Subject {
    let baseline = server
        .received_requests()
        .await
        .expect("the mock server records requests")
        .len();
    Subject {
        label,
        provider,
        server,
        currency,
        baseline,
    }
}

async fn every_adapter() -> Vec<Subject> {
    let mut subjects = Vec::new();

    let server = dead_server().await;
    let config = kasapay::iyzico::classic::Config::new(
        &server.uri(),
        kasapay::iyzico::Credentials::new("api-key", "secret-key"),
    )
    .expect("valid base");
    subjects.push(
        subject(
            "iyzico classic",
            Box::new(kasapay::iyzico::classic::Client::new(config).expect("client builds")),
            server,
            Currency::Try,
        )
        .await,
    );

    let server = dead_server().await;
    let config = kasapay::iyzico::in_store::Config::new(
        &format!("{}/v3/in-store/", server.uri()),
        "api-key",
        "secret-key",
        "merchant-id",
    )
    .expect("valid base");
    subjects.push(
        subject(
            "iyzico in_store",
            Box::new(kasapay::iyzico::in_store::Client::new(config).expect("client builds")),
            server,
            Currency::Try,
        )
        .await,
    );

    let server = dead_server().await;
    let config = kasapay::mollie::Config::at(&server.uri(), Secret::new("test_kasapay"))
        .expect("valid base");
    subjects.push(
        subject(
            "mollie",
            Box::new(kasapay::mollie::Mollie::new(config).expect("client builds")),
            server,
            Currency::Eur,
        )
        .await,
    );

    let server = dead_server().await;
    let config = kasapay::paypal::Config::at(
        &server.uri(),
        Secret::new("client-id"),
        Secret::new("client-secret"),
    )
    .expect("valid base");
    subjects.push(
        subject(
            "paypal",
            Box::new(kasapay::paypal::PayPal::new(config).expect("client builds")),
            server,
            Currency::Eur,
        )
        .await,
    );

    let server = dead_server().await;
    let config = kasapay::paytr::Config::at(
        &server.uri(),
        kasapay::paytr::Credentials::new("merchant-1", "merchant-key", "merchant-salt"),
    )
    .expect("valid base")
    .test_mode();
    subjects.push(
        subject(
            "paytr",
            Box::new(kasapay::paytr::PayTr::new(config).expect("client builds")),
            server,
            Currency::Try,
        )
        .await,
    );

    let server = dead_server().await;
    let stripe = kasapay::stripe::Stripe::at(&server.uri(), &Secret::new("sk_test_kasapay"))
        .expect("client builds");
    subjects.push(subject("stripe", Box::new(stripe), server, Currency::Eur).await);

    subjects
}

fn a_payment() -> PaymentId {
    PaymentId::issued("pay-1")
}

/// `separate_capture: false` says there is no capture step at all — the money
/// was taken at authorisation. A caller reading that flag and skipping the
/// call has to be right, and a caller who calls anyway has to get a refusal
/// rather than a second charge.
#[tokio::test]
async fn capture_answers_the_flag_that_describes_it() {
    for subject in every_adapter().await {
        let who = subject.label;
        let capable = subject.provider.capabilities().separate_capture;
        let error = subject
            .provider
            .capture(&a_payment(), None, None)
            .await
            .expect_err("the server answers 500 to everything it is asked");

        assert_eq!(
            error.provider(),
            subject.provider.id(),
            "{who} answered for somebody else"
        );
        if capable {
            assert_ne!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} says it captures separately and then refuses to"
            );
            assert!(
                subject.reached().await > 0,
                "{who} says it captures separately and never asked"
            );
        } else {
            assert_eq!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} has no capture step and did not say so"
            );
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused a capture only after sending it"
            );
        }
    }
}

/// `lookup_by_order` is read by a crash-recovery path, which is the one place
/// a wrong answer costs a second payment: `Unsupported` there means "ask the
/// provider some other way", and anything else means "you may retry once this
/// says there is no record".
#[tokio::test]
async fn lookup_answers_the_flag_that_describes_it() {
    for subject in every_adapter().await {
        let who = subject.label;
        let capable = subject.provider.capabilities().lookup_by_order;
        let error = subject
            .provider
            .lookup(&OrderRef::new("ord-1"))
            .await
            .expect_err("the server answers 500 to everything it is asked");

        assert_eq!(
            error.provider(),
            subject.provider.id(),
            "{who} answered for somebody else"
        );
        if capable {
            assert_ne!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} says it looks up by order and then refuses to"
            );
            assert!(
                subject.reached().await > 0,
                "{who} says it looks up by order and never asked"
            );
        } else {
            assert_eq!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} cannot look up by order and did not say so"
            );
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused a lookup only after sending it"
            );
        }
    }
}

/// A refusal that costs a request costs a timeout too, in the retry path where
/// that matters most.
#[tokio::test]
async fn a_refused_cancel_never_reaches_the_network() {
    for subject in every_adapter().await {
        let who = subject.label;
        let error = subject
            .provider
            .cancel(&a_payment())
            .await
            .expect_err("the server answers 500 to everything it is asked");
        assert_eq!(
            error.provider(),
            subject.provider.id(),
            "{who} answered for somebody else"
        );
        if error.kind() == ErrorKind::Unsupported {
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused a cancel only after sending it"
            );
        }
    }
}

/// The same, for the one call a checkout makes before it has a payment at all.
#[tokio::test]
async fn a_refused_instrument_list_never_reaches_the_network() {
    for subject in every_adapter().await {
        let who = subject.label;
        let error = subject
            .provider
            .instruments("cus-1")
            .await
            .expect_err("the server answers 500 to everything it is asked");
        assert_eq!(
            error.provider(),
            subject.provider.id(),
            "{who} answered for somebody else"
        );
        if error.kind() == ErrorKind::Unsupported {
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused an instrument list only after sending it"
            );
        }
    }
}

/// `partial_capture` is only meaningful where there is a capture step, and a
/// pair that says otherwise is a caller offering a partial shipment against a
/// provider that takes the money up front.
#[tokio::test]
async fn no_capability_contradicts_another() {
    for subject in every_adapter().await {
        let who = subject.label;
        let capabilities = subject.provider.capabilities();
        assert!(
            capabilities.separate_capture || !capabilities.partial_capture,
            "{who} captures part of an amount it never authorises separately"
        );
        assert!(
            capabilities.partial_refund || !capabilities.repeated_refund,
            "{who} refunds repeatedly but never for part of an amount"
        );
    }
}

/// A charge with nothing but an order and an amount is what a caller writes
/// first, and no provider here takes it — each wants something more. What the
/// refusal may not be is `Unsupported`, which means "never, for anyone": this
/// is "not with these fields", and the difference is whether the caller can
/// fix it. That distinction is the whole of what `Provider::charge` promises.
#[tokio::test]
async fn a_charge_missing_fields_is_an_invalid_request_rather_than_an_unsupported_one() {
    for subject in every_adapter().await {
        let who = subject.label;
        let request = ChargeRequest::builder(
            OrderRef::new("ord-1"),
            Money::parse("10.00", subject.currency).expect("valid amount"),
        )
        .build()
        .expect("valid request");

        let error = subject
            .provider
            .charge(&request)
            .await
            .expect_err("the server answers 500 to everything it is asked");
        assert_eq!(
            error.provider(),
            subject.provider.id(),
            "{who} answered for somebody else"
        );
        assert_ne!(
            error.kind(),
            ErrorKind::Unsupported,
            "{who} cannot start a payment at all through the trait"
        );
    }
}
