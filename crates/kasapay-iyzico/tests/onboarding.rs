//! Onboarding against a mock server.
//!
//! Neither documentation language gives a worked example — a request or a
//! response with real values — for any of the three sub-merchant operations,
//! only the schema: field names, types, and which are required. So these
//! fixtures are built from that schema and nothing else, the same as
//! `mass`'s `authorize`, `cancel`, `balance` and single-item read, which are
//! undemonstrated the same way. The values are stand-ins. No live marketplace
//! account was available to check any of this against, and no test here
//! claims otherwise.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, Money};
use kasapay_iyzico::Credentials;
use kasapay_iyzico::classic;
use kasapay_iyzico::onboarding::{
    Client, CompanyUpdate, LimitedJointSubmerchant, NewSubmerchant, PersonalSubmerchant,
    PersonalUpdate, SubmerchantKind, SubmerchantUpdate,
};
use serde_json::json;
use wiremock::matchers::{body_json, header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const IBAN: &str = "TR920086402100002353983528";

fn client(server: &MockServer) -> Client {
    let config = classic::Config::new(&server.uri(), Credentials::new("api-key", "secret-key"))
        .expect("valid base");
    Client::new(classic::Client::new(config).expect("client builds"))
}

fn a_personal_submerchant() -> NewSubmerchant {
    NewSubmerchant::Personal(
        PersonalSubmerchant::builder(
            "ext-1",
            "ayse@example.com",
            "+905555856935",
            "Kadıköy, İstanbul",
            "Ayşe",
            "Yılmaz",
            "11111111110",
        )
        .name("Ayşe Butik")
        .iban(IBAN)
        .currency(Currency::Try)
        .conversation_id("conv-1")
        .build()
        .expect("every field iyzico documents as required is present"),
    )
}

#[tokio::test]
async fn creating_a_personal_submerchant_sends_the_body_iyzico_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant"))
        .and(header("content-type", "application/json"))
        .and(header_exists("x-iyzi-rnd"))
        .and(body_json(json!({
            "subMerchantType": "PERSONAL",
            "name": "Ayşe Butik",
            "email": "ayse@example.com",
            "gsmNumber": "+905555856935",
            "address": "Kadıköy, İstanbul",
            "iban": IBAN,
            "contactName": "Ayşe",
            "contactSurname": "Yılmaz",
            "subMerchantExternalId": "ext-1",
            "identityNumber": "11111111110",
            "currency": "TRY",
            "locale": "tr",
            "conversationId": "conv-1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
            "conversationId": "conv-1",
            "subMerchantKey": "sub-merchant-key-1",
        })))
        .mount(&server)
        .await;

    let created = client(&server)
        .create(&a_personal_submerchant())
        .await
        .expect("the sub-merchant is created");

    assert_eq!(&*created.key, "sub-merchant-key-1");
}

#[tokio::test]
async fn creating_a_limited_joint_submerchant_omits_the_fields_that_were_never_set() {
    let server = MockServer::start().await;
    let submerchant = NewSubmerchant::LimitedOrJointStockCompany(
        LimitedJointSubmerchant::builder(
            "ext-2",
            "info@acme.example",
            "+905555856936",
            "Kadıköy, İstanbul",
            "Kadıköy V.D.",
            "1234567890",
            "Acme Perakende A.Ş.",
        )
        .build()
        .expect("every field iyzico documents as required is present"),
    );

    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant"))
        .and(body_json(json!({
            "subMerchantType": "LIMITED_OR_JOINT_STOCK_COMPANY",
            "email": "info@acme.example",
            "gsmNumber": "+905555856936",
            "address": "Kadıköy, İstanbul",
            "taxOffice": "Kadıköy V.D.",
            "taxNumber": "1234567890",
            "legalCompanyTitle": "Acme Perakende A.Ş.",
            "subMerchantExternalId": "ext-2",
            "locale": "tr",
            // No name, iban, identityNumber, currency or conversationId: none
            // were set, and none of the three creation schemas requires them
            // for this kind.
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "subMerchantKey": "sub-merchant-key-2",
        })))
        .mount(&server)
        .await;

    let created = client(&server)
        .create(&submerchant)
        .await
        .expect("the sub-merchant is created");

    assert_eq!(&*created.key, "sub-merchant-key-2");
}

#[tokio::test]
async fn updating_a_personal_submerchant_sends_no_submerchant_type() {
    let server = MockServer::start().await;
    let update = SubmerchantUpdate::Personal(
        PersonalUpdate::builder(
            "sub-merchant-key-1",
            "ayse@example.com",
            "+905555856935",
            "Kadıköy, İstanbul",
            IBAN,
            "Ayşe",
            "Yılmaz",
            "11111111110",
        )
        .build()
        .expect("every field an update requires is present"),
    );

    Mock::given(method("PUT"))
        .and(path("/onboarding/submerchant"))
        .and(body_json(json!({
            "email": "ayse@example.com",
            "gsmNumber": "+905555856935",
            "address": "Kadıköy, İstanbul",
            "iban": IBAN,
            "contactName": "Ayşe",
            "contactSurname": "Yılmaz",
            "identityNumber": "11111111110",
            "subMerchantKey": "sub-merchant-key-1",
            "locale": "tr",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
        })))
        .mount(&server)
        .await;

    client(&server)
        .update(&update)
        .await
        .expect("the sub-merchant is updated");

    // iyzico's own words: do not send subMerchantType on an update.
    let sent: Vec<Request> = server.received_requests().await.expect("recorded");
    let body = String::from_utf8(sent.first().expect("one request").body.clone())
        .expect("the body is utf-8");
    assert!(!body.contains("subMerchantType"), "{body}");
}

#[tokio::test]
async fn a_private_company_and_a_limited_joint_update_send_the_same_shape() {
    let server = MockServer::start().await;
    let company = CompanyUpdate::builder(
        "sub-merchant-key-3",
        "info@acme.example",
        "+905555856936",
        "Kadıköy, İstanbul",
        IBAN,
        "Acme Perakende A.Ş.",
        "Kadıköy V.D.",
        "1234567890",
    )
    .build()
    .expect("every field an update requires is present");

    let expected_body = json!({
        "email": "info@acme.example",
        "gsmNumber": "+905555856936",
        "address": "Kadıköy, İstanbul",
        "iban": IBAN,
        "taxOffice": "Kadıköy V.D.",
        "legalCompanyTitle": "Acme Perakende A.Ş.",
        "identityNumber": "1234567890",
        "subMerchantKey": "sub-merchant-key-3",
        "locale": "tr",
    });

    Mock::given(method("PUT"))
        .and(path("/onboarding/submerchant"))
        .and(body_json(expected_body.clone()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "status": "success" })))
        .expect(2)
        .mount(&server)
        .await;

    // The same body iyzico documents for both kinds — see the module
    // documentation for why they share one struct.
    client(&server)
        .update(&SubmerchantUpdate::PrivateCompany(company.clone()))
        .await
        .expect("a private company update is accepted");
    client(&server)
        .update(&SubmerchantUpdate::LimitedOrJointStockCompany(company))
        .await
        .expect("a limited/joint-stock company update is accepted");
}

#[tokio::test]
async fn reading_a_submerchants_detail_reads_the_fields_iyzico_documents() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant/detail"))
        .and(body_json(json!({
            "locale": "tr",
            "subMerchantExternalId": "ext-1",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "conversationId": "conv-1",
            "name": "Ayşe Butik",
            "email": "ayse@example.com",
            "gsmNumber": "+905555856935",
            "address": "Kadıköy, İstanbul",
            "iban": IBAN,
            "bankCountry": "Turkey",
            "currency": "TRY",
            "taxOffice": null,
            "legalCompanyTitle": null,
            "subMerchantExternalId": "ext-1",
            "identityNumber": "11111111110",
            "subMerchantType": "PERSONAL",
            "subMerchantKey": "sub-merchant-key-1",
        })))
        .mount(&server)
        .await;

    let detail = client(&server)
        .detail("ext-1")
        .await
        .expect("the sub-merchant is read back");

    assert_eq!(detail.key.as_deref(), Some("sub-merchant-key-1"));
    assert_eq!(detail.kind, Some(SubmerchantKind::Personal));
    assert_eq!(detail.currency, Some(Currency::Try));
    assert_eq!(
        detail.iban.as_ref().map(kasapay_core::Secret::expose),
        Some(IBAN)
    );
    // The account number itself never appears in a Debug of the answer.
    assert!(!format!("{detail:?}").contains(IBAN));
}

#[tokio::test]
async fn a_currency_kasapay_cannot_name_reads_back_as_none_and_stays_in_raw() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant/detail"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "subMerchantExternalId": "ext-9",
            "currency": "SEK",
        })))
        .mount(&server)
        .await;

    let detail = client(&server)
        .detail("ext-9")
        .await
        .expect("the sub-merchant is read back");

    assert_eq!(detail.currency, None);
    assert_eq!(detail.raw.text_at("/currency").as_deref(), Some("SEK"));
}

#[tokio::test]
async fn a_refusal_is_an_error_carrying_iyzicos_own_code() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failure",
            "errorCode": "5093",
            "errorMessage": "subMerchantExternalId is already in use",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .create(&a_personal_submerchant())
        .await
        .expect_err("a failure status is not a created sub-merchant");

    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("5093"));
}

#[tokio::test]
async fn the_request_is_signed_over_the_path_and_the_exact_bytes_sent() {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/onboarding/submerchant"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "subMerchantKey": "sub-merchant-key-1",
        })))
        .mount(&server)
        .await;

    client(&server)
        .create(&a_personal_submerchant())
        .await
        .expect("the sub-merchant is created");

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
    let body = String::from_utf8(request.body.clone()).expect("utf-8");

    let expected = Credentials::new("api-key", "secret-key").signature(
        random_key,
        "/onboarding/submerchant",
        &body,
    );
    assert!(
        decoded.ends_with(&format!("signature:{expected}")),
        "the signature covered something other than the path and the body"
    );
}

/// The split id, which is the basket line rather than the payment.
const SPLIT: &str = "17652320";

#[tokio::test]
async fn approving_a_line_names_the_split_and_not_the_payment() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/item/approve"))
        .and(header_exists("authorization"))
        .and(body_json(json!({
            "locale": "tr",
            "paymentTransactionId": SPLIT,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "locale": "tr",
            "systemTime": 1_770_000_000_i64,
            "paymentTransactionId": SPLIT,
        })))
        .mount(&server)
        .await;

    let approved = client(&server)
        .approve_item(SPLIT)
        .await
        .expect("the sub-merchant's share is released");
    assert_eq!(&*approved.transaction, SPLIT);
}

#[tokio::test]
async fn disapproving_a_line_holds_the_share_again() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/item/disapprove"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentTransactionId": SPLIT,
        })))
        .mount(&server)
        .await;

    let held = client(&server)
        .disapprove_item(SPLIT)
        .await
        .expect("the approval is revoked");
    assert_eq!(&*held.transaction, SPLIT);
}

/// Acting on the wrong split pays the wrong seller, so the echo is checked.
#[tokio::test]
async fn an_answer_about_another_split_is_not_this_calls() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/payment/iyzipos/item/approve"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "paymentTransactionId": "99999999",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .approve_item(SPLIT)
        .await
        .expect_err("that is another line's answer");
    assert_eq!(error.kind(), ErrorKind::Malformed);
}

/// iyzico types these amounts as JSON numbers, and no money here goes through
/// an `f64` in either direction.
#[tokio::test]
async fn changing_a_share_sends_a_number_and_reads_the_arithmetic_back() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/payment/item"))
        .and(body_json(json!({
            "locale": "tr",
            "paymentTransactionId": SPLIT,
            "subMerchantKey": "sub-merchant-key-1",
            // A number, not a string.
            "subMerchantPrice": 90.00,
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "itemId": "item-1",
            "paymentTransactionId": SPLIT,
            "transactionStatus": 2,
            "price": 100.00,
            "paidPrice": 100.00,
            "subMerchantKey": "sub-merchant-key-1",
            "subMerchantPrice": 90.00,
            "subMerchantPayoutAmount": 88.20,
            "merchantPayoutAmount": 9.80,
            "blockageResolvedDate": "2026-08-26 10:00:00",
        })))
        .mount(&server)
        .await;

    let payout = client(&server)
        .update_item_payout(
            SPLIT,
            "sub-merchant-key-1",
            Money::parse("90.00", Currency::Try).expect("valid amount"),
        )
        .await
        .expect("iyzico answers the new arithmetic");

    assert_eq!(payout.item_id.as_deref(), Some("item-1"));
    assert_eq!(payout.transaction_status, Some(2));
    assert_eq!(
        payout.submerchant_price.map(Money::minor_units),
        Some(9_000)
    );
    // What actually reaches them, after iyzico's blockage.
    assert_eq!(
        payout.submerchant_payout.map(Money::minor_units),
        Some(8_820)
    );
    assert_eq!(payout.merchant_payout.map(Money::minor_units), Some(980));
    assert_eq!(
        payout.blockage_resolved.as_deref(),
        Some("2026-08-26 10:00:00")
    );
}

/// No mock is mounted: nothing is paid out for nothing.
#[tokio::test]
async fn a_share_of_nothing_never_reaches_iyzico() {
    let server = MockServer::start().await;
    let error = client(&server)
        .update_item_payout(
            SPLIT,
            "sub-merchant-key-1",
            Money::from_minor_units(0, Currency::Try),
        )
        .await
        .expect_err("zero is not a share");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
}
