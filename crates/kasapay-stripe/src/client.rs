//! The Stripe client and its [`Provider`] implementation.

use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{
    Charge, ChargeRequest, Error, ErrorKind, NextAction, OrderRef, PaymentId, Provider, ProviderId,
    Secret,
};
use stripe::{IdempotencyKey, RequestStrategy, StripeRequest};
use stripe_core::payment_intent::{CreatePaymentIntent, RetrievePaymentIntent};

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
            convert::currency(request.amount.currency()),
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
    let amount = convert::amount(intent.amount, &intent.currency)?;
    let status = convert::status(&intent.status);
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
    let raw = serde_json::to_value(RawIntent::from(intent)).map_err(|e| {
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
        status,
        next_action,
        provider: convert::PROVIDER,
        raw,
    })
}

/// What lands on [`Charge::raw`] for Stripe.
///
/// `async-stripe` deserializes with miniserde, so a PaymentIntent cannot be
/// re-serialized with serde; this carries the fields worth keeping instead.
/// For anything more, take the intent from [`Stripe::client`] directly.
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
