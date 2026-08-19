//! Customers, mandates, and charging one — against a mock server.
//!
//! **Every response body here is one of Mollie's own documented examples**,
//! copied from their OpenAPI document — `create-customer-201-1` (which reuses
//! their `get-customer` example), `list-mandates-200-1`, `get-mandate-200-1`,
//! and `create-payment-201-6` / `create-payment-201-7`, "First payment linked
//! to customer (creates mandate)" and "Recurring payment linked to customer".
//! Mollie publishes no request body for either payment example — only
//! `x-request` pointers into files their public repository does not carry —
//! so nothing here asserts what was sent for those two beyond what this
//! crate's own code is responsible for adding.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{
    ChargeRequest, Currency, ErrorKind, InstrumentId, Money, NextAction, OrderRef, Provider,
    Secret, Status,
};
use kasapay_mollie::{Config, CustomerId, MandateStatus, Mollie};
use serde_json::json;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Mollie {
    let config = Config::at(&server.uri(), Secret::new("test_kasapay")).expect("valid base");
    Mollie::new(config).expect("client builds")
}

fn request_for(customer: Option<&str>) -> ChargeRequest {
    let builder = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("10.00", Currency::Eur).expect("valid amount"),
    )
    .description("Order #12345")
    .return_url("https://example.com".parse().expect("valid url"));
    let builder = match customer {
        Some(id) => builder.customer(id),
        None => builder,
    };
    builder.build().expect("valid request")
}

/// Mollie's `create-payment-201-7`, "Recurring payment linked to customer".
/// `redirectUrl` is `null` and `_links` carries no `checkout` — only
/// `changePaymentState`, which this crate does not read.
fn recurring_payment_linked_to_customer() -> serde_json::Value {
    json!({
        "resource": "payment",
        "id": "tr_DjH8FpPNj7",
        "mode": "test",
        "createdAt": "2022-01-04T12:44:39+00:00",
        "amount": { "value": "20.00", "currency": "EUR" },
        "description": "Direct debit charge as recuring payment",
        "method": "directdebit",
        "metadata": {
            "someProperty": "someValue",
            "anotherProperty": "anotherValue"
        },
        "status": "pending",
        "isCancelable": true,
        "locale": "en_US",
        "profileId": "pfl_85dxyKqNHa",
        "customerId": "cst_tKt44u85MM",
        "mandateId": "mdt_uDPFVsxjR4",
        "sequenceType": "recurring",
        "redirectUrl": null,
        "webhookUrl": "https://example.com/webhook",
        "settlementAmount": { "value": "20.00", "currency": "EUR" },
        "details": {
            "transferReference": "SD49-7608-3386-2993",
            "creditorIdentifier": "NL08ZZZ502057730000",
            "consumerName": "Jane Doe",
            "consumerAccount": "NL55INGB0000000000",
            "consumerBic": "INGBNL2A",
            "dueDate": "2022-01-05",
            "signatureDate": "2022-01-04"
        },
        "_links": {
            "self": { "href": "...", "type": "application/hal+json" },
            "dashboard": {
                "href": "https://www.mollie.com/dashboard/org_13514547/payments/tr_DjH8FpPNj7",
                "type": "text/html"
            },
            "changePaymentState": {
                "href": "https://www.mollie.com/checkout/test-mode?method=directdebit&token=3.88ss2m",
                "type": "text/html"
            },
            "customer": {
                "href": "https://api.mollie.com/v2/customers/cst_tKt44u85MM",
                "type": "application/hal+json"
            },
            "mandate": {
                "href": "https://api.mollie.com/v2/customers/cst_tKt44u85MM/mandates/mdt_uDPFVsxjR4",
                "type": "application/hal+json"
            },
            "documentation": { "href": "...", "type": "text/html" }
        }
    })
}

/// Mollie's `create-customer-201-1` example, which is their `get-customer`
/// object.
#[tokio::test]
async fn creating_a_customer_answers_its_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/customers"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "resource": "customer",
            "id": "cst_tKt44u85MM",
            "mode": "test",
            "name": "Jane Doe",
            "email": "test@mollie.com",
            "locale": "en_US",
            "metadata": {
                "someProperty": "someValue",
                "anotherProperty": "anotherValue"
            },
            "createdAt": "2022-01-03T13:42:04+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "dashboard": {
                    "href": "https://www.mollie.com/dashboard/org_13514547/customers/cst_tKt44u85MM",
                    "type": "text/html"
                },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let customer = client(&server)
        .create_customer(Some("Jane Doe"), Some("test@mollie.com"))
        .await
        .expect("the customer is created");

    assert_eq!(customer.id.as_str(), "cst_tKt44u85MM");
    assert_eq!(customer.name.as_deref(), Some("Jane Doe"));
    assert_eq!(customer.email.as_deref(), Some("test@mollie.com"));
}

/// Mollie's `list-mandates-200-1` example, which carries a `next` link — so
/// the walk asks for a second page, and there has to be one.
#[tokio::test]
async fn listing_mandates_reads_each_ones_own_status() {
    let server = MockServer::start().await;
    // The page after the example's cursor. Mollie publishes no two-page
    // example, so this is their list shape with nothing left in it.
    Mock::given(method("GET"))
        .and(path("/v2/customers/cst_4qqhO89gsT/mandates"))
        .and(query_param("from", "mdt_pWUnw6pkBN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 0,
            "_embedded": { "mandates": [] },
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "previous": null,
                "next": null,
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/customers/cst_4qqhO89gsT/mandates"))
        .and(query_param_is_missing("from"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 1,
            "_embedded": {
                "mandates": [{
                    "resource": "mandate",
                    "id": "mdt_h3gAaD5zP",
                    "mode": "live",
                    "status": "valid",
                    "method": "directdebit",
                    "details": {},
                    "mandateReference": "EXAMPLE-CORP-MD13804",
                    "signatureDate": "2023-05-07",
                    "customerId": "cst_4qqhO89gsT",
                    "scopes": ["customer-not-present"],
                    "createdAt": "2023-05-07T10:49:08+00:00",
                    "_links": {
                        "self": { "href": "...", "type": "application/hal+json" },
                        "customer": {
                            "href": "https://api.mollie.com/v2/customers/cst_4qqhO89gsT",
                            "type": "application/hal+json"
                        },
                        "documentation": { "href": "...", "type": "text/html" }
                    }
                }]
            },
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "previous": null,
                "next": {
                    "href": "https://api.mollie.com/v2/mandates?from=mdt_pWUnw6pkBN&limit=5",
                    "type": "application/hal+json"
                },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let customer = CustomerId::issued("cst_4qqhO89gsT");
    let mandates = client(&server)
        .mandates(&customer)
        .await
        .expect("the list reads back");

    assert_eq!(mandates.len(), 1);
    let mandate = &mandates[0];
    assert_eq!(mandate.id.as_str(), "mdt_h3gAaD5zP");
    assert_eq!(mandate.customer, customer);
    assert_eq!(mandate.status, MandateStatus::Valid);
    assert_eq!(mandate.method.as_deref(), Some("directdebit"));
    // Its own object out of the list, not the list body.
    assert_eq!(
        mandate.raw.text_at("/mandateReference").as_deref(),
        Some("EXAMPLE-CORP-MD13804")
    );
    assert!(mandate.raw.text_at("/count").is_none());
}

/// `Provider::instruments` is `Mollie::mandates` behind the shared trait,
/// with `method` becoming the label.
#[tokio::test]
async fn provider_instruments_lists_the_same_mandates() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/customers/cst_4qqhO89gsT/mandates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "count": 1,
            "_embedded": {
                "mandates": [{
                    "resource": "mandate",
                    "id": "mdt_h3gAaD5zP",
                    "mode": "live",
                    "status": "valid",
                    "method": "directdebit",
                    "details": {},
                    "mandateReference": "EXAMPLE-CORP-MD13804",
                    "signatureDate": "2023-05-07",
                    "customerId": "cst_4qqhO89gsT",
                    "scopes": ["customer-not-present"],
                    "createdAt": "2023-05-07T10:49:08+00:00",
                    "_links": {
                        "self": { "href": "...", "type": "application/hal+json" }
                    }
                }]
            },
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" }
            }
        })))
        .mount(&server)
        .await;

    let instruments = client(&server)
        .instruments("cst_4qqhO89gsT")
        .await
        .expect("the list reads back");

    assert_eq!(instruments.len(), 1);
    let instrument = &instruments[0];
    assert_eq!(instrument.id, InstrumentId::issued("mdt_h3gAaD5zP"));
    assert_eq!(instrument.label.as_deref(), Some("directdebit"));
    assert_eq!(
        instrument.raw.text_at("/mandateReference").as_deref(),
        Some("EXAMPLE-CORP-MD13804")
    );
}

/// Mollie's `get-mandate-200-1` example.
#[tokio::test]
async fn reading_a_mandate_back_by_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/customers/cst_4qqhO89gsT/mandates/mdt_h3gAaD5zP"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": "mandate",
            "id": "mdt_h3gAaD5zP",
            "mode": "live",
            "status": "valid",
            "method": "directdebit",
            "details": {
                "consumerName": "John Doe",
                "consumerAccount": "NL55INGB0000000000",
                "consumerBic": "INGBNL2A"
            },
            "mandateReference": "EXAMPLE-CORP-MD13804",
            "signatureDate": "2023-05-07",
            "customerId": "cst_4qqhO89gsT",
            "scopes": ["customer-not-present"],
            "createdAt": "2023-05-07T10:49:08+00:00",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "customer": {
                    "href": "https://api.mollie.com/v2/customers/cst_4qqhO89gsT",
                    "type": "application/hal+json"
                },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let mandate = client(&server)
        .mandate(
            &CustomerId::issued("cst_4qqhO89gsT"),
            &InstrumentId::issued("mdt_h3gAaD5zP"),
        )
        .await
        .expect("the mandate reads back");

    assert_eq!(mandate.status, MandateStatus::Valid);
    assert_eq!(
        mandate.raw.text_at("/details/consumerAccount").as_deref(),
        Some("NL55INGB0000000000")
    );
}

/// Mollie's `create-payment-201-6`, "First payment linked to customer
/// (creates mandate)". The mandate exists — `pending`, per Mollie's own
/// description of the status — before the payer has done anything, and the
/// payment still redirects them the way an ordinary `charge` does.
#[tokio::test]
async fn a_first_payment_creates_a_mandate_and_still_redirects() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "resource": "payment",
            "id": "tr_NQCRf7VtKP",
            "mode": "test",
            "createdAt": "2022-01-14T14:37:01+00:00",
            "amount": { "value": "0.00", "currency": "EUR" },
            "description": "Direct debit charge as recuring payment",
            "method": "creditcard",
            "metadata": {
                "someProperty": "someValue",
                "anotherProperty": "anotherValue"
            },
            "status": "open",
            "isCancelable": false,
            "expiresAt": "2022-01-14T14:54:01+00:00",
            "locale": "en_US",
            "profileId": "pfl_85dxyKqNHa",
            "customerId": "cst_tKt44u85MM",
            "mandateId": "mdt_KmQnsDNkq2",
            "sequenceType": "first",
            "redirectUrl": "https://example.com/redirect",
            "webhookUrl": "https://example.com/webhook",
            "_links": {
                "self": { "href": "...", "type": "application/hal+json" },
                "checkout": {
                    "href": "https://www.mollie.com/checkout/test-mode?method=creditcard&token=3.ivicl6",
                    "type": "text/html"
                },
                "dashboard": {
                    "href": "https://www.mollie.com/dashboard/org_13514547/payments/tr_NQCRf7VtKP",
                    "type": "text/html"
                },
                "customer": {
                    "href": "https://api.mollie.com/v2/customers/cst_tKt44u85MM",
                    "type": "application/hal+json"
                },
                "mandate": {
                    "href": "https://api.mollie.com/v2/customers/cst_tKt44u85MM/mandates/mdt_KmQnsDNkq2",
                    "type": "application/hal+json"
                },
                "documentation": { "href": "...", "type": "text/html" }
            }
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_first(&request_for(Some("cst_tKt44u85MM")))
        .await
        .expect("the first payment opens");

    assert_eq!(charge.status, Status::RequiresAction);
    match charge.next_action.expect("a checkout to send the payer to") {
        NextAction::Redirect { url, .. } => {
            assert!(url.as_str().contains("checkout"), "{url}");
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
    assert_eq!(
        charge.raw.text_at("/mandateId").as_deref(),
        Some("mdt_KmQnsDNkq2")
    );
    assert_eq!(
        charge.raw.text_at("/sequenceType").as_deref(),
        Some("first")
    );

    let sent = &server.received_requests().await.expect("requests recorded")[0];
    let body = String::from_utf8_lossy(&sent.body);
    assert!(body.contains(r#""sequenceType":"first""#), "{body}");
    assert!(body.contains(r#""customerId":"cst_tKt44u85MM""#), "{body}");
    assert!(!body.contains("mandateId"), "{body}");
}

/// A first payment establishes a mandate on a customer, and there is no
/// customer to establish one on without `ChargeRequest::customer`.
#[tokio::test]
async fn a_first_payment_without_a_customer_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let error = client(&server)
        .charge_first(&request_for(None))
        .await
        .expect_err("no customer was named");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert!(error.to_string().contains("customer"), "{error}");
}

/// Mollie's `create-payment-201-7`, "Recurring payment linked to customer".
/// `redirectUrl` is `null` and `_links` carries no `checkout` — only
/// `changePaymentState`, which this crate does not read — so the charge
/// carries no [`NextAction`], unlike every other charge this adapter opens.
#[tokio::test]
async fn a_recurring_payment_against_a_mandate_has_no_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(recurring_payment_linked_to_customer()),
        )
        .mount(&server)
        .await;

    let mandate = InstrumentId::issued("mdt_uDPFVsxjR4");
    let charge = client(&server)
        .charge_with_mandate(&request_for(Some("cst_tKt44u85MM")), &mandate)
        .await
        .expect("the recurring payment is charged");

    assert_eq!(charge.status, Status::Pending);
    // The status is still open, but there is nowhere left to send the payer:
    // Mollie's own example carries no `checkout` link for this one.
    assert!(charge.next_action.is_none());
    assert_eq!(
        charge.raw.text_at("/mandateId").as_deref(),
        Some("mdt_uDPFVsxjR4")
    );

    let sent = &server.received_requests().await.expect("requests recorded")[0];
    let body = String::from_utf8_lossy(&sent.body);
    assert!(body.contains(r#""sequenceType":"recurring""#), "{body}");
    assert!(body.contains(r#""mandateId":"mdt_uDPFVsxjR4""#), "{body}");
    assert!(body.contains(r#""customerId":"cst_tKt44u85MM""#), "{body}");
}

/// A recurring payment is charged against a customer's mandate, and there is
/// no customer to charge without `ChargeRequest::customer`.
#[tokio::test]
async fn a_recurring_payment_without_a_customer_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let mandate = InstrumentId::issued("mdt_uDPFVsxjR4");
    let error = client(&server)
        .charge_with_mandate(&request_for(None), &mandate)
        .await
        .expect_err("no customer was named");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert!(error.to_string().contains("customer"), "{error}");
}

/// `capabilities().saved_instruments` and the call that honours it, in the
/// same test: a capability that says yes and a call that then fails is a bug
/// in the adapter, and this is where that would show up. The response is
/// Mollie's `create-payment-201-7` example, the same one
/// `a_recurring_payment_against_a_mandate_has_no_redirect` reads, byte for
/// byte.
#[tokio::test]
async fn saved_instruments_is_true_and_charging_a_mandate_is_how_its_honoured() {
    let server = MockServer::start().await;
    assert!(client(&server).capabilities().saved_instruments);

    Mock::given(method("POST"))
        .and(path("/v2/payments"))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(recurring_payment_linked_to_customer()),
        )
        .mount(&server)
        .await;

    let mandate = InstrumentId::issued("mdt_uDPFVsxjR4");
    client(&server)
        .charge_with_mandate(&request_for(Some("cst_tKt44u85MM")), &mandate)
        .await
        .expect("the capability that says yes is honoured by a call that works");
}
