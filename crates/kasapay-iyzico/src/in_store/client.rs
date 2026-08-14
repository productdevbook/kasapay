//! The In-Store client and its [`Provider`] implementation.

use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Currency, Error, ErrorKind, Money, NextAction, OrderRef,
    PaymentId, Provider, ProviderId, Raw, Secret, Status,
};
use url::Url;

use crate::in_store::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where the client points and what it authenticates with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Url,
    api_key: Secret,
    secret_key: Secret,
    merchant_id: Secret,
    timeout: Duration,
}

impl Config {
    /// How long a request waits before it is given up on.
    ///
    /// A checkout typically holds a database transaction open across this call,
    /// so a provider that never answers is a locked cart rather than a slow one.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// The production base, `https://api.iyzipay.com/v3/in-store`.
    pub const PRODUCTION: &'static str = "https://api.iyzipay.com/v3/in-store/";
    /// The sandbox base, `https://sandbox-api.iyzipay.com/v3/in-store`.
    pub const SANDBOX: &'static str = "https://sandbox-api.iyzipay.com/v3/in-store/";

    /// Points at the sandbox with the given credentials.
    pub fn sandbox(
        api_key: impl Into<Secret>,
        secret_key: impl Into<Secret>,
        merchant_id: impl Into<Secret>,
    ) -> Self {
        Self::new(Self::SANDBOX, api_key, secret_key, merchant_id)
            .unwrap_or_else(|_| unreachable!("the sandbox constant parses"))
    }

    /// Points at production with the given credentials.
    pub fn production(
        api_key: impl Into<Secret>,
        secret_key: impl Into<Secret>,
        merchant_id: impl Into<Secret>,
    ) -> Self {
        Self::new(Self::PRODUCTION, api_key, secret_key, merchant_id)
            .unwrap_or_else(|_| unreachable!("the production constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    ///
    /// The base is joined against, so it must end in a slash.
    pub fn new(
        base_url: &str,
        api_key: impl Into<Secret>,
        secret_key: impl Into<Secret>,
        merchant_id: impl Into<Secret>,
    ) -> Result<Self, url::ParseError> {
        let base_url = Url::parse(base_url)?;
        Ok(Self {
            base_url,
            api_key: api_key.into(),
            secret_key: secret_key.into(),
            merchant_id: merchant_id.into(),
            timeout: Self::DEFAULT_TIMEOUT,
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Takes payments through iyzico's In-Store API v3.
///
/// Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
}

impl Client {
    /// Builds a client with its own HTTP connection pool.
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self::with_http(http, config))
    }

    /// Builds a client over an HTTP client the caller already has.
    ///
    /// The caller's own timeout applies; [`Config::timeout`] is ignored here,
    /// because a client that already has one should not have it overridden.
    #[must_use]
    pub fn with_http(http: reqwest::Client, config: Config) -> Self {
        Self {
            inner: Arc::new(Inner { http, config }),
        }
    }

    fn endpoint(&self, path: &str) -> Result<Url, Error> {
        self.inner.config.base_url.join(path).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PROVIDER, "endpoint is not a URL").with_source(e)
        })
    }

    fn authenticated(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let config = &self.inner.config;
        request
            .header("x-api-key", config.api_key.expose())
            .header("x-secret-key", config.secret_key.expose())
            .header("x-merchant-id", config.merchant_id.expose())
    }

    /// Sends a request and returns both the typed body and the body verbatim.
    ///
    /// The verbatim copy is what ends up on [`Charge::raw`]; without it every
    /// field iyzico sends that kasapay does not model would be dropped.
    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(T, Raw), Error> {
        let response = self
            .authenticated(request)
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        if !status.is_success() {
            return Err(http_error(status, &body));
        }
        let text = String::from_utf8_lossy(&body).into_owned();
        let typed = serde_json::from_slice(&body).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "response was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        Ok((typed, Raw::from_text(text)))
    }

    /// Reads the encrypted result iyzico posts to the callback address.
    ///
    /// This is how an In-Store payment finishes. [`Provider::charge`] hands
    /// back a [`NextAction::Redirect`] whose `continuation` is the
    /// `paymentSessionToken`; when the payer is done, iyzico posts an
    /// encrypted `data` blob to the `x-callback-url`, and this is what opens
    /// it. Without it a caller has no supported way to learn the outcome
    /// except to poll [`Provider::charge_status`].
    ///
    /// The `payment` argument is the [`PaymentId`] from the charge that
    /// started this. The decrypted body does not carry one — it has a
    /// `recordId`, which is a different identifier and not the one
    /// `charge_status` or `refund` accept.
    ///
    /// # Which version
    ///
    /// This calls `/crypt/decrypt` under the configured base, so `/v3` for the
    /// default configuration. iyzico also documents the same operation under
    /// `/v2/in-store` with identical request and response bodies; whether that
    /// is the deprecated one or the live one is not settled — see the crate
    /// docs. Point [`Config::new`] at the v2 base to reach it.
    pub async fn decrypt_callback(
        &self,
        payment: &PaymentId,
        data: &str,
        session_token: &str,
    ) -> Result<Charge, Error> {
        let body = wire::DecryptRequest {
            data,
            payment_session_token: session_token,
        };
        let request = self
            .inner
            .http
            .post(self.endpoint("crypt/decrypt")?)
            .json(&body);
        let (response, raw) = self.send::<wire::DecryptResponse>(request).await?;
        decrypted_into_charge(payment, response, raw)
    }

    /// Cancels or refunds a payment, in whole or in part.
    ///
    /// Like [`Provider::charge`], this only starts the flow: the payer
    /// approves it through the returned deep link.
    pub async fn refund(
        &self,
        user_id: &str,
        payment_id: &PaymentId,
        amount: Option<Money>,
        callback_url: &Url,
    ) -> Result<Charge, Error> {
        let numeric = numeric_payment_id(payment_id)?;
        let body = wire::RefundRequest {
            user_id,
            payment_id: numeric,
            refund_amount: amount.map(decimal_number).transpose()?,
        };
        let request = self
            .inner
            .http
            .post(self.endpoint("payment/refund")?)
            .header("x-callback-url", callback_url.as_str())
            .json(&body);
        let (response, raw) = self.send::<wire::SessionResponse>(request).await?;
        session_into_charge(response, raw, None, amount)
    }
}

#[async_trait::async_trait]
impl Provider for Client {
    fn id(&self) -> ProviderId {
        PROVIDER
    }

    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        if request.amount.currency() != Currency::Try {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PROVIDER,
                format!(
                    "the In-Store API settles in TRY only, was asked for {}",
                    request.amount.currency()
                ),
            ));
        }
        let user_id = request.customer.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "ChargeRequest::customer carries iyzico's userId and is required",
            )
        })?;
        if request.idempotency_key.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PROVIDER,
                "the In-Store API documents no idempotency mechanism;                  orderId is the closest thing it has and it is not one",
            ));
        }
        let callback_url = request.return_url.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "ChargeRequest::return_url is required: it becomes the x-callback-url header",
            )
        })?;

        let body = wire::PaymentInitRequest {
            user_id,
            order_id: request.order.as_str(),
            amount: decimal_number(request.amount)?,
        };
        let http = self
            .inner
            .http
            .post(self.endpoint("payment/init")?)
            .header("x-callback-url", callback_url.as_str())
            .json(&body);
        let (response, raw) = self.send::<wire::SessionResponse>(http).await?;
        session_into_charge(
            response,
            raw,
            Some(request.order.clone()),
            Some(request.amount),
        )
    }

    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let numeric = numeric_payment_id(id)?;
        let request = self
            .inner
            .http
            .get(self.endpoint("payment/query")?)
            .query(&[("paymentId", numeric)]);
        let (response, raw) = self.send::<wire::PaymentQueryResponse>(request).await?;
        query_into_charge(response, raw)
    }

    /// Always [`ErrorKind::Unsupported`]: the In-Store flow has no capture step.
    ///
    /// The payer approves the payment in iyzico's app and the money is taken
    /// there and then, so a payment this API reports on is either taken or
    /// never happened; there is nothing held for a later call to take.
    /// [`Capabilities::separate_capture`] says the same thing before a caller
    /// gets this far.
    ///
    /// Answering `Ok` with the amount unchanged would be the more convenient
    /// lie: it would put a capture in the caller's ledger at a time when no
    /// money moved.
    async fn capture(&self, _id: &PaymentId, _amount: Option<Money>) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            "the In-Store API takes the money when the payer approves it and \
             documents no capture step",
        ))
    }

    /// Always [`ErrorKind::Unsupported`]: there is no authorisation to release.
    ///
    /// Giving back money the payer has already handed over is a refund, and
    /// that is [`Client::refund`] — which needs a callback address, because
    /// iyzico makes the payer approve it too.
    async fn cancel(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            "the In-Store API holds no authorisation to release; \
             giving the money back is Client::refund",
        ))
    }

    /// No separate capture, and refunds only in the ways iyzico documents.
    ///
    /// `repeated_refund` is false because the In-Store documentation says
    /// nothing about refunding a payment twice, and a capability is a promise
    /// rather than a guess.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: false,
            partial_capture: false,
            partial_refund: true,
            repeated_refund: false,
        }
    }
}

fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PROVIDER, error.to_string())
}

fn http_error(status: reqwest::StatusCode, body: &[u8]) -> Error {
    let parsed: Option<wire::ErrorResponse> = serde_json::from_slice(body).ok();
    let kind = match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    };
    let message = parsed
        .as_ref()
        .and_then(|e| e.error_message.clone())
        .unwrap_or_else(|| format!("HTTP {status}"));
    let error = Error::new(kind, PROVIDER, message);
    match parsed.and_then(|e| e.error_code) {
        Some(code) => error.with_code(code),
        None => error,
    }
}

/// Writes an amount as a bare JSON number, exactly as the decimal it is.
fn decimal_number(amount: Money) -> Result<Box<serde_json::value::RawValue>, Error> {
    serde_json::value::RawValue::from_string(amount.to_decimal_string()).map_err(|e| {
        Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "amount is not writable as a JSON number",
        )
        .with_source(e)
    })
}

/// iyzico's `paymentId` is a signed 64-bit integer, not the opaque string
/// [`PaymentId`] holds for every provider.
fn numeric_payment_id(id: &PaymentId) -> Result<i64, Error> {
    id.as_str().parse().map_err(|e| {
        Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            format!("`{id}` is not an iyzico paymentId"),
        )
        .with_source(e)
    })
}

fn failed(response_status: Option<&str>) -> bool {
    matches!(response_status, Some(s) if !s.eq_ignore_ascii_case("success"))
}

fn session_into_charge(
    response: wire::SessionResponse,
    raw: Raw,
    order: Option<OrderRef>,
    amount: Option<Money>,
) -> Result<Charge, Error> {
    if failed(response.status.as_deref()) {
        let message = response
            .error_message
            .unwrap_or_else(|| "iyzico refused the request".to_owned());
        let error = Error::new(
            crate::errors::kind_for(response.error_code.as_deref(), ErrorKind::Declined),
            PROVIDER,
            message,
        );
        return Err(match response.error_code {
            Some(code) => error.with_code(code),
            None => error,
        });
    }
    let payment_id = response.payment_id.ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "a successful response carried no paymentId",
        )
    })?;
    let next_action = match response.deep_link_url {
        Some(url) => Some(NextAction::Redirect {
            url: Url::parse(&url).map_err(|e| {
                Error::new(ErrorKind::Malformed, PROVIDER, "deepLinkUrl is not a URL")
                    .with_source(e)
            })?,
            continuation: response.payment_session_token.map(Into::into),
        }),
        None => None,
    };
    Ok(Charge {
        id: PaymentId::new(payment_id.to_string()),
        order,
        amount: amount.unwrap_or_else(|| Money::from_minor_units(0, Currency::Try)),
        order_amount: None,
        status: if next_action.is_some() {
            Status::RequiresAction
        } else {
            Status::Pending
        },
        next_action,
        provider: PROVIDER,
        raw,
    })
}

/// Reads a decrypted callback as a [`Charge`].
///
/// The receipt's three approval flags are the only thing saying which operation
/// the callback reports on. A refund's outcome has nowhere to go in [`Status`],
/// which has no refunded variant, so a refunded payment stays [`Status::Captured`]
/// and the flags are left on [`Charge::raw`].
fn decrypted_into_charge(
    payment: &PaymentId,
    response: wire::DecryptResponse,
    raw: Raw,
) -> Result<Charge, Error> {
    if failed(response.status.as_deref()) {
        let message = response
            .error_message
            .unwrap_or_else(|| "iyzico refused to decrypt the callback".to_owned());
        let error = Error::new(
            crate::errors::kind_for(response.error_code.as_deref(), ErrorKind::InvalidRequest),
            PROVIDER,
            message,
        );
        return Err(match response.error_code {
            Some(code) => error.with_code(code),
            None => error,
        });
    }
    let transaction = response
        .operation
        .as_ref()
        .and_then(|o| o.transaction.as_ref())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a decrypted callback carried no transaction",
            )
        })?;

    let currency = transaction
        .currency_code
        .as_deref()
        .map_or(Ok(Currency::Try), str::parse)
        .map_err(|e: kasapay_core::UnknownCurrency| {
            Error::new(ErrorKind::Malformed, PROVIDER, e.to_string())
        })?;
    let amount = transaction
        .amount
        .as_ref()
        .map(|n| Money::parse(&n.to_string(), currency))
        .transpose()
        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?
        .unwrap_or_else(|| Money::from_minor_units(0, currency));

    let receipt = transaction.receipt.as_ref();
    let flag = |f: fn(&wire::SettledReceipt) -> Option<bool>| receipt.and_then(f).unwrap_or(false);
    let status = if flag(|r| r.void_approved) {
        Status::Canceled
    } else if flag(|r| r.approved) || flag(|r| r.refund_approved) {
        Status::Captured
    } else {
        Status::Failed
    };

    Ok(Charge {
        id: payment.clone(),
        order: None,
        amount,
        // The In-Store API reports one amount; there is no basket beside it.
        order_amount: None,
        status,
        next_action: None,
        provider: PROVIDER,
        raw,
    })
}

fn query_into_charge(response: wire::PaymentQueryResponse, raw: Raw) -> Result<Charge, Error> {
    if failed(response.status.as_deref()) {
        let message = response
            .error_message
            .unwrap_or_else(|| "iyzico refused the query".to_owned());
        let error = Error::new(
            crate::errors::kind_for(response.error_code.as_deref(), ErrorKind::Provider),
            PROVIDER,
            message,
        );
        return Err(match response.error_code {
            Some(code) => error.with_code(code),
            None => error,
        });
    }
    let payment_id = response.payment_id.ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "a successful query carried no paymentId",
        )
    })?;
    let detail = response.transaction_detail.as_ref();
    let currency = detail
        .and_then(|d| d.currency_code.as_deref())
        .map_or(Ok(Currency::Try), str::parse)
        .map_err(|e: kasapay_core::UnknownCurrency| {
            Error::new(ErrorKind::Malformed, PROVIDER, e.to_string())
        })?;
    let amount = detail
        .and_then(|d| d.amount.as_ref())
        .map(|n| Money::parse(&n.to_string(), currency))
        .transpose()
        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?
        .unwrap_or_else(|| Money::from_minor_units(0, currency));

    // The query endpoint reports no status field of its own. `receipt.approved`
    // is the only settled/unsettled signal the documented body carries.
    let approved = detail
        .and_then(|d| d.receipt.as_ref())
        .and_then(|r| r.approved)
        .unwrap_or(false);
    let refundable = detail.and_then(|d| d.is_refundable).unwrap_or(false);
    let status = if approved || refundable {
        Status::Captured
    } else {
        Status::Pending
    };

    Ok(Charge {
        id: PaymentId::new(payment_id.to_string()),
        order: response.order_id.map(OrderRef::new),
        amount,
        // The In-Store API reports one amount; there is no basket beside it.
        order_amount: None,
        status,
        next_action: None,
        provider: PROVIDER,
        raw,
    })
}
