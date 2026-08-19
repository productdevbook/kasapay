//! iyzilink against a mock server.
//!
//! The fixtures are the shapes iyzico documents: field names and nesting come
//! from the OpenAPI fragments on their iyzico Link pages, and the failure
//! envelope from the `errorCode`/`errorMessage` their own SDKs map on these
//! endpoints — the schemas themselves say only `status`. The values are
//! stand-ins. No live iyzilink account was available to check them against,
//! and no test here claims otherwise.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{Currency, ErrorKind, Money};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic;
use kasapay_iyzico::iyzilink::{
    Category, Client, FastLink, LinkError, LinkKind, LinkStatus, LinkUpdate, NewLink,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    let config = classic::Config::new(&server.uri(), Credentials::new("api-key", "secret-key"))
        .expect("valid base");
    Client::new(classic::Client::new(config).expect("client builds"))
}

/// A created or updated link: `data` with the three fields iyzico documents.
fn saved() -> serde_json::Value {
    json!({
        "status": "success",
        "locale": "tr",
        "systemTime": 1_770_000_000_i64,
        "conversationId": "ref-1",
        "data": {
            "token": "AbC123",
            "url": "https://iyzi.link/AbC123",
            "imageUrl": "https://images.iyzico.test/AbC123.png",
        },
    })
}

/// One link, with every field the product schema lists.
fn product() -> serde_json::Value {
    json!({
        "name": "Kahve",
        "conversationId": "ref-1",
        "description": "250g filtre kahve",
        "price": "149.90",
        "currencyId": 1,
        "currencyCode": "TRY",
        "token": "AbC123",
        "productType": "IYZILINK",
        "productStatus": "ACTIVE",
        "merchantId": 12345,
        "url": "https://iyzi.link/AbC123",
        "imageUrl": "https://images.iyzico.test/AbC123.png",
        "addressIgnorable": true,
        "soldCount": 3,
        "installmentRequested": false,
        "stockEnabled": true,
        "stockCount": 40,
        "flexibleLink": false,
        "categoryType": "FOOD",
    })
}

fn a_coffee() -> NewLink {
    NewLink::builder(
        "Kahve",
        "250g filtre kahve",
        Money::parse("149.90", Currency::Try).expect("a valid amount"),
        "iVBORw0KGgo=",
    )
    .stock(40)
    .category(Category::Food)
    .build()
    .expect("a link iyzico documents")
}

#[tokio::test]
async fn create_sends_the_documented_body_and_reads_the_link_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/iyzilink/products"))
        .and(header("content-type", "application/json"))
        .and(header_exists("x-iyzi-rnd"))
        .and(body_json(json!({
            "locale": "tr",
            "name": "Kahve",
            "description": "250g filtre kahve",
            "price": "149.90",
            "currencyCode": "TRY",
            "encodedImageFile": "iVBORw0KGgo=",
            "stockEnabled": true,
            "stockCount": 40,
            "categoryType": "FOOD",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved()))
        .mount(&server)
        .await;

    let link = client(&server)
        .create(&a_coffee())
        .await
        .expect("the link is created");

    assert_eq!(&*link.token, "AbC123");
    assert_eq!(
        link.url.as_ref().map(url::Url::as_str),
        Some("https://iyzi.link/AbC123")
    );
    assert_eq!(
        link.raw.text_at("/data/imageUrl").as_deref(),
        Some("https://images.iyzico.test/AbC123.png")
    );
}

#[tokio::test]
async fn a_fast_link_sends_only_what_iyzico_asks_of_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/iyzilink/fast-link/products"))
        .and(body_json(json!({
            "locale": "tr",
            "description": "Masa 4",
            "price": "120.00",
            "currencyCode": "TRY",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved()))
        .mount(&server)
        .await;

    let fast = FastLink::builder(Money::parse("120.00", Currency::Try).expect("valid"))
        .describe("Masa 4")
        .build()
        .expect("a fast link iyzico documents");
    let link = client(&server)
        .create_fast_link(&fast)
        .await
        .expect("the fast link is created");
    assert_eq!(&*link.token, "AbC123");
}

#[tokio::test]
async fn the_signature_covers_the_path_and_leaves_the_query_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/iyzilink/products"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": { "totalCount": 0, "currentPage": 1, "pageCount": 0, "items": [] },
        })))
        .mount(&server)
        .await;

    client(&server).list(2, 25).await.expect("an empty page");

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
    // as a body. Signing the query too is what iyzico's own SDKs cut off.
    assert_eq!(request.url.query(), Some("locale=tr&page=2&count=25"));
    assert!(request.body.is_empty());
    let expected = Credentials::new("api-key", "secret-key").signature(
        random_key,
        "/v2/iyzilink/products",
        "",
    );
    assert!(
        decoded.ends_with(&format!("signature:{expected}")),
        "the signature covered something other than the path"
    );
}

#[tokio::test]
async fn a_listing_reads_its_page_and_the_links_on_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/iyzilink/products"))
        .and(query_param("locale", "tr"))
        .and(query_param("page", "1"))
        .and(query_param("count", "10"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
            "data": {
                "listingReviewed": true,
                "totalCount": 2,
                "currentPage": 1,
                "pageCount": 1,
                "items": [product(), { "token": "ZzZ999", "productStatus": "PASSIVE" }],
            },
        })))
        .mount(&server)
        .await;

    let page = client(&server).list(1, 10).await.expect("a page of links");

    assert_eq!(page.total_count, Some(2));
    assert_eq!(page.page_count, Some(1));
    assert_eq!(page.listing_reviewed, Some(true));
    assert_eq!(page.links.len(), 2);

    let first = &page.links[0];
    assert_eq!(&*first.token, "AbC123");
    assert_eq!(
        first.price,
        Some(Money::parse("149.90", Currency::Try).expect("valid"))
    );
    assert_eq!(first.kind, Some(LinkKind::IyziLink));
    assert_eq!(first.status, Some(LinkStatus::Active));
    assert_eq!(first.category, Some(Category::Food));
    assert_eq!(first.sold_count, Some(3));
    assert_eq!(first.stock_count, Some(40));
    // Each link keeps its own bytes, not the whole page's.
    assert_eq!(first.raw.text_at("/name").as_deref(), Some("Kahve"));

    let second = &page.links[1];
    assert_eq!(second.status, Some(LinkStatus::Passive));
    assert_eq!(second.price, None);
}

#[tokio::test]
async fn a_read_names_the_token_in_the_path_and_sends_no_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/iyzilink/products/AbC123"))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": product(),
        })))
        .mount(&server)
        .await;

    let link = client(&server).get("AbC123").await.expect("the link");

    assert_eq!(&*link.token, "AbC123");
    assert_eq!(link.name.as_deref(), Some("Kahve"));
    assert_eq!(link.address_ignorable, Some(true));
    assert_eq!(link.instalments, Some(false));

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn an_update_replaces_what_it_carries() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v2/iyzilink/products/AbC123"))
        .and(body_json(json!({
            "locale": "tr",
            "conversationId": "ref-2",
            "name": "Kahve",
            "description": "500g filtre kahve",
            "price": "259.00",
            "currencyCode": "TRY",
            "encodedImageFile": "iVBORw0KGgo=",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(saved()))
        .mount(&server)
        .await;

    let update = LinkUpdate::builder(
        "Kahve",
        "500g filtre kahve",
        Money::parse("259.00", Currency::Try).expect("valid"),
    )
    .image("iVBORw0KGgo=")
    .conversation_id("ref-2")
    .build()
    .expect("an update iyzico documents");

    let link = client(&server)
        .update("AbC123", &update)
        .await
        .expect("the link is updated");
    assert_eq!(&*link.token, "AbC123");
}

#[tokio::test]
async fn a_status_change_names_the_status_in_the_path() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v2/iyzilink/products/AbC123/status/PASSIVE"))
        .and(query_param("locale", "tr"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
        })))
        .mount(&server)
        .await;

    client(&server)
        .set_status("AbC123", &LinkStatus::Passive)
        .await
        .expect("the link stops taking money");
}

#[tokio::test]
async fn deleting_a_link_carries_no_body_at_all() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/v2/iyzilink/products/AbC123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
        })))
        .mount(&server)
        .await;

    client(&server).delete("AbC123").await.expect("it is gone");

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    // The classic API deletes a stored card with a JSON body; this one does not.
    assert!(sent.first().expect("one request").body.is_empty());
}

#[tokio::test]
async fn a_failure_envelope_carries_iyzicos_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v2/iyzilink/products"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5088",
            "errorMessage": "Urun gorseli zorunludur",
            "locale": "tr",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .create(&a_coffee())
        .await
        .expect_err("a failure status is not a link");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5088"));
}

#[tokio::test]
async fn a_token_that_would_change_the_path_never_reaches_the_wire() {
    let server = MockServer::start().await;

    let error = client(&server)
        .get("AbC123/../../payment/detail")
        .await
        .expect_err("a token is one path segment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty(), "a hostile token opened a socket");
}

#[tokio::test]
async fn a_page_iyzico_will_not_serve_is_refused_before_a_socket_opens() {
    let server = MockServer::start().await;

    for (page, count) in [(0, 10), (1, 0), (1, 101)] {
        let error = client(&server)
            .list(page, count)
            .await
            .expect_err("iyzico pages from 1, in counts of at most 100");
        assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    }

    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    assert!(sent.is_empty());
}

#[tokio::test]
async fn a_price_in_a_currency_iyzico_does_not_document_stays_in_the_raw_body() {
    let server = MockServer::start().await;
    let mut body = product();
    body["currencyCode"] = json!("ISK");
    // And a decimal iyzico wrote as a number rather than a string.
    body["price"] = json!(149.9);
    Mock::given(method("GET"))
        .and(path("/v2/iyzilink/products/AbC123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": body,
        })))
        .mount(&server)
        .await;

    let link = client(&server).get("AbC123").await.expect("the link");

    // Icelandic króna: `Currency` leaves it out on purpose, because Stripe
    // reads it as having no minor unit where ISO gives it two — the one class
    // of currency this library will not name without a reading behind it. The
    // link still reads, and the amount stays where iyzico put it.
    assert_eq!(link.price, None);
    assert_eq!(link.raw.text_at("/currencyCode").as_deref(), Some("ISK"));
}

#[tokio::test]
async fn a_decimal_iyzico_sent_as_a_number_keeps_its_digits() {
    let server = MockServer::start().await;
    let mut body = product();
    body["price"] = json!(149.9);
    body["flexibleLink"] = json!(true);
    body["presetPriceValues"] = json!(["10.00", "20.00", "30.00"]);
    Mock::given(method("GET"))
        .and(path("/v2/iyzilink/products/AbC123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "data": body,
        })))
        .mount(&server)
        .await;

    let link = client(&server).get("AbC123").await.expect("the link");

    assert_eq!(
        link.price,
        Some(Money::parse("149.90", Currency::Try).expect("valid"))
    );
    assert_eq!(link.flexible_price, Some(true));
    assert_eq!(
        link.preset_prices.iter().map(|p| &**p).collect::<Vec<_>>(),
        ["10.00", "20.00", "30.00"]
    );
}

#[test]
fn a_currency_iyzico_does_not_take_is_refused_by_the_builder() {
    let yen = Money::parse("1500", Currency::Jpy).expect("valid");
    let error = NewLink::builder("Kahve", "250g", yen, "iVBORw0KGgo=")
        .build()
        .expect_err("iyzico documents no yen link");
    assert_eq!(error, LinkError::UnsupportedCurrency(Currency::Jpy));
}

#[test]
fn a_price_of_nothing_is_refused_by_the_builder() {
    let free = Money::from_minor_units(0, Currency::Try);
    assert!(
        FastLink::builder(free).build().is_err(),
        "a link for nothing is not a link"
    );
}
