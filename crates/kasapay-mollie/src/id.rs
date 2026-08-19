//! What Mollie names, where core has no kind for it.
//!
//! Mollie issues three identifiers against one payment and they are all its
//! own strings: `tr_…` for the payment, `cpt_…` for a capture taken off it,
//! `re_…` for a refund. [`IdSource`](kasapay_core::IdSource) cannot separate
//! them — the provider issued all three — so the kind does, and the compiler
//! holds it. Two of the three are Mollie's own kinds; the refund is core's,
//! re-exported here, because core answers one now.
//!
//! A fourth is `cst_…`, the customer a mandate is hung on. `mdt_…`, the
//! mandate itself, is not here: it is core's own
//! [`InstrumentId`](kasapay_core::InstrumentId), the same kind Stripe's
//! `pm_…` and iyzico's `cardToken` are, because that is what it is — the
//! saved instrument this adapter charges.

use kasapay_core::Id;

/// What a Mollie identifier names.
pub mod kind {
    /// One capture taken off an authorised payment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Capture;

    impl kasapay_core::IdKind for Capture {
        const NAMES: &'static str = "mollie capture";
    }

    /// One customer, existing so a mandate has somewhere to be hung.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Customer;

    impl kasapay_core::IdKind for Customer {
        const NAMES: &'static str = "mollie customer";
    }
}

/// Mollie's identifier for one capture — `cpt_…`.
///
/// Not a [`PaymentId`](kasapay_core::PaymentId). A capture is a slice of a
/// payment rather than the payment, and handing one to a call that reads a
/// payment does not compile.
pub type CaptureId = Id<kind::Capture>;

/// Mollie's identifier for one refund — `re_…`.
///
/// Core's own [`RefundId`](kasapay_core::RefundId), re-exported rather than a
/// kind of Mollie's: [`Provider::refund`](kasapay_core::Provider::refund)
/// answers a refund for every provider, so the concept is shared the way a
/// payment is. Still not a [`PaymentId`](kasapay_core::PaymentId), for the
/// same reason a [`CaptureId`] is not.
pub use kasapay_core::RefundId;

/// Mollie's identifier for one customer — `cst_…`.
///
/// A mandate is named by two things: [`InstrumentId`](kasapay_core::InstrumentId)
/// for the `mdt_…` itself, and this for whose customer it belongs to —
/// [`ChargeRequest::customer`](kasapay_core::ChargeRequest::customer) carries
/// the same string when charging one. Not a [`PaymentId`](kasapay_core::PaymentId)
/// or an [`InstrumentId`](kasapay_core::InstrumentId): a customer is neither.
pub type CustomerId = Id<kind::Customer>;
