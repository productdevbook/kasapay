//! How a provider names one thing, and the two questions asked of every one.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Whose uniqueness an identifier rests on.
///
/// One question, asked of every identifier kasapay hands back: did the provider
/// give us this, or did we make it up? A caller writing an identifier into a
/// unique index — so a second webhook delivery collides instead of shipping
/// twice — is relying on somebody's guarantee, and the two answers are worth
/// very different things.
///
/// Exhaustive on purpose. There is no third answer, and an adapter that adds
/// one has invented a guarantee nobody made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdSource {
    /// The provider issued it, and it is unique because they say so.
    Provider,
    /// kasapay composed it out of the named fields, because the provider issues
    /// none of its own. It is unique exactly as far as those fields are.
    Derived(&'static [&'static str]),
}

/// What an identifier names — the other question, answered in the type.
///
/// A kind carries no data. It exists so that a payment id and a hosted form's
/// token, both of them the provider's own and both of them a string, are not
/// the same type and cannot be handed to each other's calls. An adapter names
/// its own by implementing this on a unit struct of its own and writing a type
/// alias over [`Id`], the way [`PaymentId`] is one over [`kind::Payment`].
pub trait IdKind {
    /// What this kind names, in words, for [`Debug`](fmt::Debug).
    const NAMES: &'static str;
}

/// What an identifier can name.
///
/// A provider adapter adds its own where the concept is that provider's: a
/// hosted checkout form belongs to iyzico's classic API rather than here.
pub mod kind {
    /// A payment at the provider.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Payment;

    impl super::IdKind for Payment {
        const NAMES: &'static str = "payment";
    }
}

/// How a provider names one thing of kind `K`.
///
/// Opaque on purpose: Stripe issues `pi_…`, iyzico a 64-bit integer, PayTR
/// nothing at all, and nothing outside the adapter should read any of them.
/// [`Display`](fmt::Display) writes the text alone, because that is what goes
/// into a request.
///
/// Two facts travel with the text. `K` is what the identifier names, and it is
/// checked by the compiler: a [`PaymentId`] cannot be passed where the token of
/// a hosted checkout form is wanted, however alike the two strings look.
/// [`Id::source`] is whose uniqueness it rests on — the provider's, or
/// kasapay's own composition of fields the caller sent.
///
/// ```compile_fail
/// use kasapay_core::{Id, IdKind, PaymentId};
///
/// struct Session;
/// impl IdKind for Session {
///     const NAMES: &'static str = "session";
/// }
///
/// fn refund(payment: &PaymentId) {
///     println!("{payment}");
/// }
///
/// // A session is not a payment, and this does not compile.
/// refund(&Id::<Session>::issued("tok-1"));
/// ```
pub struct Id<K: IdKind> {
    key: Box<str>,
    source: IdSource,
    kind: PhantomData<K>,
}

impl<K: IdKind> Id<K> {
    /// Wraps an identifier the provider issued.
    pub fn issued(value: impl Into<Box<str>>) -> Self {
        Self {
            key: value.into(),
            source: IdSource::Provider,
            kind: PhantomData,
        }
    }

    /// Wraps an identifier kasapay composed, naming the fields it came from.
    ///
    /// For a provider that issues none of its own: PayTR names a payment by
    /// the `merchant_oid` the merchant chose and sent. `from` is what the
    /// value's uniqueness actually rests on, and a caller reads it back
    /// through [`Id::source`].
    pub fn derived(value: impl Into<Box<str>>, from: &'static [&'static str]) -> Self {
        Self {
            key: value.into(),
            source: IdSource::Derived(from),
            kind: PhantomData,
        }
    }

    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.key
    }

    /// Whose uniqueness this identifier rests on.
    #[must_use]
    pub const fn source(&self) -> IdSource {
        self.source
    }
}

/// How the provider names a payment.
///
/// [`Charge::id`](crate::Charge::id) carries one, and
/// [`Provider::charge_status`](crate::Provider::charge_status) takes one. A
/// handle to something that is not a payment — the token of a checkout form the
/// payer has not finished — is a different kind and will not fit here.
pub type PaymentId = Id<kind::Payment>;

// Written out rather than derived: a derive would demand the same trait of `K`.
impl<K: IdKind> fmt::Debug for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Id")
            .field("names", &K::NAMES)
            .field("key", &self.key)
            .field("source", &self.source)
            .finish()
    }
}

impl<K: IdKind> fmt::Display for Id<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key)
    }
}

impl<K: IdKind> Clone for Id<K> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            source: self.source,
            kind: PhantomData,
        }
    }
}

impl<K: IdKind> PartialEq for Id<K> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.source == other.source
    }
}

impl<K: IdKind> Eq for Id<K> {}

impl<K: IdKind> Hash for Id<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.source.hash(state);
    }
}

impl<K: IdKind> PartialOrd for Id<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K: IdKind> Ord for Id<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key
            .cmp(&other.key)
            .then_with(|| self.source.cmp(&other.source))
    }
}

#[cfg(test)]
mod tests {
    use super::PaymentId;

    #[test]
    fn an_identifier_we_composed_is_not_one_the_provider_issued() {
        let issued = PaymentId::issued("ord-1");
        let composed = PaymentId::derived("ord-1", &["merchant_oid"]);
        assert_eq!(issued.as_str(), composed.as_str());
        assert_ne!(issued, composed);
        assert_ne!(issued.source(), composed.source());
    }

    #[test]
    fn debug_says_what_the_identifier_names() {
        let shown = format!("{:?}", PaymentId::issued("pi_1"));
        assert!(shown.contains("payment"), "{shown}");
        assert!(shown.contains("pi_1"), "{shown}");
    }
}
