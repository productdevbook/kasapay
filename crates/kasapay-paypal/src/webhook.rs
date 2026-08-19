//! Checking that a delivery to a webhook address really came from PayPal.
//!
//! PayPal is the one provider here that verifies its own deliveries. There is
//! a signature — `PAYPAL-TRANSMISSION-SIG`, over a digest of the body — but
//! checking it locally means fetching and validating the certificate chain at
//! `PAYPAL-CERT-URL` and trusting whatever address that header names. PayPal's
//! documented answer is `POST /v1/notifications/verify-webhook-signature`,
//! which takes the five headers and the body and answers `SUCCESS` or
//! `FAILURE`, and that is what this does.
//!
//! So verifying a PayPal delivery is a request to PayPal, and it costs a round
//! trip. That is the whole reason
//! [`Webhook::verify`](kasapay_core::Webhook::verify) is `async`.

use kasapay_core::{
    Delivery, Error, ErrorKind, Event, EventId, EventKind, Money, PaymentId, ProviderId, Raw,
    Webhook, async_trait,
};
use serde_json::value::RawValue;

use crate::PAYPAL;
use crate::client::PayPal;
use crate::convert;

/// The five headers PayPal signs a delivery with.
///
/// All five go back to PayPal unchanged; a delivery missing any one of them is
/// not one PayPal sent.
const AUTH_ALGO: &str = "PAYPAL-AUTH-ALGO";
const CERT_URL: &str = "PAYPAL-CERT-URL";
const TRANSMISSION_ID: &str = "PAYPAL-TRANSMISSION-ID";
const TRANSMISSION_SIG: &str = "PAYPAL-TRANSMISSION-SIG";
const TRANSMISSION_TIME: &str = "PAYPAL-TRANSMISSION-TIME";

/// Verifies deliveries PayPal makes to one webhook address.
///
/// `webhook_id` is the id of the webhook as configured in the PayPal
/// dashboard, not the id of any delivery. It is what tells PayPal which
/// address's configuration to check the signature against, and a delivery to
/// one address does not verify against another's.
#[derive(Debug, Clone)]
pub struct Webhooks {
    paypal: PayPal,
    webhook_id: Box<str>,
}

impl Webhooks {
    /// Verifies against one configured webhook address, over an existing
    /// client.
    ///
    /// The client is what holds the credentials the verification call itself
    /// is made with — verifying a delivery is an authenticated request like
    /// any other.
    #[must_use]
    pub fn new(paypal: PayPal, webhook_id: impl Into<Box<str>>) -> Self {
        Self {
            paypal,
            webhook_id: webhook_id.into(),
        }
    }
}

/// The body of the verification call, with the delivery inside it.
///
/// `webhook_event` is a [`RawValue`]: the bytes PayPal sent, passed through
/// rather than parsed and printed again. A re-serialised object is not the
/// same bytes — key order and spacing both move — and the point of this call
/// is to ask about what actually arrived.
#[derive(serde::Serialize)]
struct VerifyRequest<'a> {
    auth_algo: &'a str,
    cert_url: &'a str,
    transmission_id: &'a str,
    transmission_sig: &'a str,
    transmission_time: &'a str,
    webhook_id: &'a str,
    webhook_event: &'a RawValue,
}

#[derive(serde::Deserialize)]
struct VerifyResponse {
    verification_status: Option<String>,
}

#[async_trait]
impl Webhook for Webhooks {
    fn provider(&self) -> ProviderId {
        PAYPAL
    }

    /// Asks PayPal whether this delivery is theirs, and only then reads it.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Untrusted`] for a delivery missing any of the five headers
    /// PayPal signs with, and for one PayPal answers `FAILURE` for. A
    /// verification call that cannot be made at all — PayPal is down, the
    /// credentials are wrong — is that failure's own kind rather than
    /// `Untrusted`: not knowing is not the same as knowing it is forged, and
    /// the two want different handling. Neither is a delivery to act on.
    async fn verify(&self, delivery: &Delivery<'_>) -> Result<Event, Error> {
        let header = |name: &'static str| {
            delivery.header(name).ok_or_else(|| {
                Error::new(
                    ErrorKind::Untrusted,
                    PAYPAL,
                    format!("the delivery carried no {name} header"),
                )
            })
        };
        let body = delivery.body_str().ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "a delivery whose body is not UTF-8",
            )
        })?;
        let event: &RawValue = serde_json::from_str(body).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "a delivery whose body is not JSON",
            )
            .with_source(e)
        })?;

        let raw = self
            .paypal
            .send(
                reqwest::Method::POST,
                "/v1/notifications/verify-webhook-signature",
                Some(&VerifyRequest {
                    auth_algo: header(AUTH_ALGO)?,
                    cert_url: header(CERT_URL)?,
                    transmission_id: header(TRANSMISSION_ID)?,
                    transmission_sig: header(TRANSMISSION_SIG)?,
                    transmission_time: header(TRANSMISSION_TIME)?,
                    webhook_id: &self.webhook_id,
                    webhook_event: event,
                }),
                false,
                None,
            )
            .await?;
        let verified: VerifyResponse = serde_json::from_str(raw.as_str()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "the verification answer was not the JSON PayPal documents",
            )
            .with_source(e)
        })?;
        if verified.verification_status.as_deref() != Some("SUCCESS") {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PAYPAL,
                "PayPal does not recognise this delivery as one of theirs",
            ));
        }

        read_event(body)
    }
}

/// What a verified body says, in the shared words.
fn read_event(body: &str) -> Result<Event, Error> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            "a verified delivery whose body is not the JSON PayPal documents",
        )
        .with_source(e)
    })?;
    let missing = |field: &str| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("a delivery carried no {field}"),
        )
    };
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| missing("id, and there is nothing to deduplicate it by"))?;
    let event_type = value
        .get("event_type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| missing("event_type"))?;

    Ok(Event {
        id: EventId::issued(id),
        kind: event_kind(event_type),
        payment: order_id(event_type, &value).map(PaymentId::issued),
        amount: amount(&value),
        provider: PAYPAL,
        raw: Raw::from_text(body),
    })
}

/// What one of PayPal's event types means here.
///
/// The `PAYMENT.*` family is what moves money and is what is named.
/// `CHECKOUT.ORDER.*` is deliberately [`EventKind::Other`]: an order being
/// approved is the payer finishing at PayPal rather than a payment, and
/// `CHECKOUT.ORDER.COMPLETED` arrives beside the capture event for the same
/// money — a caller acting on both counts one payment twice.
fn event_kind(event_type: &str) -> EventKind {
    match event_type {
        "PAYMENT.CAPTURE.COMPLETED" => EventKind::Captured,
        "PAYMENT.CAPTURE.DENIED" => EventKind::Failed,
        // Refunded is a refund the merchant made; reversed is one PayPal made
        // for them. The money went back either way.
        "PAYMENT.CAPTURE.REFUNDED" | "PAYMENT.CAPTURE.REVERSED" => EventKind::Refunded,
        "PAYMENT.AUTHORIZATION.CREATED" => EventKind::Authorized,
        "PAYMENT.AUTHORIZATION.VOIDED" => EventKind::Canceled,
        other => EventKind::Other(other.into()),
    }
}

/// The order the delivery is about, which is what kasapay names a PayPal
/// payment by.
///
/// A capture, an authorization and a refund each carry the order they came off
/// under `supplementary_data.related_ids.order_id` rather than at the top of
/// the resource — PayPal's own answer to "which order is this". A
/// `CHECKOUT.ORDER.*` delivery is about the order itself, so there the
/// resource's own id is it.
fn order_id(event_type: &str, value: &serde_json::Value) -> Option<String> {
    if event_type.starts_with("CHECKOUT.ORDER.") {
        return value
            .pointer("/resource/id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned);
    }
    value
        .pointer("/resource/supplementary_data/related_ids/order_id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

/// The figure the delivery is about, where its resource carries one.
///
/// A `CHECKOUT.ORDER.*` resource carries its amounts a level down, inside each
/// purchase unit, and an order can hold more than one — so nothing is read
/// there rather than the first of several being passed off as the total.
fn amount(value: &serde_json::Value) -> Option<Money> {
    let amount = value.pointer("/resource/amount")?;
    let currency = convert::currency_back(amount.get("currency_code")?.as_str()?).ok()?;
    Money::parse(amount.get("value")?.as_str()?, currency).ok()
}

#[cfg(test)]
mod tests {
    use super::{amount, event_kind, order_id};
    use kasapay_core::EventKind;
    use serde_json::json;

    #[test]
    fn the_payment_family_is_named_and_everything_else_keeps_paypals_name() {
        assert_eq!(event_kind("PAYMENT.CAPTURE.COMPLETED"), EventKind::Captured);
        assert_eq!(event_kind("PAYMENT.CAPTURE.REVERSED"), EventKind::Refunded);
        assert_eq!(
            event_kind("CHECKOUT.ORDER.APPROVED"),
            EventKind::Other("CHECKOUT.ORDER.APPROVED".into())
        );
        assert_eq!(
            event_kind("CUSTOMER.DISPUTE.CREATED"),
            EventKind::Other("CUSTOMER.DISPUTE.CREATED".into())
        );
    }

    /// PayPal's own `PAYMENT.CAPTURE.COMPLETED` example, cut to the fields
    /// read here.
    #[test]
    fn a_capture_names_the_order_it_came_off_rather_than_itself() {
        let body = json!({
            "id": "WH-58D329510W468432D-8HN650336L201105X",
            "event_type": "PAYMENT.CAPTURE.COMPLETED",
            "resource": {
                "id": "8MC585209K746392H",
                "amount": { "currency_code": "USD", "value": "100.00" },
                "supplementary_data": {
                    "related_ids": { "order_id": "5O190127TN364715T" }
                }
            }
        });
        assert_eq!(
            order_id("PAYMENT.CAPTURE.COMPLETED", &body).as_deref(),
            Some("5O190127TN364715T")
        );
        assert_eq!(
            amount(&body).map(kasapay_core::Money::minor_units),
            Some(10_000)
        );
    }

    /// An order event is about the order, so its own id is the payment.
    #[test]
    fn an_order_event_names_itself() {
        let body = json!({
            "id": "WH-2WR32451HC0233532-67976317FL4543714",
            "event_type": "CHECKOUT.ORDER.APPROVED",
            "resource": {
                "id": "5O190127TN364715T",
                "purchase_units": [
                    { "amount": { "currency_code": "USD", "value": "100.00" } }
                ]
            }
        });
        assert_eq!(
            order_id("CHECKOUT.ORDER.APPROVED", &body).as_deref(),
            Some("5O190127TN364715T")
        );
        // The amounts are a level down and there may be several: none is read.
        assert!(amount(&body).is_none());
    }
}
