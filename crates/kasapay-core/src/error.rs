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
    /// The provider does not offer what was asked of it.
    Unsupported,
    /// The provider failed on its own side.
    Provider,
}

impl ErrorKind {
    /// Whether replaying the same request unchanged could plausibly succeed.
    ///
    /// A retry still needs an idempotency key: this says the error is not a
    /// verdict, not that the first attempt did nothing.
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
        self.source.as_ref().map(|e| &**e as &(dyn StdError + 'static))
    }
}
