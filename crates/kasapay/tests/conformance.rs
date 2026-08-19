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

use kasapay::{
    ChargeRequest, ChargeRequestBuilder, Currency, ErrorKind, IdempotencyKey, InstrumentId, Money,
    OrderRef, PaymentId, Provider, RefundRequest, Secret, Sequence,
};
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
    /// Whether any request this subject sent carries the key, in a header or
    /// in the body.
    ///
    /// Both, because the providers that take one disagree about where it
    /// goes: Stripe and Mollie use an `Idempotency-Key` header and PayPal a
    /// `PayPal-Request-Id`, and a provider that wanted it in the body would be
    /// no less correct.
    async fn sent_key(&self, key: &IdempotencyKey) -> bool {
        let wanted = key.as_str();
        self.server
            .received_requests()
            .await
            .expect("the mock server records requests")
            .iter()
            .any(|request| {
                request
                    .headers
                    .iter()
                    .any(|(_, value)| value.to_str().is_ok_and(|v| v.contains(wanted)))
                    || String::from_utf8_lossy(&request.body).contains(wanted)
            })
    }

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

/// Everything any provider here asks for, so a test can vary one thing and know
/// the answer turned on it. A request missing a field would be refused for the
/// field instead, and would measure nothing.
fn complete_request(currency: Currency) -> ChargeRequestBuilder {
    ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::from_minor_units(1000, currency),
    )
    .description("Order #1")
    .customer("cus-1")
    .return_url("https://merchant.test/ok".parse().expect("valid url"))
    .failure_url("https://merchant.test/no".parse().expect("valid url"))
    .buyer(
        kasapay::Buyer::new("Ayse", "ayse@example.test")
            .surname("Yilmaz")
            .identity_number("11111111111")
            .phone("+905350000000")
            .ip("203.0.113.7")
            .address(kasapay::Address::new("Bagdat Cad. 1", "Istanbul", "Turkey")),
    )
    .item(
        kasapay::BasketItem::new("sku-1", "Kahve", Money::from_minor_units(1000, currency))
            .category("Icecek"),
    )
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

/// A key accepted and dropped reads as a guarantee against taking the money
/// twice where there is none, so an adapter that cannot honour one refuses the
/// call. What this pins is *when*: before the request rather than after it. A
/// capture refused once it has already been sent has already taken the money,
/// and the refusal would be a lie about what happened.
#[tokio::test]
async fn a_capture_refuses_an_idempotency_key_before_it_sends_anything() {
    let key = IdempotencyKey::new("idem-1");
    for subject in every_adapter().await {
        let who = subject.label;
        let error = subject
            .provider
            .capture(&a_payment(), None, Some(&key))
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
                "{who} refused the key only after sending the capture"
            );
        } else {
            assert!(
                subject.sent_key(&key).await,
                "{who} neither refused the key nor sent it"
            );
        }
    }
}

/// An idempotency key is either **sent or refused**, never dropped, on every
/// call that takes one.
///
/// This is the rule `ChargeRequest::idempotency_key`, `Provider::capture` and
/// `Provider::refund` all state, and the reason it is a rule rather than a
/// preference: a caller sets a key, the call times out, `is_retryable()`
/// answers true, and they send it again. If the key reached the provider the
/// retry is free. If it was quietly dropped, the retry is a second payment.
///
/// It is a check rather than a convention because the class has now been found
/// twice — #154 on `capture`'s documentation, #165 and #167 on `charge` and
/// `refund`. Fixing a bug twice is when the rule gets written down.
///
/// # What this does not prove
///
/// That the key was sent *correctly* — only that its value appears somewhere
/// in a request this call made. A provider whose idempotency header is
/// misspelled would pass. It also cannot see a key folded into a signature
/// rather than sent verbatim; no adapter here does that, and one that did
/// would have to be named below rather than left to pass silently.
///
/// **The refund half is weaker than the charge half, and deliberately so.**
/// PayPal reads the order to find the capture and Mollie reads the payment to
/// find what is left, so their first request carries no key and the one that
/// would is never reached against a server that fails everything. Asking for
/// the key on the wire there would fail an adapter that is doing exactly the
/// right thing. So refund asserts refuse-with-nothing-sent or something-sent,
/// and `charge` — where every adapter here is a single call, and where the
/// defect actually was — carries the full check.
#[tokio::test]
async fn an_idempotency_key_is_sent_or_refused_but_never_dropped() {
    let key = IdempotencyKey::new("idem-ratchet-1");

    for subject in every_adapter().await {
        let who = subject.label;
        let request = complete_request(subject.currency)
            .idempotency_key(key.clone())
            .build()
            .expect("valid request");
        let error = subject
            .provider
            .charge(&request)
            .await
            .expect_err("the server answers 500 to everything it is asked");
        if error.kind() == ErrorKind::Unsupported {
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused the key on charge only after sending something"
            );
        } else {
            assert!(
                subject.sent_key(&key).await,
                "{who} neither refused the key on charge nor sent it"
            );
        }
    }

    for subject in every_adapter().await {
        let who = subject.label;
        let request = RefundRequest::builder(a_payment())
            .amount(Money::from_minor_units(500, subject.currency))
            .idempotency_key(key.clone())
            .build()
            .expect("valid request");
        let error = subject
            .provider
            .refund(&request)
            .await
            .expect_err("the server answers 500 to everything it is asked");
        if error.kind() == ErrorKind::Unsupported {
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused the key on refund only after sending something"
            );
        } else {
            assert!(
                subject.reached().await > 0,
                "{who} neither refused the key on refund nor sent anything"
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

/// `resume_by_continuation` is what a caller reads when the payer comes back
/// from a hosted form, to decide between the token it kept and the payment id
/// it may not have. Getting it wrong is a shop that cannot tell whether it was
/// paid.
#[tokio::test]
async fn resume_answers_the_flag_that_describes_it() {
    for subject in every_adapter().await {
        let who = subject.label;
        let capable = subject.provider.capabilities().resume_by_continuation;
        let error = subject
            .provider
            .resume("continuation-1")
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
                "{who} says it resumes by continuation and then refuses to"
            );
            assert!(
                subject.reached().await > 0,
                "{who} says it resumes by continuation and never asked"
            );
        } else {
            assert_eq!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} cannot resume by continuation and did not say so"
            );
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused a resume only after sending it"
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

/// `saved_instruments` is what a checkout reads before it offers "use my saved
/// card". Getting it wrong costs a payer a button that does nothing, or a shop
/// a payment it thinks it took and did not.
///
/// The pair that matters is the one that used to be impossible to state: an
/// adapter handed an instrument either charges it or **refuses**. Ignoring the
/// field and opening a hosted form instead would answer a request to spend a
/// card on file with a redirect nobody is waiting for.
#[tokio::test]
async fn charging_a_saved_instrument_answers_the_flag_that_describes_it() {
    for subject in every_adapter().await {
        let who = subject.label;
        let capable = subject.provider.capabilities().saved_instruments;
        let request = complete_request(subject.currency)
            .instrument(InstrumentId::issued("instrument-1"))
            .sequence(Sequence::Unattended)
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

        if capable {
            assert_ne!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} says it charges a saved instrument and then refuses to"
            );
            assert!(
                subject.reached().await > 0,
                "{who} says it charges a saved instrument and never asked"
            );
        } else {
            assert_eq!(
                error.kind(),
                ErrorKind::Unsupported,
                "{who} cannot charge a saved instrument and did not say so"
            );
            assert_eq!(
                subject.reached().await,
                0,
                "{who} refused a saved instrument only after sending something"
            );
        }
    }
}

/// A request that names no instrument and asks for nothing unusual is the one
/// every caller wrote before any of this existed. It must reach exactly what
/// it reached before.
#[tokio::test]
async fn the_default_sequence_changes_nothing() {
    for subject in every_adapter().await {
        let who = subject.label;
        let request = complete_request(subject.currency)
            .build()
            .expect("valid request");
        let error = subject
            .provider
            .charge(&request)
            .await
            .expect_err("the server answers 500 to everything it is asked");
        assert_ne!(
            error.kind(),
            ErrorKind::Unsupported,
            "{who} refuses an ordinary payment"
        );
    }
}

/// The rule that lets `Currency` be more than a handful: every currency it
/// names is either one this adapter settles in — and the request goes to the
/// provider — or one it refuses **before a socket opens**. What is never
/// allowed is the third answer, mapping an unknown currency onto something and
/// sending it, which is what a wildcard arm used to be forbidden to prevent.
///
/// This is the proof that replaced that prohibition. It costs one pass over a
/// hundred-odd currencies per adapter and it is the only reason widening the
/// enum is safe.
///
/// # What this does not prove
///
/// That the code on the wire is the one asked for. Providers spell them
/// differently — PayTR writes lira as `TL` — so checking the spelling is each
/// adapter's own test, and several have one. What this asserts is the property
/// the enum's own documentation rests on: every currency is either refused
/// before a socket opens or sent to the provider, and never settled in
/// silence.
#[tokio::test]
async fn every_currency_is_either_settled_or_refused_before_the_wire() {
    for subject in every_adapter().await {
        let who = subject.label;
        let mut settled = 0_usize;
        let mut refused = 0_usize;
        let mut before = subject.reached().await;

        for currency in Currency::KNOWN.iter().copied() {
            let request = complete_request(currency).build().expect("valid request");

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

            let after = subject.reached().await;
            if error.kind() == ErrorKind::Unsupported {
                assert_eq!(
                    after, before,
                    "{who} refused {currency} only after sending it"
                );
                refused += 1;
            } else {
                // The half this branch used to leave unasserted. An adapter
                // that settled a currency without sending anything would have
                // counted here and passed, which is exactly the silence the
                // refusal branch above exists to forbid.
                assert!(
                    after > before,
                    "{who} neither refused {currency} nor sent anything for it"
                );
                settled += 1;
            }
            before = after;
        }

        // Not a threshold anybody tuned: every adapter here settles in at
        // least one currency and refuses at least one, so a zero on either
        // side is a mapping that stopped discriminating rather than a
        // provider that changed.
        assert!(settled > 0, "{who} refuses every currency kasapay names");
        assert!(refused > 0, "{who} settles in every currency kasapay names");
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
