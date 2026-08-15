//! The one error every provider reports through.

use std::error::Error as StdError;
use std::fmt;

use crate::provider::ProviderId;

/// What went wrong, in terms a caller can branch on without knowing the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Credentials were missing, wrong, or not allowed to do this.
    Auth,
    /// The request was rejected before it reached the card network.
    InvalidRequest,
    /// The bank or the network refused the payment.
    Declined,
    /// The payment was not found, or no longer exists.
    NotFound,
    /// The provider asked us to slow down.
    RateLimited,
    /// The request never got a usable answer: DNS, TLS, timeout, socket.
    Transport,
    /// The provider answered, but with something this crate cannot read.
    Malformed,
    /// The answer could not be shown to have come from the provider.
    ///
    /// A signature that does not match, or one that is missing where the
    /// provider always sends it. Not a transport failure and not a decline: it
    /// means the message may not be theirs, and it must never be acted on.
    Untrusted,
    /// The provider does not offer what was asked of it.
    Unsupported,
    /// The provider failed on its own side.
    Provider,
}

impl ErrorKind {
    /// Whether replaying the same request unchanged could plausibly succeed.
    ///
    /// # This does not mean the retry is safe
    ///
    /// It says the failure was not a verdict. It says nothing about whether
    /// the first attempt took the money — a timeout is exactly the case where
    /// nobody knows.
    ///
    /// Replaying a **charge** is safe only where the provider offers
    /// idempotency, and not every one does:
    ///
    /// | | replaying a charge |
    /// |---|---|
    /// | Stripe | safe — `ChargeRequest::idempotency_key` is sent as `Idempotency-Key` |
    /// | iyzico | **not documented safe** — it refuses an idempotency key, and does not say what a reused `orderId` does |
    /// | PayTR | **not documented safe** — no idempotency mechanism is documented for opening a payment |
    /// | Mollie | safe — `ChargeRequest::idempotency_key` is sent as `Idempotency-Key`, and Mollie replays the first answer for an hour |
    /// | PayPal | safe — `ChargeRequest::idempotency_key` is sent as `PayPal-Request-Id`, and PayPal returns the first answer for a repeated key |
    ///
    /// Where it is not safe, read the payment back with
    /// [`Provider::charge_status`](crate::Provider::charge_status) before
    /// sending it again. Reading is always safe.
    ///
    /// **Replaying a capture is a narrower question, because a capture takes
    /// money rather than opening a request for it.**
    /// [`Provider::capture`](crate::Provider::capture) carries its own
    /// `idempotency`, and what a timeout means depends on whether one was
    /// sent:
    ///
    /// | | replaying a capture, with a key | replaying a capture, without one |
    /// |---|---|---|
    /// | Stripe | safe — sent as `Idempotency-Key`, same as a charge | **not safe** — a second PaymentIntent capture can take the funds twice |
    /// | iyzico | n/a — neither `in_store` nor `classic` implements capture | n/a |
    /// | PayTR | n/a — no capture step; the hosted form takes the money as it goes | n/a |
    /// | Mollie | safe — sent as `Idempotency-Key` on the captures endpoint, answered from the cache for an hour | **not safe** — a second capture against the same authorisation can take the funds twice |
    /// | PayPal | safe — sent as `PayPal-Request-Id`, same as opening an order | **not safe, and PayPal documents it** — a second capture of the same order can take the funds twice |
    ///
    /// Where it is not safe, this table's answer does not change: read the
    /// payment back with
    /// [`Provider::charge_status`](crate::Provider::charge_status) rather
    /// than sending [`Provider::capture`](crate::Provider::capture) again. A
    /// capture whose outcome `is_retryable` does not resolve is read back,
    /// never resent — with a key, resending is safe but reading is still
    /// simpler and costs nothing extra.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::RateLimited | Self::Transport | Self::Provider)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Auth => "authentication",
            Self::InvalidRequest => "invalid request",
            Self::Declined => "declined",
            Self::NotFound => "not found",
            Self::RateLimited => "rate limited",
            Self::Transport => "transport",
            Self::Malformed => "malformed response",
            Self::Untrusted => "unverified response",
            Self::Unsupported => "unsupported",
            Self::Provider => "provider failure",
        };
        f.write_str(text)
    }
}

/// A payment operation failed.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    provider: ProviderId,
    message: Box<str>,
    code: Option<Box<str>>,
    source: Option<Box<dyn StdError + Send + Sync>>,
}

impl Error {
    /// Builds an error attributed to a provider.
    pub fn new(kind: ErrorKind, provider: ProviderId, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            provider,
            message: message.into(),
            code: None,
            source: None,
        }
    }

    /// Attaches the provider's own error code, verbatim.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<Box<str>>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Attaches the underlying error.
    #[must_use]
    pub fn with_source(mut self, source: impl StdError + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// What went wrong.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Which provider reported it.
    #[must_use]
    pub const fn provider(&self) -> ProviderId {
        self.provider
    }

    /// The provider's own error code, when it gave one.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Whether replaying the same request unchanged could plausibly succeed.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        self.kind.is_retryable()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.provider, self.message, self.kind)?;
        if let Some(code) = &self.code {
            write!(f, " [{code}]")?;
        }
        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source
            .as_ref()
            .map(|e| &**e as &(dyn StdError + 'static))
    }
}
