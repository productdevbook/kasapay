//! The trait every payment provider implements.

use std::fmt;

use crate::charge::{Charge, ChargeRequest, PaymentId};
use crate::error::Error;

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
}
