//! The In-Store API client and its [`Provider`] implementation.

use std::sync::Arc;

use kasapay_core::{
    Charge, ChargeRequest, Currency, Error, ErrorKind, Money, NextAction, OrderRef, PaymentId,
    Provider, ProviderId, Secret, Status,
};
use url::Url;

use crate::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where the client points and what it authenticates with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Url,
    api_key: Secret,
    secret_key: Secret,
    merchant_id: Secret,
}

impl Config {
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
        })
    }
}

/// Takes payments through iyzico's In-Store API v3.
///
/// Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct Iyzico {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
}

impl Iyzico {
    /// Builds a client with its own HTTP connection pool.
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        Ok(Self::with_http(reqwest::Client::builder().build()?, config))
    }

    /// Builds a client over an HTTP client the caller already has.
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
    ) -> Result<(T, serde_json::Value), Error> {
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
        let raw: serde_json::Value = serde_json::from_slice(&body).map_err(|e| {
            Error::new(ErrorKind::Malformed, PROVIDER, "response was not JSON").with_source(e)
        })?;
        let typed = serde_json::from_value(raw.clone()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "response was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        Ok((typed, raw))
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
impl Provider for Iyzico {
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
    raw: serde_json::Value,
    order: Option<OrderRef>,
    amount: Option<Money>,
) -> Result<Charge, Error> {
    if failed(response.status.as_deref()) {
        let message = response
            .error_message
            .unwrap_or_else(|| "iyzico refused the request".to_owned());
        let error = Error::new(ErrorKind::Declined, PROVIDER, message);
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

fn query_into_charge(
    response: wire::PaymentQueryResponse,
    raw: serde_json::Value,
) -> Result<Charge, Error> {
    if failed(response.status.as_deref()) {
        let message = response
            .error_message
            .unwrap_or_else(|| "iyzico refused the query".to_owned());
        let error = Error::new(ErrorKind::Provider, PROVIDER, message);
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
        status,
        next_action: None,
        provider: PROVIDER,
        raw,
    })
}
