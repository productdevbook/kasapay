//! The payment notice PayTR posts when a payment ends.
//!
//! PayTR does not wait for the payer to come back. It posts the outcome to the
//! merchant's notification address and retries until the body of the reply is
//! exactly `OK`. This is the only place a refusal is reported — the status
//! query answers a payment that succeeded, or an error — so
//! [`Notice::charge`] is where a refused payment becomes a
//! [`Charge`] with [`Status::Failed`].
//!
//! **A notice that does not verify must still be answered `OK`** and then
//! ignored. Anything else makes PayTR retry it for days, and acting on it is
//! how a shop ships against a payment nobody made.
//!
//! # Three fields are signed and the rest are not
//!
//! PayTR's hash covers `merchant_oid`, `status` and `total_amount`. Everything
//! else on the notice arrived unauthenticated, which is why [`Notice::charge`]
//! takes the currency from the caller rather than reading it off
//! [`Notice::currency`].
//!
//! # Amounts here are in minor units
//!
//! `total_amount` and `payment_amount` arrive multiplied by a hundred — 34.56
//! as `3456` — while [`PayTr::refund`](crate::PayTr::refund) takes a decimal
//! string. Two formats in one API, and [`Notice::charge`] is what keeps them
//! apart.

use kasapay_core::{
    Charge, Currency, Delivery, Error, ErrorKind, Event, EventId, EventKind, Money, OrderRef,
    ProviderId, Raw, Status, Webhook, async_trait,
};
use serde::Deserialize;

use crate::client::{PAYTR, PayTr, payment_id};
use crate::signing::Credentials;

/// What a composed event identifier is composed of.
///
/// PayTR issues nothing that names a delivery, so the two fields it signs that
/// say which payment and what became of it are what one is built from. A
/// payment has one outcome, so a retry of the same notice composes the same
/// key — which is what a caller's unique index needs.
const EVENT_NAMED_BY: &[&str] = &["merchant_oid", "status"];

/// A payment notice, as PayTR posts it.
///
/// Open on purpose: a web handler deserialises one off the form body, and a
/// test builds one.
#[derive(Debug, Clone, Deserialize)]
pub struct Notice {
    /// The merchant's own order reference, which is what PayTR names the
    /// payment by.
    pub merchant_oid: Box<str>,
    /// `success` or `failed`.
    pub status: Box<str>,
    /// What the payer was charged, in minor units.
    pub total_amount: Box<str>,
    /// PayTR's hash over the order reference, the status and the total.
    pub hash: Box<str>,
    /// Why a payment was refused, as a number from PayTR's own list.
    pub failed_reason_code: Option<Box<str>>,
    /// Why a payment was refused, in words meant for the payer.
    pub failed_reason_msg: Option<Box<str>>,
    /// What the order came to, in minor units, before any instalment surcharge.
    pub payment_amount: Option<Box<str>>,
    /// PayTR's own spelling of the currency: `TL`, `USD`, `EUR`, `GBP` or `RUB`.
    ///
    /// Outside the hash, so it says what PayTR meant and proves nothing.
    pub currency: Option<Box<str>>,
    /// `card` or `eft`.
    pub payment_type: Option<Box<str>>,
    /// `1` for a payment made in test mode.
    pub test_mode: Option<Box<str>>,
}

impl Notice {
    /// What the notice says happened, once its hash checks out.
    ///
    /// `currency` is the one the payment was opened in. PayTR signs the order
    /// reference, the status and the total and nothing else, so
    /// [`Notice::currency`] is not evidence of anything and this does not read
    /// it.
    ///
    /// A refused payment is [`Status::Failed`] carrying the amount that was
    /// attempted — including the payer who ran out of time or closed the page,
    /// which PayTR reports as a refusal like any other and names in
    /// [`Notice::failed_reason_code`].
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Untrusted`] for a notice that is not signed the way PayTR
    /// signs one, and [`ErrorKind::Malformed`] for a status or an amount it
    /// does not document. **Answer `OK` anyway**: the reply body is what stops
    /// PayTR retrying, not what it says.
    pub fn charge(&self, credentials: &Credentials, currency: Currency) -> Result<Charge, Error> {
        if !credentials.verify_callback(
            &self.hash,
            &self.merchant_oid,
            &self.status,
            &self.total_amount,
        ) {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PAYTR,
                "the notice is not signed the way PayTR signs one",
            ));
        }

        let status = match &*self.status {
            "success" => Status::Captured,
            "failed" => Status::Failed,
            other => {
                return Err(Error::new(
                    ErrorKind::Malformed,
                    PAYTR,
                    format!("a notice carried the status {other}, which PayTR does not document"),
                ));
            }
        };

        let amount = minor_units(&self.total_amount, currency)?;
        let order_amount = self
            .payment_amount
            .as_deref()
            .map(|value| minor_units(value, currency))
            .transpose()?
            .filter(|order| *order != amount);
        let order = OrderRef::new(&*self.merchant_oid);

        Ok(Charge {
            id: Some(payment_id(&order)),
            order: Some(order),
            amount,
            order_amount,
            status,
            next_action: None,
            provider: PAYTR,
            raw: self.raw(),
        })
    }

    /// The notice as a body, for everything [`Charge`] does not model.
    ///
    /// The hash is left out: it is the signature over the rest, not something
    /// the payment is described by.
    pub(crate) fn raw(&self) -> Raw {
        Raw::from_json(&serde_json::json!({
            "merchant_oid": &*self.merchant_oid,
            "status": &*self.status,
            "total_amount": &*self.total_amount,
            "failed_reason_code": self.failed_reason_code.as_deref(),
            "failed_reason_msg": self.failed_reason_msg.as_deref(),
            "payment_amount": self.payment_amount.as_deref(),
            "currency": self.currency.as_deref(),
            "payment_type": self.payment_type.as_deref(),
            "test_mode": self.test_mode.as_deref(),
        }))
    }
}

/// Reads one of the notice's amounts, which are multiplied by a hundred.
fn minor_units(value: &str, currency: Currency) -> Result<Money, Error> {
    value
        .parse::<i64>()
        .map(|units| Money::from_minor_units(units, currency))
        .map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYTR,
                format!("a notice carried {value} where an amount in minor units belongs"),
            )
            .with_source(e)
        })
}

/// PayTR's notice is the only thing it posts, and [`PayTr`] already holds what
/// signs it.
///
/// No second type and no second credential: PayTR's notice hash is keyed with
/// the same merchant key and salt every request carries, unlike Stripe's
/// endpoint secret. A caller that has a client has a verifier.
#[async_trait]
impl Webhook for PayTr {
    fn provider(&self) -> ProviderId {
        PAYTR
    }

    /// Reads a notice, checks its hash, and says what it means.
    ///
    /// # The amount is left out on purpose
    ///
    /// [`Event::amount`] is `None` for every PayTR notice. PayTR signs
    /// `merchant_oid`, `status` and `total_amount` and **not** the currency, so
    /// the figure that arrived is a number with no unit anybody can vouch for.
    /// It is on [`Event::raw`], and [`Notice::charge`] is the call that turns
    /// it into money — it takes the currency from the caller, who knows what
    /// the payment was opened in.
    ///
    /// # A status PayTR does not document is not a failure
    ///
    /// [`Notice::charge`] answers [`ErrorKind::Malformed`] for one, because a
    /// [`Charge`] has to say `Captured` or `Failed` and neither would be true.
    /// An [`Event`] has [`EventKind::Other`], which is the honest answer and
    /// the one that does not earn a week of redeliveries.
    ///
    /// **Answer `OK` whatever this returns.** PayTR retries any other reply
    /// for days; the `Err` says do not act, not what to say.
    async fn verify(&self, delivery: &Delivery<'_>) -> Result<Event, Error> {
        let body = delivery.body_str().ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYTR,
                "a notice whose body is not UTF-8",
            )
        })?;
        let notice: Notice = serde_urlencoded::from_str(body).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYTR,
                "a notice that is not the form PayTR posts",
            )
            .with_source(e)
        })?;

        if !self.credentials().verify_callback(
            &notice.hash,
            &notice.merchant_oid,
            &notice.status,
            &notice.total_amount,
        ) {
            return Err(Error::new(
                ErrorKind::Untrusted,
                PAYTR,
                "the notice is not signed the way PayTR signs one",
            ));
        }

        let kind = match &*notice.status {
            "success" => EventKind::Captured,
            "failed" => EventKind::Failed,
            other => EventKind::Other(other.into()),
        };
        let order = OrderRef::new(&*notice.merchant_oid);
        Ok(Event {
            id: EventId::derived(
                format!("{}:{}", notice.merchant_oid, notice.status),
                EVENT_NAMED_BY,
            ),
            kind,
            payment: Some(payment_id(&order)),
            amount: None,
            provider: PAYTR,
            raw: notice.raw(),
        })
    }
}
