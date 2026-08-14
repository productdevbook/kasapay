//! What a provider tells us out of band, once it has been shown to be theirs.

use std::fmt;

use crate::charge::PaymentId;
use crate::money::Money;
use crate::provider::ProviderId;
use crate::raw::Raw;

/// What identifies one delivery, and where that identity came from.
///
/// A caller writes this into a unique index so that a second delivery of the
/// same event collides instead of being acted on twice. Whether that index is
/// a correctness guarantee or a heuristic depends entirely on who composed the
/// value, and returning both in one `String` would hide the difference at
/// exactly the point it matters.
///
/// ```
/// # use kasapay_core::EventId;
/// # fn store(_key: &str) {}
/// # let event_id = EventId::Provider("evt_1".into());
/// // A caller that does not care reads the text.
/// store(event_id.as_str());
///
/// // A caller building a ledger asks where it came from.
/// if let EventId::Derived { from, .. } = &event_id {
///     eprintln!("uniqueness rests on {from:?}, and nothing else");
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventId {
    /// The provider's own id. Unique by their guarantee.
    Provider(Box<str>),
    /// Composed by kasapay because the provider sends none.
    ///
    /// Unique only as far as the parts it is made of are — and for at least
    /// one provider here they are not enough. Read the adapter's own
    /// documentation before treating this as a key.
    Derived {
        /// The composed value.
        key: Box<str>,
        /// Which fields of the delivery it was built out of, so a caller can
        /// read at runtime what their uniqueness actually rests on.
        from: &'static [&'static str],
    },
}

impl EventId {
    /// The identifier as text, however it was arrived at.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Provider(id) | Self::Derived { key: id, .. } => id,
        }
    }

    /// Whether kasapay composed this rather than the provider issuing it.
    #[must_use]
    pub const fn is_derived(&self) -> bool {
        matches!(self, Self::Derived { .. })
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a delivery says happened.
///
/// # An unmodelled type is [`EventKind::Other`], never an error
///
/// A provider adds event types without asking, and an adapter that answered
/// `Err` for one it had not heard of would turn a handler into a rejection
/// every provider retries — for days, against an endpoint that is working. So
/// anything not named here arrives as `Other` carrying the provider's own word
/// for it, with the whole body still on [`Event::raw`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EventKind {
    /// Funds are held and not yet taken.
    Authorized,
    /// Funds are taken.
    Captured,
    /// Money went back, in whole or in part.
    Refunded,
    /// The payment was refused.
    Failed,
    /// The payment was withdrawn before it completed.
    Canceled,
    /// Something this crate does not model, in the provider's own words.
    Other(Box<str>),
}

/// A delivery that has been shown to have come from the provider.
///
/// **There is no way to build one of these from an unverified body**, and that
/// is the point: [`Provider::verify_webhook`](crate::Provider::verify_webhook)
/// is the only thing in this crate that produces one, and it checks the
/// signature before it reads anything else.
#[derive(Debug, Clone)]
pub struct Event {
    /// What identifies this delivery, and whether the provider issued it.
    pub id: EventId,
    /// What it says happened.
    pub kind: EventKind,
    /// The payment it is about, where the delivery names one.
    pub payment: Option<PaymentId>,
    /// The amount it is about, where the delivery carries one in a currency
    /// kasapay knows.
    ///
    /// `None` is not zero: a provider that settles in a currency
    /// [`Currency`](crate::Currency) has no variant for still sends a valid
    /// event, and dropping the amount is better than refusing the delivery.
    pub amount: Option<Money>,
    /// Which provider sent it.
    pub provider: ProviderId,
    /// The body exactly as it arrived, signature included.
    pub raw: Raw,
}

/// One header of a delivery, matched without regard to case.
///
/// HTTP header names are case-insensitive and every framework spells them
/// differently, so an adapter that compared them directly would work behind
/// one server and fail behind another.
#[must_use]
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::{EventId, EventKind, header};

    #[test]
    fn a_derived_id_says_what_its_uniqueness_rests_on() {
        let id = EventId::Derived {
            key: "ord-1:success".into(),
            from: &["merchant_oid", "status"],
        };
        assert_eq!(id.as_str(), "ord-1:success");
        assert!(id.is_derived());
        let EventId::Derived { from, .. } = &id else {
            unreachable!("built as derived")
        };
        assert_eq!(*from, ["merchant_oid", "status"]);
    }

    #[test]
    fn a_providers_own_id_says_it_is_theirs() {
        let id = EventId::Provider("evt_1".into());
        assert_eq!(id.as_str(), "evt_1");
        assert!(!id.is_derived());
        assert_eq!(id.to_string(), "evt_1");
    }

    #[test]
    fn two_deliveries_the_provider_numbered_are_distinguishable() {
        // The reason a caller writes this into a unique index.
        assert_ne!(
            EventId::Provider("evt_1".into()),
            EventId::Provider("evt_2".into())
        );
    }

    #[test]
    fn an_unmodelled_kind_keeps_the_providers_own_word() {
        let kind = EventKind::Other("payment_intent.partially_funded".into());
        assert_eq!(
            kind,
            EventKind::Other("payment_intent.partially_funded".into())
        );
    }

    #[test]
    fn a_header_is_found_however_it_is_spelled() {
        let headers = vec![
            ("Content-Type".to_owned(), "application/json".to_owned()),
            ("STRIPE-SIGNATURE".to_owned(), "t=1,v1=abc".to_owned()),
        ];
        assert_eq!(header(&headers, "stripe-signature"), Some("t=1,v1=abc"));
        assert_eq!(header(&headers, "Stripe-Signature"), Some("t=1,v1=abc"));
        assert_eq!(header(&headers, "x-missing"), None);
    }
}
