//! One saved instrument a provider holds, as
//! [`Provider::instruments`](crate::Provider::instruments) lists it.

use crate::id::InstrumentId;
use crate::raw::Raw;

/// One instrument a provider holds against a customer.
///
/// # Not always a card
///
/// Mollie's saved instrument is a mandate against a bank account, not a card
/// at all, so nothing here assumes one — no brand, no expiry, no last four.
/// What every provider can answer is an identity, something to show a payer
/// choosing between saved instruments, and its own untouched response; that
/// is what this carries. The richer, provider-specific type — iyzico's
/// `classic::StoredCard`, Stripe's `saved::StoredCard`, Mollie's `Mandate` —
/// stays on each adapter's own listing method, where it already was, for
/// whatever more than that a caller working against one provider wants.
///
/// Every field is public and the struct is open, for the same reason
/// [`Charge`](crate::Charge) is: an adapter in someone else's repository has
/// to be able to build one.
#[derive(Debug, Clone)]
pub struct Instrument {
    /// What a payment names this instrument by.
    ///
    /// Half a name at iyzico and PayTR, exactly as [`InstrumentId`]'s own
    /// documentation says: the `customer` that
    /// [`Provider::instruments`](crate::Provider::instruments) was asked with
    /// is the other half, and an adapter's own charging call takes both
    /// because [`Provider::charge`](crate::Provider::charge) cannot.
    pub id: InstrumentId,
    /// Something to show a person choosing between saved instruments — "Visa
    /// ending 4242", the alias a cardholder gave a card, a mandate's payment
    /// method.
    ///
    /// `None` where the provider answered nothing usable for it, rather than
    /// a label assembled out of parts that might be missing.
    pub label: Option<Box<str>>,
    /// The provider's own answer for this instrument, untouched.
    pub raw: Raw,
}
