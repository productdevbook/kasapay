//! PayPOS's softpos services against a mock server.
//!
//! Same position as `tests/agent.rs`: none of `init_sale_transaction`,
//! `init_reversal_transaction` or `check_transaction` has a worked example on
//! either language's page, so every fixture here is a stand-in built from the
//! field names PayPOS documents. No live PayPOS account was available to
//! check any of it against.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, Money};
use kasapay_iyzico::softpos::{Client, Config, InitReversal, InitSale};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    let config = Config::new(&format!("{}/", server.uri())).expect("valid base");
    Client::new(config, "session-abc").expect("client builds")
}

fn lira(amount: &str) -> Money {
    Money::parse(amount, Currency::Try).expect("a valid amount")
}

#[tokio::test]
async fn starting_a_sale_sends_the_session_key_and_the_amount_as_a_bare_number() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/init_sale_transaction"))
        .and(header("session-key", "session-abc"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "amount": 149.90,
            "reference_no": "ref-1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "payment_session_id": "ps-1",
            "deeplink_url": "paypos://start?session=ps-1",
            "encryption_key": "enc-key-1",
            "object_name": "InitSale",
            "code": 0,
            "message": "Success",
        })))
        .mount(&server)
        .await;

    let flow = client(&server)
        .init_sale_transaction(&InitSale::new(lira("149.90")).reference_no("ref-1"))
        .await
        .expect("the sale starts");

    assert_eq!(flow.payment_session_id.as_deref(), Some("ps-1"));
    assert_eq!(
        flow.deeplink_url.as_deref(),
        Some("paypos://start?session=ps-1")
    );
    assert_eq!(flow.encryption_key.as_deref(), Some("enc-key-1"));

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body = String::from_utf8(sent.first().expect("one request").body.clone())
        .expect("the body is utf-8");
    assert!(
        body.contains(r#""amount":149.90"#),
        "the amount was not written as a bare JSON number: {body}"
    );
}

#[tokio::test]
async fn a_currency_that_is_not_lira_is_refused_before_a_socket_opens() {
    let server = MockServer::start().await;

    let error = client(&server)
        .init_sale_transaction(&InitSale::new(
            Money::parse("10.00", Currency::Usd).expect("valid"),
        ))
        .await
        .expect_err("softpos is not documented in dollars");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "an unsupported currency opened a socket");
}

#[tokio::test]
async fn starting_a_reversal_names_the_transaction() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/init_reversal_transaction"))
        .and(header("session-key", "session-abc"))
        .and(body_json(json!({ "xact_id": "xact-1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "payment_session_id": "ps-2",
            "deeplink_url": "paypos://reverse?session=ps-2",
            "encryption_key": "enc-key-2",
        })))
        .mount(&server)
        .await;

    let flow = client(&server)
        .init_reversal_transaction(&InitReversal::new("xact-1"))
        .await
        .expect("the reversal starts");

    assert_eq!(flow.payment_session_id.as_deref(), Some("ps-2"));
}

#[tokio::test]
async fn a_blank_xact_id_never_opens_a_socket() {
    let server = MockServer::start().await;

    let error = client(&server)
        .init_reversal_transaction(&InitReversal::new(""))
        .await
        .expect_err("xact_id is required");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "a blank xact_id opened a socket");
}

#[tokio::test]
async fn checking_a_transaction_reads_the_array_paypos_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .and(header("session-key", "session-abc"))
        .and(body_json(json!({ "payment_session_id": "ps-1" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Data": [
                {
                    "xact_id": "xact-1",
                    "is_succeed": true,
                    "amount": 149.90,
                    "netAmount": 145.00,
                    "comission": 4.90,
                    "comission_tax": 0.98,
                    "currency": "TRY",
                    "card_holder": "Ayşe Yılmaz",
                    "ratio": 3.28,
                }
            ],
            "object_name": "CheckTransaction",
            "code": 0,
            "message": "Success",
        })))
        .mount(&server)
        .await;

    let transactions = client(&server)
        .check_transaction("ps-1")
        .await
        .expect("the transactions");

    assert_eq!(transactions.len(), 1);
    let transaction = &transactions[0];
    assert_eq!(transaction.xact_id.as_deref(), Some("xact-1"));
    assert_eq!(transaction.is_succeed, Some(true));
    assert_eq!(transaction.amount, Some(lira("149.90")));
    assert_eq!(transaction.net_amount, Some(lira("145.00")));
    assert_eq!(transaction.commission_amount, Some(lira("4.90")));
    assert_eq!(transaction.commission_tax, Some(lira("0.98")));
    assert_eq!(transaction.card_holder.as_deref(), Some("Ayşe Yılmaz"));
    assert_eq!(transaction.ratio.as_deref(), Some("3.28"));
}

#[tokio::test]
async fn a_currency_this_crate_can_still_read_is_not_forced_into_lira() {
    // check_transaction's reading is permissive the same way mass and
    // iyzilink's is: PayPOS restricting what it will *take* on
    // init_sale_transaction (TRY) says nothing about what it might echo back
    // on a line, so a Currency this crate can name is read as that currency,
    // not silently coerced to TRY or thrown away.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Data": [
                {
                    "xact_id": "xact-2",
                    "amount": 10.00,
                    "currency": "USD",
                }
            ],
        })))
        .mount(&server)
        .await;

    let transactions = client(&server)
        .check_transaction("ps-9")
        .await
        .expect("the transactions");

    assert_eq!(transactions.len(), 1);
    assert_eq!(
        transactions[0].amount,
        Some(Money::parse("10.00", Currency::Usd).expect("a valid amount"))
    );
}

#[tokio::test]
async fn a_transaction_in_a_currency_kasapay_cannot_name_stays_in_raw() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Data": [
                {
                    "xact_id": "xact-2",
                    "amount": 10.00,
                    "currency": "ISK",
                }
            ],
        })))
        .mount(&server)
        .await;

    let transactions = client(&server)
        .check_transaction("ps-9")
        .await
        .expect("the transactions");

    assert_eq!(transactions.len(), 1);
    assert_eq!(transactions[0].amount, None);
    assert_eq!(
        transactions[0].raw.text_at("/currency").as_deref(),
        Some("ISK")
    );
}

#[tokio::test]
async fn an_empty_transaction_list_is_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "Data": [] })))
        .mount(&server)
        .await;

    let transactions = client(&server)
        .check_transaction("ps-none")
        .await
        .expect("an empty answer is still an answer");
    assert!(transactions.is_empty());
}

#[tokio::test]
async fn a_refusal_carries_paynets_own_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "object_name": "Error",
            "code": 2004,
            "message": "Session expired",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .check_transaction("ps-1")
        .await
        .expect_err("the session is gone");
    assert_eq!(error.code(), Some("2004"));
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

#[tokio::test]
async fn a_fresh_session_key_replaces_the_one_a_client_was_built_with() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/softpos/check_transaction"))
        .and(header("session-key", "session-renewed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "Data": [] })))
        .mount(&server)
        .await;

    let till = client(&server);
    till.set_session_key("session-renewed");
    till.check_transaction("ps-1")
        .await
        .expect("the renewed key is accepted");
}
