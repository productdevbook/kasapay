//! PayTR against a mock server, and the four hashes it signs with.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use kasapay_core::{Currency, ErrorKind, IdSource, Money, NextAction, OrderRef, Provider, Status};
use kasapay_paytr::{Config, Credentials, PayTr, payment, payment_id};
use serde_json::json;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn credentials() -> Credentials {
    Credentials::new("merchant-1", "merchant-key", "merchant-salt")
}

fn client(server: &MockServer) -> PayTr {
    let config = Config::at(&server.uri(), credentials())
        .expect("valid base")
        .test_mode();
    PayTr::new(config).expect("client builds")
}

fn payment() -> payment::Payment {
    payment::Payment::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
        payment::Payer {
            email: "ayse@example.test".into(),
            ip: "203.0.113.7".into(),
            name: "Ayse Yilmaz".into(),
            address: "Bagdat Cad. 1".into(),
            phone: "+905350000000".into(),
            success_url: "https://merchant.test/ok".parse().expect("valid url"),
            failure_url: "https://merchant.test/no".parse().expect("valid url"),
        },
    )
    .item(payment::BasketItem {
        name: "Kahve".into(),
        price: Money::parse("149.90", Currency::Try).expect("valid amount"),
        quantity: 1,
    })
    .build()
    .expect("valid payment")
}

/// A currency PayTR does not take is refused before anything is sent.
///
/// It used to be signed into the token as an empty string and posted: PayTR
/// takes TL, EUR, USD, GBP and RUB, and a payment in yen went out with no
/// currency on it at all. The mock server here answers nothing, so a request
/// reaching it fails the test rather than passing quietly.
#[tokio::test]
async fn a_currency_paytr_does_not_take_never_reaches_the_wire() {
    let server = MockServer::start().await;
    let payment = payment::Payment::builder(
        OrderRef::new("ord-1"),
        Money::parse("1200", Currency::Jpy).expect("valid amount"),
        payment::Payer {
            email: "ayse@example.test".into(),
            ip: "203.0.113.7".into(),
            name: "Ayse Yilmaz".into(),
            address: "Bagdat Cad. 1".into(),
            phone: "+905350000000".into(),
            success_url: "https://merchant.test/ok".parse().expect("valid url"),
            failure_url: "https://merchant.test/no".parse().expect("valid url"),
        },
    )
    .item(payment::BasketItem {
        name: "Kahve".into(),
        price: Money::parse("1200", Currency::Jpy).expect("valid amount"),
        quantity: 1,
    })
    .build()
    .expect("valid payment");

    let error = client(&server)
        .start_payment(&payment)
        .await
        .expect_err("PayTR does not settle in yen");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[tokio::test]
async fn opening_a_payment_signs_the_documented_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/get-token"))
        // Minor units, as PayTR wants: 149.90 goes as 14990.
        .and(body_string_contains("payment_amount=14990"))
        .and(body_string_contains("merchant_oid=ord-1"))
        // Lira is TL to PayTR, not TRY.
        .and(body_string_contains("currency=TL"))
        // Computed independently from PayTR's own formula.
        .and(body_string_contains(urlencoding(
            "fYtW58G/x2bj+w89dUrab6MyxiTd9WMUHZC/cN6fP1o=",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "token": "form-token-1",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .start_payment(&payment())
        .await
        .expect("the payment opens");

    assert_eq!(charge.status, Status::RequiresAction);
    assert_eq!(charge.amount.minor_units(), 14_990);
    // PayTR has no id of its own: the order reference is the payment, and the
    // charge says as much rather than passing it off as PayTR's.
    let id = charge.id.as_ref().expect("an opened payment is named");
    assert_eq!(id.as_str(), "ord-1");
    assert!(
        matches!(id.source(), IdSource::Derived(_)),
        "PayTR issues no identifier for a payment"
    );
    match charge.next_action.expect("a form to send the payer to") {
        NextAction::Redirect { url, continuation } => {
            assert!(url.as_str().ends_with("/odeme/guvenli/form-token-1"));
            assert_eq!(continuation.as_deref(), Some("form-token-1"));
        }
        other => panic!("expected a redirect, got {other:?}"),
    }
}

/// What `reqwest`'s form encoder does to a base64 token.
fn urlencoding(value: &str) -> String {
    value
        .replace('+', "%2B")
        .replace('/', "%2F")
        .replace('=', "%3D")
}

#[tokio::test]
async fn a_refused_token_carries_paytrs_own_error_number() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/api/get-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed",
            "reason": "gecersiz paytr_token",
            "err_no": "007",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .start_payment(&payment())
        .await
        .expect_err("a refused token is not a payment");
    assert_eq!(error.kind(), ErrorKind::InvalidRequest);
    assert_eq!(error.code(), Some("007"));
}

#[tokio::test]
async fn reading_a_payment_back_names_it_by_the_order_reference() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/durum-sorgu"))
        .and(body_string_contains("merchant_oid=ord-1"))
        .and(body_string_contains(urlencoding(
            "S/fQPpCh73HbzZKzjIYGi8Cp7IBtQ0uAP0VjUA+l2ho=",
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            // The order came to 149.90 and the payer was charged 164.89 for
            // taking it in instalments.
            "payment_amount": "149.90",
            "payment_total": "164.89",
            "currency": "TL",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&payment_id(&OrderRef::new("ord-1")))
        .await
        .expect("the payment reads back");
    assert_eq!(charge.status, Status::Captured);
    // What moved, not what the basket came to.
    assert_eq!(charge.amount.minor_units(), 16_489);
    assert_eq!(charge.amount.currency(), Currency::Try);
}

/// PayTR settles in roubles and this crate opens payments in them, so reading
/// one back must not answer that kasapay has no currency for it.
#[tokio::test]
async fn a_payment_settled_in_roubles_reads_back() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/durum-sorgu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payment_amount": "1499.00",
            "payment_total": "1499.00",
            "currency": "RUB",
        })))
        .mount(&server)
        .await;

    let charge = client(&server)
        .charge_status(&payment_id(&OrderRef::new("ord-1")))
        .await
        .expect("the payment reads back");
    assert_eq!(charge.amount.currency(), Currency::Rub);
}

#[tokio::test]
async fn refunds_come_off_the_same_status_query() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/durum-sorgu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payment_amount": "149.90",
            "payment_total": "149.90",
            "currency": "TL",
            "returns": [
                {
                    "return_amount": "50.00",
                    "return_date": "2026-08-14 10:00:00",
                    "date_completed": "2026-08-14 10:01:00",
                    "return_ref_num": "ref-1",
                },
                { "return_amount": "20.00", "return_date": "2026-08-14 11:00:00" },
            ],
        })))
        .mount(&server)
        .await;

    let refunds = client(&server)
        .refunds(&OrderRef::new("ord-1"))
        .await
        .expect("the refunds read back");

    assert_eq!(refunds.len(), 2);
    assert_eq!(refunds[0].amount.minor_units(), 5000);
    assert_eq!(refunds[0].reference.as_deref(), Some("ref-1"));
    // A refund that has not settled yet has no completion time.
    assert!(refunds[1].completed.is_none());

    // "Is this fully refunded" is a sum, not a status — which is the whole
    // argument on #41.
    let total = refunds
        .iter()
        .try_fold(Money::from_minor_units(0, Currency::Try), |sum, refund| {
            sum.checked_add(refund.amount)
        });
    assert_eq!(total.expect("same currency").minor_units(), 7000);
}

#[tokio::test]
async fn a_payment_with_no_refunds_is_an_empty_list() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/durum-sorgu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "success",
            "payment_amount": "149.90",
            "payment_total": "149.90",
            "currency": "TL",
        })))
        .mount(&server)
        .await;

    // The field is absent, not empty, on a payment nobody has refunded.
    let refunds = client(&server)
        .refunds(&OrderRef::new("ord-1"))
        .await
        .expect("no refunds is a valid answer");
    assert!(refunds.is_empty());
}

#[tokio::test]
async fn a_payment_paytr_does_not_know_is_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/odeme/durum-sorgu"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "failed",
            "err_no": "010",
            "reason": "islem bulunamadi",
        })))
        .mount(&server)
        .await;

    let error = client(&server)
        .charge_status(&payment_id(&OrderRef::new("ord-nope")))
        .await
        .expect_err("no such payment");
    assert_eq!(error.kind(), ErrorKind::NotFound);
}

#[tokio::test]
async fn starting_a_payment_through_the_shared_trait_is_refused_with_the_way_out() {
    let server = MockServer::start().await;
    // No mock: a request reaching the network would fail the test.
    let request = kasapay_core::ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("149.90", Currency::Try).expect("valid amount"),
    )
    .build()
    .expect("valid request");

    let error = client(&server)
        .charge(&request)
        .await
        .expect_err("PayTR needs more than a ChargeRequest carries");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert!(error.to_string().contains("start_payment"));
}

#[tokio::test]
async fn listing_saved_instruments_is_refused_with_the_way_out() {
    let server = MockServer::start().await;
    // No mock: a request reaching the network would fail the test.
    let error = client(&server)
        .instruments("utoken-1")
        .await
        .expect_err("this crate cannot sign a cardstorage request");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}

#[test]
fn a_payment_needs_an_email_a_payer_ip_and_a_basket() {
    let payer = || payment::Payer {
        email: "ayse@example.test".into(),
        ip: "203.0.113.7".into(),
        name: "Ayse Yilmaz".into(),
        address: "Bagdat Cad. 1".into(),
        phone: "+905350000000".into(),
        success_url: "https://merchant.test/ok".parse().expect("valid url"),
        failure_url: "https://merchant.test/no".parse().expect("valid url"),
    };
    let amount = Money::parse("149.90", Currency::Try).expect("valid amount");
    let item = || payment::BasketItem {
        name: "Kahve".into(),
        price: amount,
        quantity: 1,
    };

    assert_eq!(
        payment::Payment::builder(OrderRef::new("ord-1"), amount, payer())
            .build()
            .expect_err("no basket"),
        payment::PaymentError::EmptyBasket
    );
    let mut without_ip = payer();
    without_ip.ip = "".into();
    assert_eq!(
        payment::Payment::builder(OrderRef::new("ord-1"), amount, without_ip)
            .item(item())
            .build()
            .expect_err("PayTR refuses a token without the payer's IP"),
        payment::PaymentError::NoPayerIp
    );
}

/// Every one of these is a code and a message from PayTR's own error list.
#[tokio::test]
async fn a_refund_paytr_says_to_retry_is_retryable() {
    for (code, reason) in [
        ("000", "iade yapilamiyor, daha sonra tekrar deneyin"),
        ("010", "Net bakiyeniz yetersiz"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/odeme/iade"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "failed",
                "err_no": code,
                "reason": reason,
            })))
            .mount(&server)
            .await;

        let error = client(&server)
            .refund(
                &OrderRef::new("ord-1"),
                Money::parse("50.00", Currency::Try).expect("valid amount"),
            )
            .await
            .expect_err("a refused refund is not a refund");
        assert_eq!(error.code(), Some(code));
        assert!(error.is_retryable(), "{code} says to try again later");
    }
}

#[tokio::test]
async fn a_refund_paytr_will_never_accept_is_not_retryable() {
    for (code, reason, kind) in [
        (
            "009",
            "Toplam iade tutari odeme tutarindan fazla olamaz",
            ErrorKind::InvalidRequest,
        ),
        (
            "011",
            "Bir yildan eski islemler icin iade islemi yapilamaz.",
            ErrorKind::InvalidRequest,
        ),
        (
            "008",
            "XYZ odeme tipi iade desteklemiyor",
            ErrorKind::Unsupported,
        ),
        (
            "005",
            "merchant_oid ile basarili odeme bulunamadi",
            ErrorKind::NotFound,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/odeme/iade"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "failed",
                "err_no": code,
                "reason": reason,
            })))
            .mount(&server)
            .await;

        let error = client(&server)
            .refund(
                &OrderRef::new("ord-1"),
                Money::parse("50.00", Currency::Try).expect("valid amount"),
            )
            .await
            .expect_err("a refused refund is not a refund");
        assert_eq!(error.kind(), kind, "code {code}");
        assert!(!error.is_retryable(), "{code} will never be accepted");
    }
}
