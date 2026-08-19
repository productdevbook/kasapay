//! Checking that a delivery to a webhook address really came from Stripe.
//!
//! Stripe signs the **bytes it sent**, with a timestamp folded in so that a
//! delivery captured off the wire cannot be replayed a week later. Both halves
//! matter and both are checked here: a signature that matches a body nobody
//! can name the age of is a replay waiting to happen.
//!
//! ```no_run
//! use kasapay_core::{Delivery, Secret, Webhook as _};
//! use kasapay_stripe::Webhooks;
//!
//! # async fn handler(headers: &[(&str, &str)], body: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
//! let webhooks = Webhooks::new(Secret::new("whsec_..."));
//! let event = webhooks.verify(&Delivery::new(headers, body)).await?;
//! # let _ = event;
//! # Ok(())
//! # }
//! ```

use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use kasapay_core::{
    Currency, Delivery, Error, ErrorKind, Event, EventId, EventKind, Money, PaymentId, ProviderId,
    Raw, Secret, Webhook, async_trait,
};
use sha2::Sha256;
use subtle::ConstantTimeEq as _;

use crate::convert::PROVIDER;

/// The header Stripe puts its signature in.
const SIGNATURE_HEADER: &str = "Stripe-Signature";

/// How far a delivery's timestamp may be from now.
///
/// Stripe's own libraries default to five minutes and their documentation
/// recommends it. Shorter refuses deliveries a slow queue held on to; longer
/// widens the window a captured body can be replayed in.
pub const DEFAULT_TOLERANCE: Duration = Duration::from_secs(300);

/// Verifies deliveries Stripe makes to one webhook address.
///
/// One per endpoint secret: Stripe issues a `whsec_…` per configured webhook
/// address, and a delivery to one address does not verify against another's.
#[derive(Debug, Clone)]
pub struct Webhooks {
    secret: Secret,
    tolerance: Duration,
}

impl Webhooks {
    /// Verifies against one endpoint's signing secret — Stripe's `whsec_…`.
    #[must_use]
    pub fn new(signing_secret: Secret) -> Self {
        Self {
            secret: signing_secret,
            tolerance: DEFAULT_TOLERANCE,
        }
    }

    /// Changes how far a delivery's timestamp may be from now.
    ///
    /// [`DEFAULT_TOLERANCE`] is what Stripe recommends. Raising it is a
    /// decision about how long a captured body stays replayable.
    #[must_use]
    pub fn tolerance(mut self, tolerance: Duration) -> Self {
        self.tolerance = tolerance;
        self
    }

    /// The signature Stripe should have sent for these bytes at this time.
    fn signature(&self, timestamp: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.expose().as_bytes())
            .unwrap_or_else(|_| unreachable!("HMAC accepts a key of any length"));
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(body);
        mac.finalize()
            .into_bytes()
            .iter()
            .fold(String::with_capacity(64), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            })
    }
}

/// What Stripe's signature header carries: one timestamp and one or more
/// signatures under the `v1` scheme.
///
/// More than one `v1` is normal rather than exceptional — an endpoint whose
/// secret is being rotated is signed with both, so a verifier that reads only
/// the first refuses half the deliveries for as long as the rollover lasts.
struct SignatureHeader<'a> {
    timestamp: &'a str,
    signatures: Vec<&'a str>,
}

impl<'a> SignatureHeader<'a> {
    fn parse(header: &'a str) -> Option<Self> {
        let mut timestamp = None;
        let mut signatures = Vec::new();
        for part in header.split(',') {
            match part.trim().split_once('=') {
                Some(("t", value)) => timestamp = Some(value),
                Some(("v1", value)) => signatures.push(value),
                // `v0` is Stripe's test-mode scheme for Connect, and anything
                // else is one they have added since. Neither is a v1 signature.
                _ => {}
            }
        }
        let timestamp = timestamp?;
        (!signatures.is_empty()).then_some(Self {
            timestamp,
            signatures,
        })
    }
}

#[async_trait]
impl Webhook for Webhooks {
    fn provider(&self) -> ProviderId {
        PROVIDER
    }

    /// Checks the signature, then the age, and only then reads the body.
    ///
    /// The order is the point. Nothing is parsed out of a delivery that has
    /// not been shown to be Stripe's, and a delivery whose timestamp is
    /// outside [`Webhooks::tolerance`] is refused even though its signature
    /// matched — that is what a replay looks like.
    async fn verify(&self, delivery: &Delivery<'_>) -> Result<Event, Error> {
        let header = delivery
            .signed_header(SIGNATURE_HEADER)
            .map_err(|e| Error::new(ErrorKind::Untrusted, PROVIDER, e.to_string()).with_source(e))?
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Untrusted,
                    PROVIDER,
                    "the delivery carried no Stripe-Signature header",
                )
            })?;
        let parsed = SignatureHeader::parse(header).ok_or_else(|| {
            Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the Stripe-Signature header carried no timestamp and v1 signature",
            )
        })?;

        let expected = self.signature(parsed.timestamp, delivery.body());
        let matched = parsed
            .signatures
            .iter()
            // Constant time: a comparison that returns early says how much of
            // a guess was right.
            .any(|sent| expected.as_bytes().ct_eq(sent.as_bytes()).unwrap_u8() == 1);
        if !matched {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the delivery is not signed the way Stripe signs one",
            ));
        }

        self.check_age(parsed.timestamp)?;

        let body = delivery.body_str().ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a signed delivery whose body is not UTF-8",
            )
        })?;
        read_event(body)
    }
}

impl Webhooks {
    /// Refuses a delivery older or newer than the tolerance allows.
    ///
    /// A clock this far out is worth refusing in both directions: a delivery
    /// stamped in the future is one whose sender's clock nobody can vouch for.
    fn check_age(&self, timestamp: &str) -> Result<(), Error> {
        let sent = timestamp.parse::<u64>().map_err(|e| {
            Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the Stripe-Signature timestamp is not a number of seconds",
            )
            .with_source(e)
        })?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                Error::new(
                    ErrorKind::Untrusted,
                    PROVIDER,
                    "this machine's clock is before 1970, so no delivery can be dated",
                )
                .with_source(e)
            })?
            .as_secs();
        let apart = now.abs_diff(sent);
        if apart > self.tolerance.as_secs() {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                format!(
                    "the delivery is signed {apart} seconds from now, outside the {} \
                     the tolerance allows",
                    self.tolerance.as_secs()
                ),
            ));
        }
        Ok(())
    }
}

/// What a verified body says, in the shared words.
fn read_event(body: &str) -> Result<Event, Error> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "a signed delivery whose body is not the JSON Stripe documents",
        )
        .with_source(e)
    })?;
    let id = value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a delivery carried no event id, and there is nothing to deduplicate it by",
            )
        })?;
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a delivery carried no event type",
            )
        })?;
    let object = value.pointer("/data/object");

    let payment = match event_type.split_once('.') {
        // A PaymentIntent names itself; everything else that carries one names
        // it in `payment_intent` — a Charge, a Refund, a Dispute.
        Some(("payment_intent", _)) => object.and_then(|o| o.get("id")),
        _ => object.and_then(|o| o.get("payment_intent")),
    }
    .and_then(serde_json::Value::as_str)
    .map(PaymentId::issued);

    Ok(Event {
        id: EventId::issued(id),
        kind: event_kind(event_type),
        payment,
        amount: object.and_then(|object| event_amount(event_type, object)),
        provider: PROVIDER,
        raw: Raw::from_text(body),
    })
}

/// What one of Stripe's event types means here.
///
/// Only the PaymentIntent's own lifecycle and the one refund event that says
/// money went back are named. A `charge.*` event is deliberately
/// [`EventKind::Other`]: a Charge is the object underneath the PaymentIntent
/// this crate models, and answering `Captured` for both would have a caller
/// counting one payment twice.
///
/// Everything else — a dispute, a payout, a Radar warning, and whatever Stripe
/// adds next — is `Other` carrying Stripe's own name for it. That is not a
/// failure: refusing an event type is how a handler earns days of redeliveries.
fn event_kind(event_type: &str) -> EventKind {
    match event_type {
        "payment_intent.succeeded" => EventKind::Captured,
        "payment_intent.amount_capturable_updated" => EventKind::Authorized,
        "payment_intent.canceled" => EventKind::Canceled,
        "payment_intent.payment_failed" => EventKind::Failed,
        // Stripe fires this when a refund on a charge succeeds. `refund.*`
        // reports a refund's own life — created, updated, failed — and a
        // created refund is not money that has gone back.
        "charge.refunded" => EventKind::Refunded,
        other => EventKind::Other(other.into()),
    }
}

/// The figure the event is about, where the object carries one that means
/// what [`Event::amount`] says it means.
///
/// A refunded charge answers what has gone back rather than what was taken:
/// the event is about the refund, and `amount` there is still the original
/// payment.
fn event_amount(event_type: &str, object: &serde_json::Value) -> Option<Money> {
    let field = if event_type == "charge.refunded" {
        "amount_refunded"
    } else {
        "amount"
    };
    let minor_units = object.get(field)?.as_i64()?;
    let currency = object.get("currency")?.as_str()?.parse::<Currency>().ok()?;
    Some(Money::from_minor_units(minor_units, currency))
}

#[cfg(test)]
mod tests {
    use super::{SignatureHeader, event_kind};
    use kasapay_core::EventKind;

    #[test]
    fn a_header_being_rotated_carries_two_signatures_and_both_are_read() {
        let parsed = SignatureHeader::parse("t=1492774577,v1=aaa,v0=zzz,v1=bbb")
            .expect("a timestamp and two signatures");
        assert_eq!(parsed.timestamp, "1492774577");
        assert_eq!(parsed.signatures, ["aaa", "bbb"]);
    }

    #[test]
    fn a_header_with_no_v1_signature_is_no_header_at_all() {
        assert!(SignatureHeader::parse("t=1492774577,v0=zzz").is_none());
        assert!(SignatureHeader::parse("v1=aaa").is_none());
        assert!(SignatureHeader::parse("").is_none());
    }

    /// An event type nobody has heard of is a normal thing to receive.
    #[test]
    fn an_unknown_event_type_keeps_stripes_own_name_for_it() {
        assert_eq!(event_kind("payment_intent.succeeded"), EventKind::Captured);
        assert_eq!(event_kind("charge.refunded"), EventKind::Refunded);
        assert_eq!(
            event_kind("issuing_card.created"),
            EventKind::Other("issuing_card.created".into())
        );
        // A Charge is the object under the PaymentIntent, and counting both
        // would count one payment twice.
        assert_eq!(
            event_kind("charge.succeeded"),
            EventKind::Other("charge.succeeded".into())
        );
    }
}
