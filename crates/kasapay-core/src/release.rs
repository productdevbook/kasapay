//! A hold released, as its own act.

use crate::id::PaymentId;
use crate::money::Money;
use crate::provider::ProviderId;
use crate::raw::Raw;

/// What a provider said when it was asked to release a hold.
///
/// Not a [`Charge`](crate::Charge), for the reason a
/// [`Refund`](crate::Refund) is not a [`Status`](crate::Status): releasing a
/// hold is something that happens *to* a payment rather than a state the
/// payment arrives in. Three of the four providers here that hold funds answer
/// it with something that is not a payment at all — Mollie with `202 Accepted`
/// and no body, PayPal with `204` and no body, iyzico with a reversal carrying
/// the bank's own reference and no payment state.
///
/// Asking them for a `Charge` is what kept
/// [`Provider::cancel`](crate::Provider::cancel) from doing its one documented
/// job at three of the four, so this type is shaped around what they actually
/// send rather than around what would have been convenient.
#[derive(Debug, Clone)]
pub struct Release {
    /// The payment the hold was on, where the provider names it.
    ///
    /// `None` for an answer with no body, where the only identifier is the one
    /// the caller passed in — it is not echoed back here, because an id this
    /// crate made up is not the provider confirming anything.
    pub payment: Option<PaymentId>,
    /// What was released, where the provider says.
    ///
    /// `None` for the providers that answer nothing, which is most of them. A
    /// hold is released whole or not at all everywhere here, so the amount is
    /// a reconciliation detail rather than something a caller must branch on.
    pub amount: Option<Money>,
    /// Whether the money is released, or the provider has only taken the
    /// request.
    pub state: ReleaseState,
    /// Which provider this came from.
    pub provider: ProviderId,
    /// The provider's own answer, untouched.
    pub raw: Raw,
}

/// How far a release has got.
///
/// The distinction is real money: a hold that is [`ReleaseState::Accepted`]
/// may still be against the payer's limit, and a shop that tells them it is
/// gone will be arguing about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReleaseState {
    /// The provider says the hold is gone.
    Released,
    /// The provider took the request and the issuer decides if and when.
    ///
    /// Mollie is explicit about this: it answers `202 Accepted`, says it will
    /// try, and leaves the issuing bank to release the money. Read the payment
    /// back with [`Provider::charge_status`](crate::Provider::charge_status)
    /// to learn what became of it.
    Accepted,
}

impl ReleaseState {
    /// Whether the money may still be held.
    ///
    /// True for [`ReleaseState::Accepted`], which is the case a caller must
    /// not report to a payer as finished.
    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Accepted)
    }
}
