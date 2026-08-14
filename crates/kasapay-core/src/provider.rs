//! The trait every payment provider implements.

use std::fmt;

use crate::charge::{Charge, ChargeRequest, PaymentId};
use crate::error::Error;
use crate::money::Money;
use crate::refund::{Refund, RefundRequest};
use crate::webhook::{Event, EventKind};

/// Names a provider.
///
/// A string rather than an enum so a provider living outside this workspace is
/// a first-class one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProviderId(&'static str);

impl ProviderId {
    /// Stripe.
    pub const STRIPE: Self = Self("stripe");
    /// iyzico.
    pub const IYZICO: Self = Self("iyzico");

    /// Names a provider this workspace does not ship.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    /// The name as text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// What a provider will do, asked before there is a payment to ask it about.
///
/// This and [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) answer
/// different questions and both have to exist. This one is for planning: a
/// checkout deciding whether to offer authorise-now-capture-later needs the
/// answer before it has a payment. `Unsupported` is for enforcement, and stays
/// the thing that actually refuses the call.
///
/// **A capability that says yes and a call that then fails is a bug in the
/// adapter**, and so is the reverse. An adapter's tests are where that is
/// held to.
///
/// Every field is public and the struct is open, for the same reason
/// [`Charge`] is: an adapter in someone else's repository has to be able to
/// build one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is an independent yes or no about one provider; a state machine would invent an order between them that does not exist"
)]
pub struct Capabilities {
    /// Funds can be held at authorisation and taken later by
    /// [`Provider::capture`].
    ///
    /// False says the provider takes the money at authorisation and has no
    /// capture step — not that capture failed. Distinguishing those two is the
    /// whole reason this type exists.
    pub separate_capture: bool,
    /// [`Provider::capture`] accepts an amount below the one authorised.
    ///
    /// Only meaningful where `separate_capture` is true.
    pub partial_capture: bool,
    /// A payment can be refunded for less than it was captured for.
    pub partial_refund: bool,
    /// A payment can be refunded more than once, up to what was captured.
    pub repeated_refund: bool,
}

/// Marks an implementation of [`Provider`] so its `async fn`s compile.
///
/// Re-exported because the version has to match the one this trait was defined
/// with, and matching it by hand is a footgun for anyone writing a provider
/// outside this workspace.
pub use async_trait::async_trait;

/// Takes a payment and reports on it.
///
/// Implementations are cheap to clone and safe to share: hold one per process,
/// not one per request.
#[async_trait]
pub trait Provider: fmt::Debug + Send + Sync {
    /// Which provider this is.
    fn id(&self) -> ProviderId;

    /// Starts a charge.
    ///
    /// A returned [`Charge`] is not a completed payment. Read its
    /// [`status`](Charge::status) and its
    /// [`next_action`](Charge::next_action): a provider that redirects the
    /// payer answers [`Status::RequiresAction`](crate::Status::RequiresAction)
    /// here, and the payment is only decided once they come back.
    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error>;

    /// Reads a charge back.
    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error>;

    /// Takes funds an authorisation is only holding.
    ///
    /// A shop authorises when the order is placed and captures when the parcel
    /// leaves. `amount` of `None` takes the lot; `Some` takes part of it, which
    /// is what a partial shipment needs, and requires
    /// [`Capabilities::partial_capture`].
    ///
    /// The returned [`Charge`] carries the amount that was captured, not the
    /// amount that was authorised.
    ///
    /// Capture has no inverse. Captured money is refunded, not un-captured.
    ///
    /// A provider whose [`Capabilities::separate_capture`] is false took the
    /// money at authorisation and answers
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) here.
    async fn capture(&self, id: &PaymentId, amount: Option<Money>) -> Result<Charge, Error>;

    /// Releases an authorisation that will never be taken.
    ///
    /// Cancelling a payment whose funds are already captured is
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest) rather
    /// than a silent success: giving that money back is a refund, a different
    /// act with a different entry in the ledger.
    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error>;

    /// Gives money back off a payment.
    ///
    /// This is the only way money goes back, and it is not the inverse of
    /// [`Provider::capture`]: captured money is refunded, not un-captured, and
    /// the two are different entries in a ledger.
    ///
    /// [`RefundRequest::amount`] of `None` gives back all of it; `Some` gives
    /// back part, which requires [`Capabilities::partial_refund`].
    ///
    /// **A capture refunded three times is ordinary** — three returned items
    /// out of an order of five — so the returned [`Refund`] carries its own
    /// [`RefundId`](crate::RefundId) rather than the payment's, and each of
    /// those can be replayed under its own
    /// [`idempotency_key`](RefundRequest::idempotency_key). Whether a provider
    /// allows the second one at all is
    /// [`Capabilities::repeated_refund`].
    ///
    /// A refund is not instant. Read [`Refund::status`]: a provider that makes
    /// the payer approve it answers
    /// [`RefundStatus::RequiresAction`](crate::RefundStatus::RequiresAction)
    /// with a [`NextAction`](crate::NextAction), the same shape
    /// [`Provider::charge`] uses.
    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error>;

    /// Turns a delivery the provider made into an [`Event`], or refuses it.
    ///
    /// This is the only way to get an `Event`, and it checks the signature
    /// before it reads anything else. A body that cannot be shown to have come
    /// from the provider is
    /// [`ErrorKind::Untrusted`](crate::ErrorKind::Untrusted) — including one
    /// carrying no signature at all, which must never fall through as if the
    /// header were optional. A forged delivery is how a shop ships against a
    /// payment nobody made.
    ///
    /// Comparison is constant time, and a provider that timestamps its
    /// signature has that checked too: a body replayed a day later is refused
    /// even though its signature is genuine.
    ///
    /// # An unmodelled event type is not a failure
    ///
    /// A delivery whose type this crate does not model comes back as
    /// [`EventKind::Other`] carrying the provider's own word for it — never
    /// `Err`. Providers retry a delivery until it is accepted, so answering an
    /// error for an event type that was added last week puts a working
    /// endpoint into a redelivery loop that lasts days.
    ///
    /// # Headers
    ///
    /// `headers` is every header of the delivery, in any order, spelled
    /// however the caller's HTTP server spells them —
    /// [`header`](crate::header) matches them without regard to case. A slice
    /// of pairs rather than a `HeaderMap` because `kasapay-core` holds no HTTP
    /// client and will not take a dependency on one framework's type.
    ///
    /// `body` is the bytes exactly as they arrived. A signature covers those
    /// bytes, so a body that has been through a JSON parse and back verifies
    /// against nothing.
    fn verify_webhook(&self, headers: &[(String, String)], body: &[u8]) -> Result<Event, Error>;

    /// What this provider will do, before there is a payment to ask about.
    fn capabilities(&self) -> Capabilities;
}
