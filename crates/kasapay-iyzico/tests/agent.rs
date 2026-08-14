//! PayPOS's Authorize service against a mock server.
//!
//! Neither language's page for `get_auth_key` or `logout` carries a worked
//! example — a curl command, a filled-in request, an answer with real
//! values — only the OpenAPI schema. So every fixture here is built from the
//! field names PayPOS documents and nothing more, the same position
//! [`mass`](kasapay_iyzico::mass)'s `authorize`, `cancel`, `balance` and
//! single-item read are in and say so for. No live PayPOS account was
//! available to check any of it against.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{ErrorKind, Secret};
use kasapay_iyzico::agent::{Client, Config, Credentials};
use serde_json::json;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    let config = Config::new(&format!("{}/", server.uri())).expect("valid base");
    Client::new(config, Credentials::new("sck_test_dealer")).expect("client builds")
}

#[tokio::test]
async fn getting_a_session_sends_the_secret_key_and_the_paynet_mobile_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent/get_auth_key"))
        .and(header("authorization", "Basic sck_test_dealer"))
        .and(header("paynetmobile", "2"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "agent_id": "agent-1",
            "user_id": "till-7",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "session_key": "session-abc",
            "expired_date": "2026-08-14T18:00:00+03:00",
            "agent_id": "agent-1",
            "company_code": "company-9",
            "user_id": "till-7",
            "user_unique_id": "user-42",
            "is_okc_inquiry": false,
            "object_name": "Auth",
            "code": 0,
            "message": "Success",
        })))
        .mount(&server)
        .await;

    let session = client(&server)
        .get_auth_key("agent-1", "till-7")
        .await
        .expect("a session");

    assert_eq!(session.session_key.expose(), "session-abc");
    assert_eq!(
        session.expired_date.as_deref(),
        Some("2026-08-14T18:00:00+03:00")
    );
    assert_eq!(session.agent_id.as_deref(), Some("agent-1"));
    assert_eq!(session.company_code.as_deref(), Some("company-9"));
    assert_eq!(session.is_okc_inquiry, Some(false));
}

#[tokio::test]
async fn a_session_with_no_key_is_malformed_rather_than_a_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent/get_auth_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object_name": "Auth",
            "code": 0,
            "message": "Success",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .get_auth_key("agent-1", "till-7")
        .await
        .expect_err("no session_key was sent");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

#[tokio::test]
async fn a_refusal_carries_paynets_own_code_and_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent/get_auth_key"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "object_name": "Error",
            "code": 1001,
            "message": "Invalid secret key",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .get_auth_key("agent-1", "till-7")
        .await
        .expect_err("the secret key is wrong");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("1001"));
}

#[tokio::test]
async fn a_refusal_with_an_auth_status_is_read_as_auth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent/get_auth_key"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "code": 1010,
            "message": "IP not registered",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .get_auth_key("agent-1", "till-7")
        .await
        .expect_err("the caller's IP is not on Paynet's list");
    assert_eq!(error.kind(), ErrorKind::Auth);
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn logging_out_sends_the_session_key_in_the_body_and_the_secret_in_the_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/agent/logout"))
        .and(header("authorization", "Basic sck_test_dealer"))
        .and(header("paynetmobile", "2"))
        .and(body_json(json!({ "session_key": "session-abc" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object_name": "Logout",
            "code": 0,
            "message": "Success",
        })))
        .mount(&server)
        .await;

    client(&server)
        .logout(&Secret::new("session-abc"))
        .await
        .expect("the session ends");
}

#[tokio::test]
async fn blank_identifiers_never_open_a_socket() {
    let server = MockServer::start().await;

    let error = client(&server)
        .get_auth_key("", "till-7")
        .await
        .expect_err("agent_id is required");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let error = client(&server)
        .get_auth_key("agent-1", "   ")
        .await
        .expect_err("user_id is required");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "a blank identifier opened a socket");
}

#[tokio::test]
async fn the_secret_key_does_not_print_itself() {
    let credentials = Credentials::new("sck_should_not_appear_in_debug");
    let printed = format!("{credentials:?}");
    assert!(
        !printed.contains("sck_should_not_appear_in_debug"),
        "{printed}"
    );
}
