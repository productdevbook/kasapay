//! The trait every payment provider implements.

use std::fmt;

use crate::charge::{Charge, ChargeRequest};
use crate::error::Error;
use crate::id::PaymentId;
use crate::instrument::Instrument;
use crate::money::Money;

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
    /// An instrument [`Provider::instruments`] lists can be charged, through a
    /// call of this adapter's own — with the payer entering nothing.
    ///
    /// What a checkout reads before it offers "use my saved card". This
    /// describes *charging*, not *listing*: every adapter answers
    /// [`Provider::instruments`] regardless of this flag, and the two do not
    /// have to agree. PayTR's hosted form does store a card — a vault exists —
    /// but nothing here can list it or charge it, so both answer
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported), for two
    /// different reasons that happen to give the same result: `false` here
    /// says specifically that this adapter has no call that charges one, which
    /// is the answer a checkout needs before it offers the button.
    ///
    /// The charging call itself is the adapter's own: it needs what that
    /// provider demands around a saved-instrument payment, which is not the
    /// same list twice at any two of them, and neither [`Provider::charge`]
    /// nor [`Provider::instruments`] carries any of it.
    pub saved_instruments: bool,
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
    ///
    /// `id` is a [`Charge::id`] this provider produced. A provider that names a
    /// payment by nothing at all — no identifier of its own and nothing to
    /// compose one from — answers
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) rather than
    /// accepting an identifier it cannot honour.
    ///
    /// A flow that is not yet a payment is not read here at all. iyzico's
    /// classic checkout form has only its own token until the payer finishes,
    /// and that token is a different [`IdKind`](crate::IdKind), so it has its
    /// own call rather than a signature this one cannot honestly take.
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

    /// Lists what a customer has saved with this provider.
    ///
    /// `customer` is the provider's own name for them — the same string
    /// [`ChargeRequest::customer`] carries, and, for iyzico's classic API,
    /// the `cardUserKey` that names the vault rather than a payer as such.
    ///
    /// This is the shape every provider can answer: an identity and something
    /// to show somebody choosing between them. It is not a card number and
    /// carries no field one could go in. What it is not, on purpose, is a way
    /// to charge one or to forget one — those stay each adapter's own call,
    /// because forgetting a card needs iyzico's `cardUserKey` *and* its token
    /// where Stripe's needs only the instrument, and charging one takes a
    /// buyer and a basket at iyzico, an `off_session` flag at Stripe, a
    /// `sequenceType` at Mollie — three requests this trait cannot honestly
    /// narrow to one signature. See [`Capabilities::saved_instruments`] for
    /// what that leaves this trait able to say about charging one.
    ///
    /// A provider with no vault at all — or one this crate has no working call
    /// against, which is PayTR's case: it does store a card, but nothing here
    /// signs a request against it — answers
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) rather than
    /// an empty list, because an empty list would read as "this customer has
    /// nothing saved" instead of "asking is not possible here".
    ///
    /// No default: a provider outside this workspace has to answer, the same
    /// as every other method here.
    async fn instruments(&self, customer: &str) -> Result<Vec<Instrument>, Error>;

    /// What this provider will do, before there is a payment to ask about.
    fn capabilities(&self) -> Capabilities;
}
