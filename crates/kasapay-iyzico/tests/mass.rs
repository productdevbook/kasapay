//! Mass payout against a mock server.
//!
//! The fixtures for the initialization and the reading endpoint are iyzico's
//! own worked examples, copied out of their integration pages in both
//! languages — the same request bodies and the same response bodies, including
//! the commission written with eight decimal places and the amount that does
//! not match what was sent. The one thing changed is `merchantId`, which both
//! examples write as `000000` and which is not a JSON number.
//!
//! `authorize`, `cancel`, `balance` and the single-item read have no worked
//! example on either page, so their fixtures are built from the field names
//! their schemas document and nothing else. The values are stand-ins. No live
//! mass payout account was available to check any of this against — the
//! feature is gated behind iyzico's own approval — and no test here claims
//! otherwise.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{Currency, ErrorKind, Money};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic;
use kasapay_iyzico::mass::{
    Client, ItemFilter, ItemStatus, NewPayout, NewPayoutItem, PayoutError, PayoutRef, PayoutStatus,
    Purpose, Recipient, RecipientKind,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const REQUEST: &str = "79cdefc1-3ce6-4f51-bdd9-0c050274db64";
const REFERENCE: &str = "a97b142a-1e22-4ceb-bafb-38d6c1c4a4f7";
const IBAN: &str = "TR920086402100002353983528";

fn client(server: &MockServer) -> Client {
    let config = classic::Config::new(&server.uri(), Credentials::new("api-key", "secret-key"))
        .expect("valid base");
    Client::new(classic::Client::new(config).expect("client builds"))
}

fn lira(amount: &str) -> Money {
    Money::parse(amount, Currency::Try).expect("a valid amount")
}

/// iyzico's own example payout, all four kinds of recipient.
fn a_payout() -> NewPayout {
    NewPayout::builder("massPayoutId-323ddd", "conversationId-323f")
        .purpose(Purpose::Salary)
        .item(
            NewPayoutItem::builder(
                "ext-65656",
                Recipient::Iban {
                    iban: IBAN.into(),
                    holder: "John Doe".into(),
                },
                lira("100.00"),
                "Payment for services",
            )
            .build()
            .expect("a line iyzico documents"),
        )
        .item(
            NewPayoutItem::builder(
                "ext-9545455",
                Recipient::MemberId("2228853".into()),
                lira("8000.00"),
                "Payment for services",
            )
            .build()
            .expect("a line iyzico documents"),
        )
        .build()
        .expect("a payout iyzico documents")
}

/// The body iyzico's example sends, for the two lines above.
fn the_documented_request() -> serde_json::Value {
    json!({
        "externalId": "massPayoutId-323ddd",
        "conversationId": "conversationId-323f",
        "purpose": "SALARY",
        "items": [
            {
                "itemExternalId": "ext-65656",
                "recipientType": "IBAN",
                "recipientInfo": IBAN,
                "amount": { "value": 100.00, "currency": "TRY" },
                "description": "Payment for services",
                "recipientName": "John Doe",
            },
            {
                "itemExternalId": "ext-9545455",
                "recipientType": "MEMBER_ID",
                "recipientInfo": "2228853",
                "amount": { "value": 8000.00, "currency": "TRY" },
                "description": "Payment for services",
            },
        ],
    })
}

/// iyzico's own example answer to the reading endpoint, byte for byte.
///
/// Sent as raw bytes rather than built with `json!`, because the digits are
/// what is being tested: `20.00000000` through a Rust float literal would
/// arrive as `20.0` and prove nothing. The one change from their page is
/// `merchantId`, which they write `000000` — not a JSON number, so nothing
/// could parse the body at all.
const THE_DOCUMENTED_PAYOUT: &str = r#"{
    "status": "success",
    "systemTime": 1759155442830,
    "massPayout": {
        "externalId": "massPayoutId-323",
        "merchantId": 123456,
        "totalAmount": 201.00,
        "totalSuccessfulAmount": 0,
        "massPayoutStatus": "PUBLISHED_TO_QUEUE",
        "totalCommissionAmount": 0,
        "currency": "TRY"
    },
    "massPayoutItems": {
        "items": [
            {
                "itemExternalId": "ext-0077",
                "referenceCode": "a97b142a-1e22-4ceb-bafb-38d6c1c4a4f7",
                "recipientType": "IBAN",
                "recipientInfo": "TR920086402100002353983528",
                "recipientName": "John Doe",
                "description": "Payment for services",
                "itemStatus": "INIT",
                "errorMessages": [],
                "totalAmount": 100.50,
                "commissionAmount": 20.00000000,
                "currencyCode": "TRY"
            }
        ],
        "page": 0,
        "size": 40,
        "total": 2
    }
}"#;

#[tokio::test]
async fn starting_a_payout_sends_the_body_iyzico_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/init"))
        .and(query_param("locale", "tr"))
        .and(header("content-type", "application/json"))
        .and(header_exists("x-iyzi-rnd"))
        .and(body_json(the_documented_request()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_231_970_716_i64,
            "conversationId": "conversationId-323f",
            "requestId": REQUEST,
        })))
        .mount(&server)
        .await;

    let started = client(&server)
        .start(&a_payout())
        .await
        .expect("the payout is created");

    assert_eq!(&*started.request_id, REQUEST);
    assert_eq!(
        started.conversation_id.as_deref(),
        Some("conversationId-323f")
    );
    assert!(started.invalid_items.is_empty());
}

#[tokio::test]
async fn an_amount_travels_as_the_bare_number_iyzicos_example_writes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "requestId": REQUEST,
        })))
        .mount(&server)
        .await;

    client(&server)
        .start(&a_payout())
        .await
        .expect("the payout is created");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body = String::from_utf8(sent.first().expect("one request").body.clone())
        .expect("the body is utf-8");
    // Unquoted, and with every digit `Money` holds — not `100.5`, not a float
    // that rounded on the way out, and not `"100.00"`.
    assert!(
        body.contains(r#""value":100.00"#),
        "the amount was not written as a bare JSON number: {body}"
    );
    assert!(body.contains(r#""value":8000.00"#), "{body}");
}

#[tokio::test]
async fn the_locale_travels_in_the_query_and_out_of_the_signature() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "requestId": REQUEST,
        })))
        .mount(&server)
        .await;

    client(&server)
        .start(&a_payout())
        .await
        .expect("the payout is created");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let request = sent.first().expect("one request");
    let random_key = request
        .headers
        .get("x-iyzi-rnd")
        .expect("the random key travels in its own header")
        .to_str()
        .expect("ascii");
    let decoded = String::from_utf8(
        BASE64
            .decode(
                request
                    .headers
                    .get("authorization")
                    .expect("an Authorization header")
                    .to_str()
                    .expect("ascii")
                    .strip_prefix("IYZWSv2 ")
                    .expect("the scheme, then one space"),
            )
            .expect("valid base64"),
    )
    .expect("valid utf-8");

    // Mass payout is the one part of the classic API that carries `locale` in
    // the query on a POST, because its request bodies document no such field.
    assert_eq!(request.url.query(), Some("locale=tr"));
    let body = String::from_utf8(request.body.clone()).expect("utf-8");
    assert!(
        !body.contains("locale"),
        "a locale went into the body: {body}"
    );

    let expected = Credentials::new("api-key", "secret-key").signature(
        random_key,
        "/v1/mass/payout/init",
        &body,
    );
    assert!(
        decoded.ends_with(&format!("signature:{expected}")),
        "the signature covered something other than the path and the body"
    );
}

#[tokio::test]
async fn a_success_can_still_carry_lines_iyzico_would_not_take() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/init"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_231_970_716_i64,
            "requestId": REQUEST,
            "invalidItems": [
                {
                    // iyzico names it `externalId` here and `itemExternalId`
                    // on the way in.
                    "externalId": "ext-9545455",
                    "errorCode": "5001",
                    "errorMessage": "Alıcı bulunamadı",
                },
            ],
        })))
        .mount(&server)
        .await;

    let started = client(&server)
        .start(&a_payout())
        .await
        .expect("iyzico calls this a success");

    // It is a success, and one of the two people is not going to be paid.
    assert_eq!(started.invalid_items.len(), 1);
    assert_eq!(
        started.invalid_items[0].external_id.as_deref(),
        Some("ext-9545455")
    );
    assert_eq!(started.invalid_items[0].error_code.as_deref(), Some("5001"));
}

#[tokio::test]
async fn authorizing_names_the_payout_in_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/auth"))
        .and(query_param("locale", "tr"))
        .and(body_json(json!({ "requestId": REQUEST })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_231_995_145_i64,
        })))
        .mount(&server)
        .await;

    client(&server)
        .authorize(REQUEST)
        .await
        .expect("the payout is authorized");
}

#[tokio::test]
async fn cancelling_names_the_payout_in_the_path_and_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v1/mass/payout/cancel/{REQUEST}")))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_231_995_145_i64,
            "locale": "tr",
        })))
        .mount(&server)
        .await;

    client(&server)
        .cancel(REQUEST)
        .await
        .expect("it is stopped");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    // iyzico documents no request body for the cancel, so none is sent.
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn a_balance_arrives_without_a_currency_to_read_it_in() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/mass/payout/balance"))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_231_995_145_i64,
            "balance": 1500.75,
        })))
        .mount(&server)
        .await;

    let balance = client(&server).balance().await.expect("the balance");

    // The digits iyzico sent, and no guess about what they count.
    assert_eq!(balance.amount.as_deref(), Some("1500.75"));
    assert_eq!(
        balance.in_currency(Currency::Try).expect("readable"),
        lira("1500.75")
    );
}

#[tokio::test]
async fn reading_a_payout_reads_iyzicos_own_worked_example() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/retrieve"))
        .and(query_param("locale", "tr"))
        .and(body_json(json!({
            "requestId": REQUEST,
            // Required in Turkish, absent from the English page, sent either way.
            "itemType": "VALID",
            // iyzico's example pages from zero.
            "page": 0,
            "size": 40,
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(THE_DOCUMENTED_PAYOUT, "application/json"),
        )
        .mount(&server)
        .await;

    let payout = client(&server)
        .payout(
            &PayoutRef::Request(REQUEST.into()),
            ItemFilter::Valid,
            0,
            40,
        )
        .await
        .expect("the payout");

    let summary = payout.summary.expect("a summary");
    assert_eq!(summary.status, Some(PayoutStatus::PublishedToQueue));
    assert_eq!(summary.currency, Some(Currency::Try));
    assert_eq!(summary.total_amount, Some(lira("201.00")));
    assert_eq!(summary.total_successful_amount, Some(lira("0.00")));
    assert_eq!(summary.merchant_id, Some(123_456));

    assert_eq!(payout.items.len(), 1);
    let item = &payout.items[0];
    assert_eq!(&*item.reference_code, REFERENCE);
    assert_eq!(item.recipient_kind, Some(RecipientKind::Iban));
    assert_eq!(item.recipient_name.as_deref(), Some("John Doe"));
    assert_eq!(item.status, Some(ItemStatus::Init));
    // The line was sent as 100.00 and comes back as 100.50, with a commission
    // of 20. iyzico's own example does that, and nothing here reconciles it.
    assert_eq!(item.total_amount, Some(lira("100.50")));
    // Eight decimal places on a two-decimal currency: exactly twenty lira.
    assert_eq!(item.commission_amount, Some(lira("20.00")));

    // The example nests the paging inside `massPayoutItems`; the schema puts
    // it at the top level. Both are read.
    assert_eq!(payout.page, Some(0));
    assert_eq!(payout.size, Some(40));
    assert_eq!(payout.total, Some(2));
}

#[tokio::test]
async fn paging_at_the_top_level_is_read_too() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/retrieve"))
        .and(body_json(json!({
            "externalMassPayoutId": "massPayoutId-323",
            "itemType": "INVALID",
            "page": 1,
            "size": 10,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            // Where the schema says they are.
            "page": 1,
            "size": 10,
            "total": 14,
            "massPayoutItems": { "items": [] },
        })))
        .mount(&server)
        .await;

    let payout = client(&server)
        .payout(
            &PayoutRef::External("massPayoutId-323".into()),
            ItemFilter::Invalid,
            1,
            10,
        )
        .await
        .expect("the payout");

    assert_eq!(payout.page, Some(1));
    assert_eq!(payout.total, Some(14));
    assert!(payout.items.is_empty());
    assert!(payout.summary.is_none());
}

#[tokio::test]
async fn one_line_is_read_by_the_reference_code_in_its_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/mass/payout/retrieve/items/{REFERENCE}")))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_759_155_442_830_i64,
            "locale": "tr",
            "item": {
                "itemExternalId": "ext-0077",
                "referenceCode": REFERENCE,
                "recipientType": "IBAN",
                "recipientInfo": IBAN,
                "recipientName": "John Doe",
                "description": "Payment for services",
                "itemStatus": "DEPOSIT_SUCCESS",
                "errorMessages": ["Hesap kapalı"],
                "totalAmount": "100.50",
                "commissionAmount": "20.00000000",
                "currencyCode": "TRY",
            },
        })))
        .mount(&server)
        .await;

    let item = client(&server).item(REFERENCE).await.expect("the line");

    // A line that reached DEPOSIT_SUCCESS is a line that failed: the money
    // went back to the merchant, not to John Doe.
    assert_eq!(item.status, Some(ItemStatus::DepositSuccess));
    assert_eq!(item.error_messages.len(), 1);
    // Quoted here and a JSON number in the listing. Both read.
    assert_eq!(item.total_amount, Some(lira("100.50")));
    assert_eq!(item.commission_amount, Some(lira("20.00")));

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn a_line_in_a_currency_kasapay_cannot_name_stays_in_the_raw_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/mass/payout/retrieve/items/{REFERENCE}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "item": {
                "referenceCode": REFERENCE,
                "itemStatus": "SUCCESS",
                "totalAmount": 100.50,
                "currencyCode": "SEK",
            },
        })))
        .mount(&server)
        .await;

    let item = client(&server).item(REFERENCE).await.expect("the line");

    assert_eq!(item.currency, None);
    assert_eq!(item.total_amount, None);
    assert_eq!(item.raw.text_at("/currencyCode").as_deref(), Some("SEK"));
}

#[tokio::test]
async fn a_refusal_that_documents_no_error_code_still_refuses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/mass/payout/auth"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            // Every mass payout schema documents `failure` and no error field
            // to go with it, in either language.
            "status": "failure",
            "systemTime": 1_759_231_995_145_i64,
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .authorize(REQUEST)
        .await
        .expect_err("a failure status is not an authorization");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), None);
}

#[tokio::test]
async fn a_failure_that_does_carry_a_code_keeps_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/mass/payout/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5088",
            "errorMessage": "Toplu ödeme yetkiniz bulunmamaktadır",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .balance()
        .await
        .expect_err("a failure status is not a balance");

    assert_eq!(error.code(), Some("5088"));
}

#[tokio::test]
async fn an_identifier_that_would_change_the_path_never_reaches_the_wire() {
    let server = MockServer::start().await;

    let error = client(&server)
        .cancel("79cdefc1/../../auth")
        .await
        .expect_err("a tracking id is one path segment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let error = client(&server)
        .item("a97b142a?locale=en")
        .await
        .expect_err("a reference code is one path segment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let error = client(&server)
        .authorize("  ")
        .await
        .expect_err("a payout is authorized by its tracking id");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "a hostile identifier opened a socket");
}

#[test]
fn every_currency_iyzico_does_not_document_a_payout_in_is_refused_by_the_builder() {
    // iyzico's Turkish page names TRY, USD and EUR; the English one names
    // none at all. Six of the nine `Currency` names are refused before a
    // socket opens, sterling and francs among them.
    for currency in [
        Currency::Gbp,
        Currency::Jpy,
        Currency::Kwd,
        Currency::Rub,
        Currency::Chf,
        Currency::Nok,
    ] {
        let error = NewPayoutItem::builder(
            "ext-1",
            Recipient::MemberId("2228853".into()),
            Money::from_minor_units(999, currency),
            "Payment for services",
        )
        .build()
        .expect_err("iyzico documents no mass payout in this");
        assert_eq!(error, PayoutError::UnsupportedCurrency(currency));
    }

    for currency in [Currency::Try, Currency::Usd, Currency::Eur] {
        assert!(
            NewPayoutItem::builder(
                "ext-1",
                Recipient::MemberId("2228853".into()),
                Money::from_minor_units(999, currency),
                "Payment for services",
            )
            .build()
            .is_ok(),
            "{currency} is a currency iyzico documents a mass payout in"
        );
    }
}

#[test]
fn a_payout_iyzico_would_refuse_never_leaves_the_builder() {
    // iyzico documents at least one item.
    assert_eq!(
        NewPayout::builder("massPayoutId-1", "conversationId-1")
            .build()
            .expect_err("a payout of nobody is not a payout"),
        PayoutError::NoItems
    );
    // Both identifiers are required by the English page.
    assert!(
        NewPayout::builder("", "conversationId-1")
            .item(
                NewPayoutItem::builder(
                    "ext-1",
                    Recipient::Phone("+905555856935".into()),
                    lira("70.00"),
                    "Payment for services",
                )
                .build()
                .expect("valid"),
            )
            .build()
            .is_err()
    );
    // An amount of nothing pays nobody.
    assert!(
        NewPayoutItem::builder(
            "ext-1",
            Recipient::Phone("+905555856935".into()),
            Money::from_minor_units(0, Currency::Try),
            "Payment for services",
        )
        .build()
        .is_err()
    );
    // An identifier that names nobody.
    assert_eq!(
        NewPayoutItem::builder(
            "ext-1",
            Recipient::Iban {
                iban: "   ".into(),
                holder: "John Doe".into(),
            },
            lira("70.00"),
            "Payment for services",
        )
        .build()
        .expect_err("an empty IBAN names no account"),
        PayoutError::Blank("recipientInfo")
    );
}

#[test]
fn an_iban_cannot_be_sent_without_the_name_iyzico_requires() {
    // Not a test of a check — a test that there is no check to fail. iyzico
    // makes `recipientName` mandatory when and only when the type is IBAN, and
    // `Recipient::Iban` has nowhere to put an IBAN without one.
    let item = NewPayoutItem::builder(
        "ext-65656",
        Recipient::Iban {
            iban: IBAN.into(),
            holder: "John Doe".into(),
        },
        lira("100.00"),
        "Payment for services",
    )
    .build()
    .expect("valid");
    assert_eq!(item.recipient.name(), Some("John Doe"));
    assert_eq!(item.recipient.kind(), RecipientKind::Iban);
    // And the three that iyzico asks no name for carry none.
    for recipient in [
        Recipient::Phone("+905555856935".into()),
        Recipient::MemberId("2228853".into()),
        Recipient::IdentityNumber("2222222817".into()),
    ] {
        assert_eq!(recipient.name(), None, "{}", recipient.kind_str());
    }
}

#[test]
fn a_mixed_currency_payout_is_built_and_says_that_it_is_mixed() {
    // iyzico puts a currency on every line and one on the payout that comes
    // back, and never says what happens when the lines disagree. Nothing here
    // refuses it; `currencies` is how a caller refuses it themselves.
    let payout = NewPayout::builder("massPayoutId-1", "conversationId-1")
        .item(
            NewPayoutItem::builder(
                "ext-1",
                Recipient::MemberId("2228853".into()),
                lira("70.00"),
                "Payment for services",
            )
            .build()
            .expect("valid"),
        )
        .item(
            NewPayoutItem::builder(
                "ext-2",
                Recipient::MemberId("2228854".into()),
                Money::parse("70.00", Currency::Eur).expect("valid"),
                "Payment for services",
            )
            .build()
            .expect("valid"),
        )
        .build()
        .expect("iyzico documents nothing that refuses this");

    assert_eq!(payout.currencies(), vec![Currency::Try, Currency::Eur]);
}
