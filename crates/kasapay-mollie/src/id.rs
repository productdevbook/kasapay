//! What Mollie names, where core has no kind for it.
//!
//! Mollie issues three identifiers against one payment and they are all its
//! own strings: `tr_…` for the payment, `cpt_…` for a capture taken off it,
//! `re_…` for a refund. [`IdSource`](kasapay_core::IdSource) cannot separate
//! them — the provider issued all three — so the kind does, and the compiler
//! holds it.

use kasapay_core::Id;

/// What a Mollie identifier names.
pub mod kind {
    /// One capture taken off an authorised payment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Capture;

    impl kasapay_core::IdKind for Capture {
        const NAMES: &'static str = "mollie capture";
    }

    /// One refund taken off a payment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct Refund;

    impl kasapay_core::IdKind for Refund {
        const NAMES: &'static str = "mollie refund";
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
/// Not a [`PaymentId`](kasapay_core::PaymentId), for the same reason a
/// [`CaptureId`] is not.
pub type RefundId = Id<kind::Refund>;
