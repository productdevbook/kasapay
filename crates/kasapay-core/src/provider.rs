//! The trait every payment provider implements.

use std::fmt;

use crate::charge::{Charge, ChargeRequest, IdempotencyKey};
use crate::error::Error;
use crate::id::PaymentId;
use crate::instrument::Instrument;
use crate::money::Money;
use crate::refund::{Refund, RefundRequest};

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
    ///
    /// `idempotency` makes a replayed capture safe where the provider offers
    /// it — read [`ErrorKind::is_retryable`](crate::ErrorKind::is_retryable)
    /// before retrying one without a key: unlike
    /// [`Provider::charge`](crate::Provider::charge), a repeated capture can
    /// take the same money twice, and not every provider protects against it.
    /// A provider that cannot honour a key ignores it rather than refusing the
    /// call, the same way iyzico's `in_store` refuses one on
    /// [`Provider::charge`](crate::Provider::charge) but a capture it does not
    /// implement has nothing to refuse it *for*.
    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error>;

    /// Releases an authorisation that will never be taken.
    ///
    /// Cancelling a payment whose funds are already captured is
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest) rather
    /// than a silent success: giving that money back is a refund, a different
    /// act with a different entry in the ledger.
    ///
    /// No idempotency key: repeating a cancel is harmless. The second call
    /// meets a hold that is already released and answers
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest) rather
    /// than releasing anything twice, which is the whole reason
    /// [`Provider::capture`] carries a key and this does not.
    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error>;

    /// Gives money back off a payment.
    ///
    /// Capture has no inverse — captured money is refunded, not un-captured —
    /// so this is the only way money goes back, and a
    /// [`Provider`](crate::Provider) offering
    /// [`capture`](Provider::capture) and not this is one a shop cannot use.
    ///
    /// Three refunds against one payment is ordinary: three returned items on
    /// one order. Whether this provider allows that is
    /// [`Capabilities::repeated_refund`], and whether it allows one for less
    /// than was captured is [`Capabilities::partial_refund`]; both are
    /// answerable before there is a payment to ask about.
    ///
    /// # `amount: None` is not one call everywhere
    ///
    /// `None` means all of it, and two providers have no request that says so
    /// — they take an amount and only an amount. What each adapter does:
    ///
    /// | | `amount: None` | its own idempotency |
    /// |---|---|---|
    /// | Stripe | refunds what is left, in one call | `Idempotency-Key` |
    /// | iyzico `classic` | [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest): send the amount | none — a key is refused |
    /// | iyzico `in_store` | refunds all of it, in one call | none — a key is refused |
    /// | PayTR | [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest): send the amount | none — a key is refused |
    /// | Mollie | reads the payment's `amountRemaining` first, so **two** calls | `Idempotency-Key` |
    /// | PayPal | refunds what is left, and reads the order first to find the capture, so **two** calls | `PayPal-Request-Id` |
    ///
    /// A provider that cannot honour
    /// [`RefundRequest::idempotency_key`] refuses the refund with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) rather than
    /// sending it without one. That is
    /// [`ChargeRequest::idempotency_key`](crate::ChargeRequest::idempotency_key)'s
    /// own rule, and it matters more here: accepting a key and dropping it
    /// reads as a guarantee against giving the money back twice, which is the
    /// one thing the caller asked for.
    ///
    /// **A refund that cannot be replayed safely is read back, not resent.**
    /// Each adapter has a call that lists what has already gone back —
    /// `Stripe::refunds`, `PayTr::refunds`, Mollie's `amountRefunded` — and
    /// reading is always safe.
    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error>;

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
