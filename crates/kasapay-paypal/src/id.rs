//! What PayPal names, where core has no kind for it.
//!
//! A capture, a refund and an authorization are each PayPal's own string —
//! `3C679366HH908993F`, `0K35355239430361V`, `0AW2184448108334S` — issued
//! against an order rather than naming one, the same split
//! `kasapay_mollie::id` draws for `cpt_…` and `re_…`. None of the three is a
//! [`PaymentId`](kasapay_core::PaymentId): handing a refund id to a call that
//! wants an order id does not compile.

use kasapay_core::Id;

/// What a PayPal identifier names, beyond an order.
pub mod kind {
    /// One capture taken off an order — `intent: CAPTURE`'s own, or one taken
    /// off an authorization.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Capture;

    impl kasapay_core::IdKind for Capture {
        const NAMES: &'static str = "paypal capture";
    }

    /// One hold placed on an `intent: AUTHORIZE` order.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Authorization;

    impl kasapay_core::IdKind for Authorization {
        const NAMES: &'static str = "paypal authorization";
    }
}

/// PayPal's identifier for one capture — `3C679366HH908993F`.
///
/// Not a [`PaymentId`](kasapay_core::PaymentId). A capture is taken off an
/// order rather than being one, and [`PayPal::refund`](crate::PayPal::refund)
/// is called against this, never against the order id: see the crate's own
/// documentation for why a PayPal refund has this shape at all.
///
/// PayPal's own answer to opening or capturing an order carries this nested
/// inside `purchase_units[0].payments.captures[0].id` rather than at the top
/// level the way Mollie's `paymentId` sits beside its capture — so there is
/// no field on [`Charge`](kasapay_core::Charge) to read it from directly.
/// [`capture_id`](crate::capture_id) reads it out of
/// [`Charge::raw`](kasapay_core::Charge::raw).
pub type CaptureId = Id<kind::Capture>;

/// PayPal's identifier for one refund — `0K35355239430361V`.
///
/// Core's own [`RefundId`](kasapay_core::RefundId), re-exported rather than a
/// kind of PayPal's: [`Provider::refund`](kasapay_core::Provider::refund)
/// answers a refund for every provider, so the concept is shared the way a
/// payment is. Still not a [`PaymentId`](kasapay_core::PaymentId) or a
/// [`CaptureId`], for the same reason neither of those is the other.
pub use kasapay_core::RefundId;

/// PayPal's identifier for one authorization — `0AW2184448108334S`.
///
/// Not a [`PaymentId`](kasapay_core::PaymentId). An order created with
/// `intent: AUTHORIZE` still names itself with one of those; this is the hold
/// [`PayPal::authorize_order`](crate::PayPal::authorize_order) places on it,
/// with its own id and — PayPal documents this on every authorize example —
/// its own `expiration_time`, which nothing in this crate reads back into a
/// typed field. It is on [`Charge::raw`](kasapay_core::Charge::raw), at
/// `/purchase_units/0/payments/authorizations/0/expiration_time`.
pub type AuthorizationId = Id<kind::Authorization>;
