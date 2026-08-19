//! What a provider posts when a payment finishes without us asking.

use std::fmt;

use crate::error::Error;
use crate::id::{EventId, PaymentId};
use crate::money::Money;
use crate::provider::{ProviderId, async_trait};
use crate::raw::Raw;

/// One delivery from a provider, exactly as it arrived.
///
/// Headers and bytes, and nothing parsed: **a signature is over the bytes the
/// provider sent**, and a body that has been through a JSON parser and back is
/// a different sequence of bytes that will not verify. A web framework hands
/// the body over as `&[u8]` before anything else touches it, and that is what
/// belongs here.
///
/// Header names are matched without regard to case, because HTTP/2 lowercases
/// them and HTTP/1.1 does not.
#[derive(Debug, Clone, Copy)]
pub struct Delivery<'a> {
    headers: &'a [(&'a str, &'a str)],
    body: &'a [u8],
}

impl<'a> Delivery<'a> {
    /// Holds a delivery's headers and its body.
    #[must_use]
    pub const fn new(headers: &'a [(&'a str, &'a str)], body: &'a [u8]) -> Self {
        Self { headers, body }
    }

    /// The first header with this name, ignoring case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&'a str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    }

    /// Every header, in the order they arrived.
    #[must_use]
    pub const fn headers(&self) -> &'a [(&'a str, &'a str)] {
        self.headers
    }

    /// The body as it arrived.
    #[must_use]
    pub const fn body(&self) -> &'a [u8] {
        self.body
    }

    /// The body as text, for a provider that posts a form or JSON.
    ///
    /// `None` for a body that is not UTF-8. Nothing lossy: a body that is not
    /// what the provider documents is one to refuse rather than repair.
    #[must_use]
    pub fn body_str(&self) -> Option<&'a str> {
        std::str::from_utf8(self.body).ok()
    }
}

/// What a delivery says happened.
///
/// `Other` is not an error and must never become one. A provider adding an
/// event type is normal, and a handler that answers an error for one it does
/// not know drives the provider into a redelivery loop that can run for days
/// — for something nobody wanted in the first place.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventKind {
    /// Funds are held and not taken.
    Authorized,
    /// Funds are taken.
    Captured,
    /// Money has gone back.
    Refunded,
    /// The payment was refused, and will not proceed.
    Failed,
    /// The payment was withdrawn before it completed.
    Canceled,
    /// Something else, named as the provider named it.
    Other(Box<str>),
}

/// What a provider told us happened, once it has been shown to be theirs.
///
/// Every field is public and the struct is open, for the same reason
/// [`Charge`](crate::Charge) is: an adapter in someone else's repository has
/// to be able to build one.
///
/// # Why this is not an enum of [`Charge`](crate::Charge) and [`Refund`](crate::Refund)
///
/// It was the other candidate, and it asks each adapter to build a whole
/// charge out of a delivery. Stripe's webhook carries a PaymentIntent and can;
/// PayTR's notice signs three fields and cannot say what currency they are in;
/// Mollie's carries an identifier and nothing else at all. The shape that
/// survives all three is this one — what happened, to which payment, and the
/// body for everything else — and what the caller does next is read the
/// payment back, which is a call they already have.
#[derive(Debug, Clone)]
pub struct Event {
    /// What the caller writes into a unique index before acting on this.
    ///
    /// The second delivery of an event collides with the first instead of
    /// shipping the order twice, which is the whole reason this field is not
    /// optional — a delivery nothing identifies is one nothing can deduplicate.
    ///
    /// **Read [`Id::source`](crate::Id::source) before trusting it.** Stripe
    /// and PayPal issue an identifier for the delivery itself; PayTR and
    /// Mollie issue none, so kasapay composes one out of the fields the
    /// provider did send and says which they were. A composed key is unique
    /// exactly as far as those fields are, and no further — two deliveries
    /// that differ only in a field the provider did not sign are one key.
    pub id: EventId,
    /// What happened.
    pub kind: EventKind,
    /// The payment it happened to, where the delivery names one.
    ///
    /// `None` for a delivery about something that is not a payment — a dispute
    /// opening, a payout landing — which arrives as
    /// [`EventKind::Other`] and is a normal thing to receive.
    pub payment: Option<PaymentId>,
    /// The amount the delivery carried, where it carried one that can be
    /// trusted.
    ///
    /// `None` is not "zero" and not "the provider sent nothing". PayTR's
    /// notice carries an amount whose currency is **outside the hash**, so the
    /// figure is a number without a unit and is left here rather than guessed
    /// at; it is on [`Event::raw`] with everything else.
    pub amount: Option<Money>,
    /// Which provider sent it.
    pub provider: ProviderId,
    /// The delivery's own body, kept whole.
    pub raw: Raw,
}

/// Checks that a delivery is the provider's, and says what it means.
///
/// Separate from [`Provider`](crate::Provider) because it is a separate thing
/// to hold: verification needs a webhook secret the API credentials do not
/// carry, and a process that takes payments does not always handle the
/// callbacks for them.
///
/// # Verification is not one mechanism
///
/// `verify` is `async` because for two of the four providers that implement it
/// here, checking a delivery is a network call rather than a hash:
///
/// | | how a delivery is shown to be theirs |
/// |---|---|
/// | Stripe | HMAC-SHA256 over `timestamp.body`, with a tolerance window |
/// | PayTR | HMAC-SHA256 over three of the notice's fields |
/// | Mollie | **nothing is signed** — the delivery carries an identifier, and the payment is read back over the merchant's own authenticated connection |
/// | PayPal | PayPal verifies it, at `/v1/notifications/verify-webhook-signature` |
///
/// A trait that took only `(headers, body) -> Result<Event, Error>`
/// synchronously would fit the first two and force the other two to lie.
///
/// # Answering the provider is not this trait's business
///
/// An `Err` here says **do not act on this**. It does not say what to answer:
/// PayTR retries any reply that is not exactly `OK` for days, so a handler
/// that turns [`ErrorKind::Untrusted`](crate::ErrorKind::Untrusted) into a 500
/// has arranged for a forged notice to be delivered again every hour. Answer
/// the provider what the provider documents, and act only on `Ok`.
#[async_trait]
pub trait Webhook: fmt::Debug + Send + Sync {
    /// Which provider this verifies deliveries from.
    fn provider(&self) -> ProviderId;

    /// Shows that a delivery is the provider's, and reads what it says.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Untrusted`](crate::ErrorKind::Untrusted) for a delivery
    /// that cannot be shown to be theirs — a signature that does not match,
    /// one that is missing where the provider always sends one, or a timestamp
    /// outside the tolerance a replay would fall outside. Nothing has been
    /// read out of the body at that point, and nothing should be.
    ///
    /// [`ErrorKind::Malformed`](crate::ErrorKind::Malformed) for one that
    /// verifies and is then not the shape the provider documents. An event
    /// *type* this crate does not know is not that: it is
    /// [`EventKind::Other`], and it is `Ok`.
    async fn verify(&self, delivery: &Delivery<'_>) -> Result<Event, Error>;
}

#[cfg(test)]
mod tests {
    use super::Delivery;

    #[test]
    fn a_header_is_found_whatever_case_it_arrived_in() {
        let headers = [("Stripe-Signature", "t=1,v1=abc"), ("Accept", "*/*")];
        let delivery = Delivery::new(&headers, b"{}");
        assert_eq!(delivery.header("stripe-signature"), Some("t=1,v1=abc"));
        assert_eq!(delivery.header("STRIPE-SIGNATURE"), Some("t=1,v1=abc"));
        assert_eq!(delivery.header("x-nothing"), None);
    }

    #[test]
    fn a_body_that_is_not_utf8_is_kept_as_bytes_and_read_as_nothing() {
        let delivery = Delivery::new(&[], &[0xff, 0xfe]);
        assert_eq!(delivery.body(), &[0xff, 0xfe]);
        assert!(delivery.body_str().is_none());
    }
}
