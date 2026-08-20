//! The trait every payment provider implements.

use std::fmt;

use crate::charge::{Charge, ChargeRequest, IdempotencyKey, OrderRef};
use crate::error::Error;
use crate::id::PaymentId;
use crate::instrument::Instrument;
use crate::money::Money;
use crate::refund::{Refund, RefundRequest};
use crate::release::Release;

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
    /// [`Provider::lookup`] can answer what became of a request keyed by the
    /// caller's own reference.
    ///
    /// What a crash-recovery path reads before it decides between asking and
    /// calling again. False does not mean the provider forgot the reference —
    /// it means this adapter has no call that finds a payment by it, so a
    /// caller whose request timed out has nothing to ask and must rely on
    /// whatever idempotency the provider offers instead.
    pub lookup_by_order: bool,
    /// [`Provider::resume`] can read back a flow by the token
    /// [`NextAction::Redirect`](crate::NextAction::Redirect) handed over,
    /// without the payment ever having been named.
    ///
    /// What a caller reads when the payer comes back from a hosted form. False
    /// does not mean the flow cannot be finished — it means the provider named
    /// the payment when it opened the flow, so
    /// [`Provider::charge_status`] is what finishes it and the continuation is
    /// not needed. True is the case that has no payment id yet at all.
    pub resume_by_continuation: bool,
    /// An instrument [`Provider::instruments`] lists can be charged — with the
    /// payer entering nothing.
    ///
    /// What a checkout reads before it offers "use my saved card", and what
    /// says whether [`Provider::charge`] will honour
    /// [`ChargeRequest::instrument`] or refuse it with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported).
    ///
    /// This describes *charging*, not *listing*: every adapter answers
    /// [`Provider::instruments`] regardless of this flag, and the two do not
    /// have to agree. PayTR's hosted form does store a card — a vault exists —
    /// but nothing here can list it or charge it, so both answer
    /// `Unsupported`, for two different reasons that happen to give the same
    /// result.
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
    ///
    /// # A request that satisfies one provider may not satisfy another
    ///
    /// Everything past the order reference and the amount is optional on
    /// [`ChargeRequest`], because what is mandatory is the provider's
    /// decision rather than a payment's. iyzico's classic API refuses a
    /// payment without a buyer's identity number, an address and an itemised
    /// basket; PayTR refuses one without the payer's own IP address; Stripe
    /// and Mollie ask for none of it.
    ///
    /// An adapter that is not given a field it needs answers
    /// [`ErrorKind::InvalidRequest`](crate::ErrorKind::InvalidRequest)
    /// **naming the field**, before a socket opens. So the request that works
    /// everywhere is the one carrying what the strictest provider asks, and
    /// swapping to a laxer one costs nothing: the extra fields are ignored.
    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error>;

    /// Finishes a flow [`Provider::charge`] started, by the token it handed
    /// over.
    ///
    /// # The one call that needs it
    ///
    /// A hosted form the payer has not finished is not a payment: the provider
    /// has nothing to name it by, so [`Charge::id`] is `None` and
    /// [`Provider::charge_status`] has nothing to take. What it does have is
    /// the `continuation` on
    /// [`NextAction::Redirect`](crate::NextAction::Redirect), and this is the
    /// call that takes one.
    ///
    /// # How a caller decides which to use, without naming a provider
    ///
    /// [`Capabilities::resume_by_continuation`]. True is a provider that names
    /// the payment only once the payer is done, so the continuation is the
    /// only handle there is and this is what reads it. False is a provider
    /// that named the payment when it opened the flow, so
    /// [`Provider::charge_status`] finishes it and this answers
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) saying so.
    ///
    /// # It finishes what `charge` started, and nothing else
    ///
    /// Not every flow an adapter can open comes back through here. iyzico's
    /// classic API has one result endpoint for a form that takes the money and
    /// a form that holds it, and its answer does not say which was opened — so
    /// reading a hold back through the wrong one writes a sale into a ledger
    /// for money nobody has taken. [`Provider::charge`] opens the form that
    /// takes the money, this reads that one back, and a hold opened by the
    /// adapter's own call is read back by the adapter's own call.
    async fn resume(&self, continuation: &str) -> Result<Charge, Error>;

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
    ///
    /// A provider that cannot honour a key **refuses the capture** with
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported) rather than
    /// sending it without one. That is the same rule
    /// [`ChargeRequest::idempotency_key`](crate::ChargeRequest::idempotency_key)
    /// and [`RefundRequest::idempotency_key`](crate::RefundRequest::idempotency_key)
    /// state, and it is at its sharpest here: a capture is the call that takes
    /// the money, so a key accepted and dropped reads as a guarantee against
    /// taking it twice where there is none. iyzico's classic API is the one
    /// that refuses; a provider with no capture step at all answers
    /// `Unsupported` for the capture itself and never reaches the question.
    ///
    /// The refusal comes before the request, not after it. A key that is
    /// discovered to be unusable only once the capture has been sent has
    /// already taken the money.
    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error>;

    /// Releases an authorisation that will never be taken.
    ///
    /// Every provider whose [`Capabilities::separate_capture`] is true can do
    /// this, and one whose is false answers
    /// [`ErrorKind::Unsupported`](crate::ErrorKind::Unsupported): there is no
    /// hold to release where the money was taken at authorisation. That pair
    /// is asserted for every adapter in
    /// `crates/kasapay/tests/conformance.rs`.
    ///
    /// # Why this answers a [`Release`] rather than a [`Charge`]
    ///
    /// Because three of the four providers that hold funds do not answer a
    /// payment. Mollie sends `202 Accepted` with no body, PayPal `204` with no
    /// body, and iyzico a reversal carrying the bank's own reference and no
    /// payment state. Asking them for a `Charge` is what kept this method from
    /// doing its one documented job at three of the four — each refused rather
    /// than inventing one, which was the honest answer to the wrong question.
    ///
    /// Read [`Release::state`] before telling a payer the money is back:
    /// Mollie's `202` means the request was taken and the issuing bank decides
    /// if and when, which is
    /// [`ReleaseState::Accepted`](crate::ReleaseState::Accepted).
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
    async fn cancel(&self, id: &PaymentId) -> Result<Release, Error>;

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
    /// Reading is always safe. Where to read is not the same everywhere, and
    /// the differences are the point: `Stripe::refunds`, `PayTr::refunds` and
    /// `Mollie::refunds` each list what has gone back; iyzico's classic list is
    /// `reporting::ItemTransaction::refunds`, on a different module from the
    /// one that took the payment, so `Provider::charge_status` is not where to
    /// look; PayPal has no typed listing at all, and what it has is the
    /// capture's own status and the breakdown on [`Charge::raw`]; iyzico's
    /// In-Store API has neither, which is why its `repeated_refund` is
    /// `false`.
    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error>;

    /// Asks what became of a request the caller sent under this reference.
    ///
    /// **For the call whose answer never arrived.** A charge that times out is
    /// the one case where nobody knows whether the money moved: the request may
    /// have been received and acted on, and the reply lost on the way back.
    /// Calling [`Provider::charge`] again is only safe where the provider
    /// honours an idempotency key, and
    /// [`Provider::charge_status`](Provider::charge_status) cannot be used
    /// either — it takes the provider's own identifier for the payment, which
    /// is precisely what a lost reply never delivered.
    ///
    /// So this is keyed by [`ChargeRequest::order`], the caller's own
    /// reference, which they had before they sent anything.
    ///
    /// - `Ok(None)` — the provider has no record of a payment under this
    ///   reference. Nothing was taken, and sending the charge again is safe.
    /// - `Ok(Some(charge))` — this is what became of it. Read
    ///   [`Charge::status`]; sending the charge again would open a second one.
    /// - `Err(_)` — the question could not be answered. **Not** the same as
    ///   `Ok(None)`, and the difference is a double payment.
    ///
    /// # Two of the five can answer it
    ///
    /// [`Capabilities::lookup_by_order`] says which before there is a request
    /// to ask about, and the four answers are different questions:
    ///
    /// | | |
    /// |---|---|
    /// | iyzico `classic` | yes — reporting reads a payment back by the `conversationId` it was made with |
    /// | PayTR | yes — its status query is keyed by `merchant_oid`, which is the reference itself |
    /// | Stripe | **no, on purpose** — the search API is the only way to find an intent by metadata, and Stripe documents it as eventually consistent and says not to use it in read-after-write flows. Retry the charge with the same `ChargeRequest::idempotency_key` instead: Stripe answers the original PaymentIntent rather than opening a second |
    /// | Mollie | no — nothing finds a payment by its metadata. Its `Idempotency-Key` replays the first answer for an hour, which covers the same case for as long as it lasts |
    /// | PayPal | no — Orders v2 has no lookup by `PayPal-Request-Id` or `custom_id`. Replaying with the same request id answers the original order |
    /// | iyzico `in_store` | no — its query takes iyzico's own `paymentId` and nothing else |
    ///
    /// **A `false` here is not a gap to work around with a search that might
    /// be stale.** An answer of "no record" that is merely late is how a
    /// caller authorises twice, which is the failure this method exists to
    /// prevent.
    async fn lookup(&self, order: &OrderRef) -> Result<Option<Charge>, Error>;

    /// Lists what a customer has saved with this provider.
    ///
    /// `customer` is the provider's own name for them — the same string
    /// [`ChargeRequest::customer`] carries, and, for iyzico's classic API,
    /// the `cardUserKey` that names the vault rather than a payer as such.
    ///
    /// This is the shape every provider can answer: an identity and something
    /// to show somebody choosing between them. It is not a card number and
    /// carries no field one could go in.
    ///
    /// **Charging one is [`Provider::charge`]** with
    /// [`ChargeRequest::instrument`] set to an [`Instrument::id`] this
    /// answered and [`ChargeRequest::customer`] to the same `customer` it was
    /// asked with — the two halves of the name. That used to be each adapter's
    /// own call, on the reasoning that iyzico wanted a buyer and a basket
    /// beside the token, Stripe an `off_session` flag and Mollie a
    /// `sequenceType`. The first of those stopped being true when
    /// `ChargeRequest` gained a buyer and a basket, and the other two are one
    /// question under two names, which is [`Sequence`](crate::Sequence).
    ///
    /// What is still each adapter's own is **forgetting** one: iyzico needs
    /// its `cardUserKey` *and* its token where Stripe needs only the
    /// instrument, and nothing here narrows that to one signature.
    ///
    /// See [`Capabilities::saved_instruments`] for which providers can charge
    /// one at all.
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
