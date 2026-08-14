//! The Stripe client and its [`Provider`] implementation.

use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Error, ErrorKind, Money, NextAction, OrderRef, PaymentId,
    Provider, ProviderId, Raw, Secret,
};
use stripe::{IdempotencyKey, RequestStrategy, StripeRequest};
use stripe_core::payment_intent::{
    CancelPaymentIntent, CapturePaymentIntent, CreatePaymentIntent, RetrievePaymentIntent,
};
use stripe_core::refund::CreateRefund;

use crate::convert;

/// The order reference travels as PaymentIntent metadata under this key.
///
/// Stripe has no field of its own for the merchant's order number, so it goes
/// in metadata and comes back out here.
pub const ORDER_METADATA_KEY: &str = "kasapay_order";

/// How long a request waits before it is given up on.
///
/// A checkout typically holds a database transaction open across this call, so
/// a provider that never answers is a locked cart rather than a slow one. Pass
/// a configured client to [`Stripe::with_client`] to change it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Takes payments through Stripe.
///
/// Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct Stripe {
    inner: Arc<stripe::Client>,
}

impl Stripe {
    /// Builds a client from a secret key.
    #[must_use]
    pub fn new(secret_key: &Secret) -> Self {
        Self {
            inner: Arc::new(stripe::Client::new(secret_key.expose())),
        }
    }

    /// Builds a client over an `async-stripe` client the caller configured.
    ///
    /// The escape hatch for anything this crate does not expose: timeouts,
    /// a pinned API version, an account to act on behalf of.
    #[must_use]
    pub fn with_client(client: stripe::Client) -> Self {
        Self {
            inner: Arc::new(client),
        }
    }

    /// Gives money back off a payment.
    ///
    /// `amount: None` refunds all of it. Repeated partial refunds are allowed
    /// up to what was captured.
    ///
    /// # The currency is the payment's
    ///
    /// Stripe takes a refund amount as a bare integer and applies it in
    /// whatever currency the payment was in — it has no field to say which.
    /// So a [`Money`] in the wrong currency cannot be refused before sending;
    /// it is checked against the currency Stripe reports back, and a mismatch
    /// is an error **after the money has moved**. Read
    /// [`Provider::charge_status`] first if the caller is not certain.
    pub async fn refund(
        &self,
        payment: &PaymentId,
        amount: Option<Money>,
    ) -> Result<Refund, Error> {
        let mut create = CreateRefund::new().payment_intent(payment.as_str().to_owned());
        if let Some(amount) = amount {
            create = create.amount(amount.minor_units());
        }
        let refund = create
            .customize()
            .timeout(DEFAULT_TIMEOUT)
            .send(self.inner.as_ref())
            .await
            .map_err(|e| convert::error(&e).with_source(e))?;

        let refunded = convert::amount(refund.amount, &refund.currency)?;
        if let Some(asked) = amount
            && asked.currency() != refunded.currency()
        {
            return Err(Error::new(
                ErrorKind::Malformed,
                convert::PROVIDER,
                format!(
                    "asked to refund {asked} and Stripe refunded {refunded}: \
                     the payment was not in the currency the caller thought"
                ),
            ));
        }

        Ok(Refund {
            id: refund.id.as_str().into(),
            payment: payment.clone(),
            amount: refunded,
            status: refund
                .status
                .as_deref()
                .map_or(RefundState::Pending, RefundState::from),
            raw: Raw::from_json(&serde_json::json!({
                "id": refund.id.as_str(),
                "amount": refund.amount,
                "currency": format!("{:?}", refund.currency),
                "status": refund.status,
                "reason": refund.reason.map(|r| format!("{r:?}")),
                "failure_reason": refund.failure_reason,
            })),
        })
    }

    /// Withdraws a payment that has not been captured.
    ///
    /// Captured money is refunded, not cancelled. Stripe answers
    /// `invalid_request` for a captured intent, which arrives as
    /// [`ErrorKind::InvalidRequest`].
    pub async fn cancel(&self, payment: &PaymentId) -> Result<Charge, Error> {
        let intent = CancelPaymentIntent::new(payment.as_str().to_owned())
            .customize()
            .timeout(DEFAULT_TIMEOUT)
            .send(self.inner.as_ref())
            .await
            .map_err(|e| convert::error(&e).with_source(e))?;
        into_charge(&intent)
    }

    /// The underlying `async-stripe` client, for calls kasapay does not model.
    #[must_use]
    pub fn client(&self) -> &stripe::Client {
        &self.inner
    }
}

#[async_trait::async_trait]
impl Provider for Stripe {
    fn id(&self) -> ProviderId {
        convert::PROVIDER
    }

    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        let mut create = CreatePaymentIntent::new(
            request.amount.minor_units(),
            convert::currency(request.amount.currency())?,
        )
        .metadata(metadata(request));
        if let Some(description) = &request.description {
            create = create.description(description.to_string());
        }
        if let Some(customer) = &request.customer {
            create = create.customer(customer.to_string());
        }
        if let Some(return_url) = &request.return_url {
            create = create.return_url(return_url.to_string());
        }

        let intent = match &request.idempotency_key {
            Some(key) => {
                create
                    .customize()
                    .request_strategy(RequestStrategy::Idempotent(idempotency_key(key)?))
                    .timeout(DEFAULT_TIMEOUT)
                    .send(self.inner.as_ref())
                    .await
            }
            None => {
                create
                    .customize()
                    .timeout(DEFAULT_TIMEOUT)
                    .send(self.inner.as_ref())
                    .await
            }
        }
        .map_err(|e| convert::error(&e).with_source(e))?;
        into_charge(&intent)
    }

    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let intent = RetrievePaymentIntent::new(id.as_str().to_owned())
            .customize()
            .timeout(DEFAULT_TIMEOUT)
            .send(self.inner.as_ref())
            .await
            .map_err(|e| convert::error(&e).with_source(e))?;
        into_charge(&intent)
    }

    async fn capture(&self, id: &PaymentId, amount: Option<Money>) -> Result<Charge, Error> {
        let mut capture = CapturePaymentIntent::new(id.as_str().to_owned());
        if let Some(amount) = amount {
            amount.require_positive().map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    convert::PROVIDER,
                    "a capture takes an amount above zero, or None for the lot",
                )
                .with_source(e)
            })?;
            capture = capture.amount_to_capture(amount.minor_units());
        }
        let intent = capture
            .customize()
            .timeout(DEFAULT_TIMEOUT)
            .send(self.inner.as_ref())
            .await
            .map_err(|e| convert::error(&e).with_source(e))?;
        into_charge(&intent)
    }

    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error> {
        Stripe::cancel(self, id).await
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: true,
            partial_capture: true,
            partial_refund: true,
            repeated_refund: true,
        }
    }
}

/// Stripe bounds the key it will accept; a longer one is refused before sending.
fn idempotency_key(key: &kasapay_core::IdempotencyKey) -> Result<IdempotencyKey, Error> {
    IdempotencyKey::new(key.as_str()).map_err(|e| {
        Error::new(
            ErrorKind::InvalidRequest,
            convert::PROVIDER,
            "Stripe will not accept this idempotency key",
        )
        .with_source(e)
    })
}

fn metadata(request: &ChargeRequest) -> std::collections::HashMap<String, String> {
    let mut pairs: std::collections::HashMap<String, String> = request
        .metadata
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    pairs.insert(
        ORDER_METADATA_KEY.to_owned(),
        request.order.as_str().to_owned(),
    );
    pairs
}

fn into_charge(intent: &stripe_shared::PaymentIntent) -> Result<Charge, Error> {
    let status = convert::status(&intent.status);
    // A partial capture leaves `amount` at what was authorised and shows up only in `amount_received`.
    let amount = if status == kasapay_core::Status::Captured && intent.amount_received > 0 {
        convert::amount(intent.amount_received, &intent.currency)?
    } else {
        convert::amount(intent.amount, &intent.currency)?
    };
    let order = intent
        .metadata
        .get(ORDER_METADATA_KEY)
        .map(|value| OrderRef::new(value.as_str()));
    let next_action = if status == kasapay_core::Status::RequiresAction {
        intent
            .client_secret
            .as_deref()
            .map(|secret| NextAction::ConfirmOnClient {
                client_secret: secret.into(),
            })
    } else {
        None
    };
    let raw = serde_json::to_value(RawIntent::from(intent))
        .map(|value| Raw::from_json(&value))
        .map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                convert::PROVIDER,
                "PaymentIntent could not be echoed as JSON",
            )
            .with_source(e)
        })?;

    Ok(Charge {
        id: PaymentId::new(intent.id.as_str()),
        order,
        amount,
        // Stripe has no basket at the PaymentIntent level, so there is nothing
        // to say here rather than something equal.
        order_amount: None,
        status,
        next_action,
        provider: convert::PROVIDER,
        raw,
    })
}

/// Where a refund has got to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefundState {
    /// Sent, not yet settled.
    Pending,
    /// Waiting on somebody — a bank transfer refund needs the payer's details.
    RequiresAction,
    /// The money is back.
    Succeeded,
    /// It did not go back, and `failure_reason` on the raw response says why.
    Failed,
    /// Withdrawn before it settled.
    Canceled,
    /// A state Stripe has added since this was written.
    Other(Box<str>),
}

impl From<&str> for RefundState {
    fn from(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "requires_action" => Self::RequiresAction,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "canceled" => Self::Canceled,
            other => Self::Other(other.into()),
        }
    }
}

/// Money given back off a Stripe payment.
#[derive(Debug, Clone)]
pub struct Refund {
    /// Stripe's own id for the refund, which a second attempt must not reuse.
    pub id: Box<str>,
    /// The payment it came off.
    pub payment: PaymentId,
    /// How much went back.
    pub amount: Money,
    /// Where it has got to.
    pub status: RefundState,
    /// The fields of Stripe's answer worth keeping.
    pub raw: Raw,
}

/// What lands on [`Charge::raw`] for Stripe.
///
/// A reconstruction rather than the body Stripe sent: `async-stripe`
/// deserializes with miniserde and hands back a typed PaymentIntent, so the
/// original bytes are gone by the time this crate sees it. These are the
/// fields worth keeping; for anything more, take the intent from
/// [`Stripe::client`] directly.
#[derive(serde::Serialize)]
struct RawIntent<'a> {
    id: &'a str,
    amount: i64,
    currency: String,
    status: String,
    client_secret: Option<&'a str>,
    metadata: &'a std::collections::HashMap<String, String>,
}

impl<'a> From<&'a stripe_shared::PaymentIntent> for RawIntent<'a> {
    fn from(intent: &'a stripe_shared::PaymentIntent) -> Self {
        Self {
            id: intent.id.as_str(),
            amount: intent.amount,
            currency: format!("{:?}", intent.currency),
            status: format!("{:?}", intent.status),
            client_secret: intent.client_secret.as_deref(),
            metadata: &intent.metadata,
        }
    }
}
