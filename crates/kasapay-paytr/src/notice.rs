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

use kasapay_core::{Charge, Currency, Error, ErrorKind, Money, OrderRef, Raw, Status};
use serde::Deserialize;

use crate::client::{PAYTR, payment_id};
use crate::signing::Credentials;

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
    fn raw(&self) -> Raw {
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
