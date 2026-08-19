//! The subscription catalogue against a mock server.
//!
//! The fixtures are the shapes iyzico documents: field names, nesting and the
//! two shapes `createdDate` and `price` come from the OpenAPI fragments on
//! their subscription product and pricing plan pages, in both languages. The
//! failure envelope is the `errorCode`/`errorMessage` those same fragments
//! document for a 400. The values are stand-ins. No live subscription account
//! was available to check them against, and no test here claims otherwise.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{Currency, ErrorKind, Money};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic;
use kasapay_iyzico::subscription::{
    Address, Client, InitialStatus, NewPlan, NewProduct, NewSubscription, PaymentInterval,
    PlanError, PlanPaymentType, PlanUpdate, ProductUpdate, RecordStatus, Subscriber,
    SubscriptionStatus, Upgrade,
};
use serde_json::json;
use wiremock::matchers::{
    body_json, body_string_contains, header, header_exists, method, path, query_param,
};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const PRODUCT: &str = "b7f3a1c2-4d5e-4f60-8a91-2c3d4e5f6071";
const PLAN: &str = "9c2a8b7d-6e5f-4a3b-9c8d-7e6f5a4b3c2d";

fn client(server: &MockServer) -> Client {
    let config = classic::Config::new(&server.uri(), Credentials::new("api-key", "secret-key"))
        .expect("valid base");
    Client::new(classic::Client::new(config).expect("client builds"))
}

/// A plan as iyzico documents it inside a product: a date as text, and the
/// price as the JSON number the Turkish pages type it as.
fn plan_summary() -> serde_json::Value {
    json!({
        "referenceCode": PLAN,
        "createdDate": "2026-08-14 10:31:00",
        "name": "A Dergisi aylık",
        "price": 50.0,
        "paymentInterval": "MONTHLY",
        "paymentIntervalCount": 1,
        "trialPeriodDays": 14,
        "currencyCode": "TRY",
        "productReferenceCode": PRODUCT,
        "planPaymentType": "RECURRING",
        "status": "ACTIVE",
        "recurrenceCount": 12,
    })
}

/// The same plan as iyzico documents it on its own: a date in epoch
/// milliseconds, and the price as a decimal string.
fn plan() -> serde_json::Value {
    json!({
        "referenceCode": PLAN,
        "createdDate": 1_770_000_000_000_i64,
        "name": "A Dergisi aylık",
        "productReferenceCode": PRODUCT,
        "price": "50.00",
        "currencyCode": "TRY",
        "paymentInterval": "MONTHLY",
        "paymentIntervalCount": 1,
        "planPaymentType": "RECURRING",
        "recurrenceCount": 12,
        "trialPeriodDays": 14,
        "status": "ACTIVE",
    })
}

/// One product, with every field the product schema lists.
fn product() -> serde_json::Value {
    json!({
        "referenceCode": PRODUCT,
        "createdDate": "2026-08-14 10:30:00",
        "name": "A Dergisi",
        "description": "Aylık dergi",
        "status": "ACTIVE",
        "pricingPlans": [plan_summary()],
    })
}

fn envelope(data: &serde_json::Value) -> serde_json::Value {
    json!({
        "status": "success",
        "systemTime": 1_770_000_000_000_i64,
        "data": data,
    })
}

fn a_monthly_plan() -> NewPlan {
    NewPlan::builder(
        "A Dergisi aylık",
        Money::parse("50.00", Currency::Try).expect("a valid amount"),
        PaymentInterval::Monthly,
    )
    .trial_days(14)
    .recurrences(12)
    .build()
    .expect("a plan iyzico documents")
}

#[tokio::test]
async fn creating_a_product_sends_the_documented_body_and_reads_it_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/products"))
        .and(header("content-type", "application/json"))
        .and(header_exists("x-iyzi-rnd"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "ref-1",
            "name": "A Dergisi",
            "description": "Aylık dergi",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&product())))
        .mount(&server)
        .await;

    let created = client(&server)
        .create_product(
            &NewProduct::builder("A Dergisi")
                .describe("Aylık dergi")
                .conversation_id("ref-1")
                .build(),
        )
        .await
        .expect("the product is created");

    assert_eq!(&*created.reference_code, PRODUCT);
    assert_eq!(created.name.as_deref(), Some("A Dergisi"));
    assert_eq!(created.status, Some(RecordStatus::Active));
    // Documented as text on a product, and kept as iyzico wrote it.
    assert_eq!(created.created_date.as_deref(), Some("2026-08-14 10:30:00"));
    assert_eq!(created.plans.len(), 1);
    assert_eq!(
        created.plans[0].price,
        Some(Money::parse("50.00", Currency::Try).expect("valid"))
    );
    assert_eq!(created.plans[0].interval, Some(PaymentInterval::Monthly));
    assert_eq!(
        created.plans[0].payment_type,
        Some(PlanPaymentType::Recurring)
    );
    // Each plan keeps its own bytes, not the product's.
    assert_eq!(
        created.plans[0].raw.text_at("/name").as_deref(),
        Some("A Dergisi aylık")
    );
}

#[tokio::test]
async fn a_product_with_no_description_sends_none() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/products"))
        .and(body_json(json!({ "locale": "tr", "name": "B Dergisi" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&product())))
        .mount(&server)
        .await;

    client(&server)
        .create_product(&NewProduct::builder("B Dergisi").build())
        .await
        .expect("a name is all iyzico asks for");
}

#[tokio::test]
async fn the_signature_covers_the_path_and_leaves_the_query_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/subscription/products"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": { "totalCount": "0", "currentPage": 1, "pageCount": 0, "items": [] },
        })))
        .mount(&server)
        .await;

    client(&server)
        .products(2, 25)
        .await
        .expect("an empty page");

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

    // The paging is on the URL and out of the signature, and nothing was sent
    // as a body — iyzico documents a JSON body here and their own SDK sends a
    // query instead.
    assert_eq!(request.url.query(), Some("locale=tr&page=2&count=25"));
    assert!(request.body.is_empty());
    let expected = Credentials::new("api-key", "secret-key").signature(
        random_key,
        "/v2/subscription/products",
        "",
    );
    assert!(
        decoded.ends_with(&format!("signature:{expected}")),
        "the signature covered something other than the path"
    );
}

#[tokio::test]
async fn a_product_listing_reads_its_page_and_the_products_on_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/subscription/products"))
        .and(query_param("locale", "tr"))
        .and(query_param("page", "1"))
        .and(query_param("count", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_770_000_000_000_i64,
            "data": {
                // A string here and an integer on a page of plans. Both read.
                "totalCount": "2",
                "currentPage": 1,
                "pageCount": 1,
                "items": [product(), { "referenceCode": "ZzZ999", "status": "ACTIVE" }],
            },
        })))
        .mount(&server)
        .await;

    let page = client(&server)
        .products(1, 10)
        .await
        .expect("a page of products");

    assert_eq!(page.total_count, Some(2));
    assert_eq!(page.page_count, Some(1));
    assert_eq!(page.items.len(), 2);
    assert_eq!(&*page.items[0].reference_code, PRODUCT);
    assert_eq!(page.items[0].plans.len(), 1);
    // A product with no plans is a product, not a malformed answer.
    assert!(page.items[1].plans.is_empty());
    assert_eq!(page.items[1].name, None);
}

#[tokio::test]
async fn reading_a_product_names_the_reference_in_the_path_and_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v2/subscription/products/{PRODUCT}")))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&product())))
        .mount(&server)
        .await;

    let read = client(&server).product(PRODUCT).await.expect("the product");

    assert_eq!(read.description.as_deref(), Some("Aylık dergi"));

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn an_update_replaces_the_name_and_the_description() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/subscription/products/{PRODUCT}")))
        .and(body_json(json!({
            "locale": "tr",
            "name": "A Dergisi Premium",
            "description": "Aylık dergi, ekli içerikle",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&product())))
        .mount(&server)
        .await;

    client(&server)
        .update_product(
            PRODUCT,
            &ProductUpdate::builder("A Dergisi Premium")
                .describe("Aylık dergi, ekli içerikle")
                .build(),
        )
        .await
        .expect("the product is updated");
}

#[tokio::test]
async fn deleting_a_product_carries_no_body_at_all() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/subscription/products/{PRODUCT}")))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_770_000_000_000_i64,
        })))
        .mount(&server)
        .await;

    client(&server)
        .delete_product(PRODUCT)
        .await
        .expect("it is gone");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    // The classic API deletes a stored card with a JSON body; this one does not.
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn creating_a_plan_names_the_product_in_the_path_and_in_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v2/subscription/products/{PRODUCT}/pricing-plans"
        )))
        .and(body_json(json!({
            "locale": "tr",
            // iyzico documents the product only in the path; their PHP SDK
            // puts it in the body as well, and this sends both.
            "productReferenceCode": PRODUCT,
            "name": "A Dergisi aylık",
            "price": "50.00",
            "currencyCode": "TRY",
            "paymentInterval": "MONTHLY",
            "planPaymentType": "RECURRING",
            "recurrenceCount": 12,
            "trialPeriodDays": 14,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&plan())))
        .mount(&server)
        .await;

    let created = client(&server)
        .create_plan(PRODUCT, &a_monthly_plan())
        .await
        .expect("the plan is created");

    assert_eq!(&*created.reference_code, PLAN);
    assert_eq!(created.interval, Some(PaymentInterval::Monthly));
    assert_eq!(created.trial_days, Some(14));
    assert_eq!(created.recurrences, Some(12));
    assert_eq!(created.status, Some(RecordStatus::Active));
    // Epoch milliseconds on a plan of its own, and text on the same plan
    // inside a product. Neither is turned into the other.
    assert_eq!(created.created_date.as_deref(), Some("1770000000000"));
}

#[tokio::test]
async fn a_plan_charged_every_other_week_says_so() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v2/subscription/products/{PRODUCT}/pricing-plans"
        )))
        .and(body_json(json!({
            "locale": "tr",
            "productReferenceCode": PRODUCT,
            "name": "İki haftada bir",
            "price": "30.00",
            "currencyCode": "TRY",
            "paymentInterval": "WEEKLY",
            "paymentIntervalCount": 2,
            "planPaymentType": "RECURRING",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&plan())))
        .mount(&server)
        .await;

    let plan = NewPlan::builder(
        "İki haftada bir",
        Money::parse("30.00", Currency::Try).expect("valid"),
        PaymentInterval::Weekly,
    )
    .interval_count(2)
    .build()
    .expect("a plan iyzico documents");

    client(&server)
        .create_plan(PRODUCT, &plan)
        .await
        .expect("the plan is created");
}

#[tokio::test]
async fn a_plan_update_carries_a_name_and_a_trial_and_nothing_else() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/v2/subscription/pricing-plans/{PLAN}")))
        .and(body_json(json!({
            "locale": "tr",
            "pricingPlanReferenceCode": PLAN,
            "name": "A Dergisi aylık (yeni)",
            "trialPeriodDays": 30,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&plan())))
        .mount(&server)
        .await;

    client(&server)
        .update_plan(
            PLAN,
            &PlanUpdate::builder("A Dergisi aylık (yeni)")
                .trial_days(30)
                .build(),
        )
        .await
        .expect("the plan is updated");
}

#[tokio::test]
async fn a_plan_listing_pages_under_its_product() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v2/subscription/products/{PRODUCT}/pricing-plans"
        )))
        .and(query_param("page", "1"))
        .and(query_param("count", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": {
                // An integer here and a string on a page of products.
                "totalCount": 1,
                "currentPage": 1,
                "pageCount": 1,
                "items": [plan()],
            },
        })))
        .mount(&server)
        .await;

    let page = client(&server)
        .plans(PRODUCT, 1, 5)
        .await
        .expect("a page of plans");

    assert_eq!(page.total_count, Some(1));
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].product_reference_code.as_deref(),
        Some(PRODUCT)
    );
}

#[tokio::test]
async fn reading_a_plan_takes_the_price_iyzico_sent_as_a_number() {
    let server = MockServer::start().await;
    let mut body = plan();
    body["price"] = json!(50.0);
    Mock::given(method("GET"))
        .and(path(format!("/v2/subscription/pricing-plans/{PLAN}")))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&body)))
        .mount(&server)
        .await;

    let read = client(&server).plan(PLAN).await.expect("the plan");

    assert_eq!(
        read.price,
        Some(Money::parse("50.00", Currency::Try).expect("valid"))
    );
}

#[tokio::test]
async fn a_price_in_a_currency_kasapay_cannot_name_stays_in_the_raw_body() {
    let server = MockServer::start().await;
    let mut body = plan();
    // Not a currency iyzico documents a plan in, and not one `Currency` names.
    body["currencyCode"] = json!("SEK");
    Mock::given(method("GET"))
        .and(path(format!("/v2/subscription/pricing-plans/{PLAN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&body)))
        .mount(&server)
        .await;

    let read = client(&server).plan(PLAN).await.expect("the plan");

    // The plan still reads, and the amount stays where iyzico put it.
    assert_eq!(read.price, None);
    assert_eq!(read.raw.text_at("/currencyCode").as_deref(), Some("SEK"));
}

#[tokio::test]
async fn a_currency_kasapay_names_and_iyzico_does_not_document_still_reads() {
    let server = MockServer::start().await;
    let mut body = plan();
    // Roubles: a currency `Currency` names since #86 and iyzico documents for
    // a link but not for a plan. Reading is the permissive direction — this
    // comes back as money, and only the builder refuses it.
    body["currencyCode"] = json!("RUB");
    Mock::given(method("GET"))
        .and(path(format!("/v2/subscription/pricing-plans/{PLAN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(envelope(&body)))
        .mount(&server)
        .await;

    let read = client(&server).plan(PLAN).await.expect("the plan");

    assert_eq!(
        read.price,
        Some(Money::parse("50.00", Currency::Rub).expect("valid"))
    );
    assert_eq!(
        NewPlan::builder(
            "Aylık",
            read.price.expect("a price"),
            PaymentInterval::Monthly
        )
        .build()
        .expect_err("iyzico documents no plan priced in roubles"),
        PlanError::UnsupportedCurrency(Currency::Rub)
    );
}

#[tokio::test]
async fn deleting_a_plan_names_it_away_from_its_product() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/subscription/pricing-plans/{PLAN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_770_000_000_000_i64,
        })))
        .mount(&server)
        .await;

    client(&server).delete_plan(PLAN).await.expect("it is gone");
}

#[tokio::test]
async fn a_failure_envelope_carries_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path(format!("/v2/subscription/products/{PRODUCT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "601001",
            "errorMessage": "Ürüne bağlı ödeme planı bulunmaktadır",
            "systemTime": 1_770_000_000_000_i64,
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .delete_product(PRODUCT)
        .await
        .expect_err("a failure status is not a deletion");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("601001"));
}

#[tokio::test]
async fn a_reference_code_that_would_change_the_path_never_reaches_the_wire() {
    let server = MockServer::start().await;

    let error = client(&server)
        .product("AbC123/../../payment/detail")
        .await
        .expect_err("a reference code is one path segment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let error = client(&server)
        .plan("AbC123?locale=en")
        .await
        .expect_err("a reference code is one path segment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "a hostile reference code opened a socket");
}

#[tokio::test]
async fn a_page_iyzico_would_not_be_asked_for_is_refused_before_a_socket_opens() {
    let server = MockServer::start().await;

    for (page, count) in [(0, 10), (1, 0)] {
        let error = client(&server)
            .products(page, count)
            .await
            .expect_err("iyzico pages from 1, in counts of at least 1");
        assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    }

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty());
}

#[test]
fn every_currency_iyzico_does_not_document_a_plan_in_is_refused_by_the_builder() {
    // Six of the nine `Currency` names. Sterling, roubles, francs and kroner
    // are ones iyzico takes elsewhere — a link is documented in all four — and
    // documents no subscription plan in, in either language.
    for currency in [
        Currency::Gbp,
        Currency::Jpy,
        Currency::Kwd,
        Currency::Rub,
        Currency::Chf,
        Currency::Nok,
    ] {
        let price = Money::from_minor_units(999, currency);
        let error = NewPlan::builder("Monthly", price, PaymentInterval::Monthly)
            .build()
            .expect_err("iyzico documents no plan priced in this");
        assert_eq!(error, PlanError::UnsupportedCurrency(currency));
    }

    // And the three it does document all build.
    for currency in [Currency::Try, Currency::Usd, Currency::Eur] {
        let price = Money::from_minor_units(999, currency);
        assert!(
            NewPlan::builder("Monthly", price, PaymentInterval::Monthly)
                .build()
                .is_ok(),
            "{currency} is a currency iyzico documents a plan in"
        );
    }
}

#[test]
fn a_price_of_nothing_is_refused_by_the_builder() {
    let free = Money::from_minor_units(0, Currency::Try);
    assert!(
        NewPlan::builder("Bedava", free, PaymentInterval::Monthly)
            .build()
            .is_err(),
        "a plan that charges nothing is not a plan"
    );
}

const SUBSCRIPTION: &str = "3f2e1d0c-9b8a-4756-8493-2a1b0c9d8e7f";
const SUBSCRIBER: &str = "5a4b3c2d-1e0f-49a8-b7c6-d5e4f3a2b1c0";

fn subscriber() -> Subscriber {
    Subscriber::builder(
        "Ayse",
        "Yilmaz",
        "ayse@example.test",
        "+905350000000",
        "11111111111",
        Address::new("Ayse Yilmaz", "Bagdat Cad. 1", "Istanbul", "Turkey"),
    )
    .build()
}

/// One subscription, as iyzico's `SubscriptionDetailItem` documents it.
fn subscription_item(status: &str) -> serde_json::Value {
    json!({
        "referenceCode": SUBSCRIPTION,
        "parentReferenceCode": null,
        "pricingPlanName": "A Dergisi aylık",
        "pricingPlanReferenceCode": PLAN,
        "productName": "A Dergisi",
        "productReferenceCode": PRODUCT,
        "customerEmail": "ayse@example.test",
        "customerGsmNumber": "+905350000000",
        "customerReferenceCode": SUBSCRIBER,
        "subscriptionStatus": status,
        "trialDays": 14,
        "startDate": 1_770_000_000_000_i64,
        "endDate": null,
        "orders": [],
    })
}

/// The way in that does not put a card number through a caller's process.
#[tokio::test]
async fn the_subscription_form_carries_the_plan_the_person_and_where_to_answer() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/checkoutform/initialize"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "locale": "tr",
            "callbackUrl": "https://merchant.test/subscribed",
            "pricingPlanReferenceCode": PLAN,
            "subscriptionInitialStatus": "ACTIVE",
            "customer": {
                "name": "Ayse",
                "surname": "Yilmaz",
                "email": "ayse@example.test",
                "gsmNumber": "+905350000000",
                "identityNumber": "11111111111",
                "billingAddress": {
                    "contactName": "Ayse Yilmaz",
                    "address": "Bagdat Cad. 1",
                    "city": "Istanbul",
                    "country": "Turkey",
                },
            },
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "systemTime": 1_770_000_000_000_i64,
            "token": "sub-token-1",
            "checkoutFormContent": "<script>iyzico</script>",
            "tokenExpireTime": 1800,
        })))
        .mount(&server)
        .await;

    let form = client(&server)
        .start_subscription_form(
            &NewSubscription::builder(PLAN, subscriber(), "https://merchant.test/subscribed")
                .build(),
        )
        .await
        .expect("the form opens");

    assert_eq!(&*form.token, "sub-token-1");
    assert_eq!(form.expires_in_seconds, Some(1800));
    // iyzico answers the form's HTML here rather than a page to redirect to.
    assert!(form.content.expect("the form itself").contains("iyzico"));
}

/// `PENDING` is what a shop uses when something outside iyzico has to happen
/// first, and the first payment waits for `activate`.
#[tokio::test]
async fn a_pending_subscription_says_so_on_the_way_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/checkoutform/initialize"))
        .and(body_string_contains(
            r#""subscriptionInitialStatus":"PENDING""#,
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "token": "sub-token-1",
        })))
        .mount(&server)
        .await;

    client(&server)
        .start_subscription_form(
            &NewSubscription::builder(PLAN, subscriber(), "https://merchant.test/subscribed")
                .initial_status(InitialStatus::Pending)
                .build(),
        )
        .await
        .expect("the form opens");
}

/// A second subscription for somebody iyzico already holds a card for.
#[tokio::test]
async fn subscribing_an_existing_customer_sends_no_card_and_no_person() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/initialize/with-customer"))
        .and(body_json(json!({
            "locale": "tr",
            "customerReferenceCode": SUBSCRIBER,
            "pricingPlanReferenceCode": PLAN,
            "subscriptionInitialStatus": "ACTIVE",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": subscription_item("ACTIVE"),
        })))
        .mount(&server)
        .await;

    let subscription = client(&server)
        .subscribe(SUBSCRIBER, PLAN, InitialStatus::Active)
        .await
        .expect("the subscription starts");

    assert_eq!(&*subscription.reference_code, SUBSCRIPTION);
    assert_eq!(subscription.status, Some(SubscriptionStatus::Active));
    assert_eq!(subscription.plan_reference.as_deref(), Some(PLAN));
    assert_eq!(subscription.trial_days, Some(14));
    // Epoch milliseconds here, `YYYY-MM-DD hh:mm:ss` elsewhere for the same
    // kind of field, so it is kept as iyzico's own bytes.
    assert_eq!(subscription.start_date.as_deref(), Some("1770000000000"));
}

#[tokio::test]
async fn a_status_iyzico_has_started_sending_is_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/v2/subscription/subscriptions/{SUBSCRIPTION}"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": subscription_item("SOMETHING_NEW"),
        })))
        .mount(&server)
        .await;

    let subscription = client(&server)
        .subscription(SUBSCRIPTION)
        .await
        .expect("a subscription somebody is paying for");
    assert_eq!(
        subscription.status,
        Some(SubscriptionStatus::Other("SOMETHING_NEW".into()))
    );
}

#[tokio::test]
async fn activating_and_cancelling_name_the_subscription_in_the_path() {
    for (action, expected) in [("activate", "activate"), ("cancel", "cancel")] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!(
                "/v2/subscription/subscriptions/{SUBSCRIPTION}/{expected}"
            )))
            .and(query_param("locale", "tr"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
            .mount(&server)
            .await;

        let client = client(&server);
        match action {
            "activate" => client.activate(SUBSCRIPTION).await,
            _ => client.cancel(SUBSCRIPTION).await,
        }
        .expect("iyzico accepts it");
    }
}

/// The three decisions with money in them, and their defaults.
#[tokio::test]
async fn an_upgrade_sends_when_it_applies_and_what_it_does_not_reset() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!(
            "/v2/subscription/subscriptions/{SUBSCRIPTION}/upgrade"
        )))
        .and(body_json(json!({
            "locale": "tr",
            "newPricingPlanReferenceCode": PLAN,
            "upgradePeriod": "NOW",
            "useTrial": false,
            "resetRecurrenceCount": false,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .mount(&server)
        .await;

    client(&server)
        .upgrade(SUBSCRIPTION, &Upgrade::to(PLAN))
        .await
        .expect("the plan changes");
}

/// A card update names the subscription or the subscriber, and this names
/// neither — so no mock is mounted.
#[tokio::test]
async fn a_card_update_against_nothing_never_reaches_iyzico() {
    let server = MockServer::start().await;
    let error = client(&server)
        .start_card_update_form(None, None, "https://merchant.test/card")
        .await
        .expect_err("there is nothing to update the card of");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}

#[tokio::test]
async fn a_card_update_form_opens_against_one_subscription() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/card-update/checkoutform/initialize"))
        .and(body_json(json!({
            "locale": "tr",
            "callbackUrl": "https://merchant.test/card",
            "subscriptionReferenceCode": SUBSCRIPTION,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "token": "card-token-1",
            "checkoutFormContent": "<script>iyzico</script>",
        })))
        .mount(&server)
        .await;

    let form = client(&server)
        .start_card_update_form(Some(SUBSCRIPTION), None, "https://merchant.test/card")
        .await
        .expect("the form opens");
    assert_eq!(&*form.token, "card-token-1");
}

#[tokio::test]
async fn a_retry_names_the_order_rather_than_the_subscription() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/subscription/operation/retry"))
        .and(body_json(json!({
            "locale": "tr",
            "referenceCode": "order-ref-1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .mount(&server)
        .await;

    client(&server)
        .retry_payment("order-ref-1")
        .await
        .expect("iyzico takes it again");
}

#[tokio::test]
async fn subscribers_are_listed_a_page_at_a_time() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/subscription/customers"))
        .and(query_param("page", "1"))
        .and(query_param("count", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": {
                "totalCount": 1,
                "currentPage": 1,
                "pageCount": 1,
                "items": [{
                    "referenceCode": SUBSCRIBER,
                    "name": "Ayse",
                    "surname": "Yilmaz",
                    "email": "ayse@example.test",
                    "gsmNumber": "+905350000000",
                }],
            },
        })))
        .mount(&server)
        .await;

    let page = client(&server)
        .subscribers(1, 10)
        .await
        .expect("the listing reads back");
    assert_eq!(page.items.len(), 1);
    assert_eq!(&*page.items[0].reference_code, SUBSCRIBER);
    assert_eq!(page.items[0].email.as_deref(), Some("ayse@example.test"));
}
