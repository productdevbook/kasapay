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

    /// Registers a till user, and answers which banks they are enrolled with.
    ///
    /// This is where a `ChargeRequest::customer` comes from: In-Store requires
    /// one and there was no way to make one before this. A user that exists is
    /// not yet a user that can take money — see [`User::can_take_payment`].
    pub async fn create_user(&self, user_id: &str) -> Result<User, Error> {
        let request = self
            .inner
            .http
            .post(self.endpoint("user")?)
            .json(&wire::UserRequest { user_id });
        let (response, _) = self.send::<wire::UserDetail>(request).await?;
        user_from(response)
    }

    /// The till users registered with this merchant, a page at a time.
    ///
    /// `page` counts from one, which is iyzico's own convention and not this
    /// crate's choice.
    pub async fn users(&self, page: u32, per_page: u32) -> Result<Vec<User>, Error> {
        let request = self.inner.http.get(self.endpoint("user/list")?).query(&[
            ("pageNumber", page.to_string()),
            ("pageCount", per_page.to_string()),
        ]);
        let (response, _) = self.send::<wire::UserListResponse>(request).await?;
        if let Some(error) = user_refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the user list",
        ) {
            return Err(error);
        }
        response.user_list.into_iter().map(user_from).collect()
    }

    /// Forgets a till user.
    pub async fn forget_user(&self, user_id: &str) -> Result<(), Error> {
        let request = self
            .inner
            .http
            .delete(self.endpoint("user")?)
            .json(&wire::UserRequest { user_id });
        let (response, _) = self.send::<wire::DeleteUserResponse>(request).await?;
        if let Some(error) = user_refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to forget the user",
        ) {
            return Err(error);
        }
        // iyzico echoes the user it deleted. A different one coming back means
        // something other than what was asked for is gone, which is worth an
        // error rather than a silent success.
        match response.user_id.as_deref() {
            Some(deleted) if deleted != user_id => Err(Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                format!("asked iyzico to forget {user_id} and it answered {deleted}"),
            )),
            _ => Ok(()),
        }
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
    /// `data` arrives as a query parameter on the callback URL and is
    /// therefore percent-encoded. **Decode it before passing it here**; this
    /// call sends what it is given, and iyzico's own documentation says an
    /// encoded value fails to decrypt.
    ///
    /// # Which version
    ///
    /// `/v3/in-store/crypt/decrypt`, under the configured base. iyzico
    /// documents this operation at both versions and v3 is the current one;
    /// v2 is a separate, older integration rather than another base for this
    /// client, so do not point [`Config::new`] at it. See the module docs.
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
        // The same value under both names iyzico documents for it. See
        // wire::RefundRequest for why.
        let refund_amount = amount.map(decimal_number).transpose()?;
        let refund_price = amount.map(decimal_number).transpose()?;
        let body = wire::RefundRequest {
            user_id,
            payment_id: numeric,
            refund_amount,
            refund_price,
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
                "the In-Store API documents no idempotency mechanism; \
                 orderId is the closest thing it has and it is not one",
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

/// A till user, and the banks they can take money through.
#[derive(Debug, Clone)]
pub struct User {
    /// What `ChargeRequest::customer` must carry for this user.
    pub id: Box<str>,
    /// The banks and terminals this user is registered with.
    pub enrollments: Vec<Enrollment>,
}

impl User {
    /// Whether this user is enrolled anywhere that will take a payment.
    ///
    /// Creating a user does not enrol them: a user with no enrollments exists
    /// and cannot charge, and iyzico reports that as a failed payment rather
    /// than as a bad request. Checking here is cheaper than finding out at the
    /// till.
    #[must_use]
    pub fn can_take_payment(&self) -> bool {
        self.enrollments.iter().any(Enrollment::is_active)
    }
}

/// One bank a till user is registered with.
#[derive(Debug, Clone)]
pub struct Enrollment {
    /// The bank's name.
    pub bank: Option<Box<str>>,
    /// The physical terminal's identifier at that bank.
    pub terminal: Option<Box<str>>,
    /// iyzico's own word for where the enrolment has got to.
    pub status: Option<Box<str>>,
}

impl Enrollment {
    /// Whether this enrolment is one that can take money.
    ///
    /// iyzico documents no set of values for `enrollmentStatus`, so anything
    /// that is not plainly a refusal is read as usable. A caller that needs
    /// certainty should read [`Enrollment::status`] itself.
    #[must_use]
    pub fn is_active(&self) -> bool {
        match self.status.as_deref() {
            Some(status) => !matches!(
                status.to_ascii_uppercase().as_str(),
                "PASSIVE" | "PASIF" | "FAILED" | "REJECTED" | "CANCELLED" | "CANCELED"
            ),
            // No status at all is how the create response comes back, and a
            // fresh enrolment is not a refusal.
            None => true,
        }
    }
}

/// Turns a `status: "failure"` envelope into an error, and anything else into `None`.
fn user_refused(
    status: Option<&str>,
    message: Option<String>,
    code: Option<String>,
    fallback: &str,
) -> Option<Error> {
    if !failed(status) {
        return None;
    }
    let error = Error::new(
        crate::errors::kind_for(code.as_deref(), ErrorKind::InvalidRequest),
        PROVIDER,
        message.unwrap_or_else(|| fallback.to_owned()),
    );
    Some(match code {
        Some(code) => error.with_code(code),
        None => error,
    })
}

fn user_from(detail: wire::UserDetail) -> Result<User, Error> {
    if let Some(error) = user_refused(
        detail.status.as_deref(),
        detail.error_message,
        detail.error_code,
        "iyzico refused the user",
    ) {
        return Err(error);
    }
    Ok(User {
        id: detail.user_id.unwrap_or_default().into_boxed_str(),
        enrollments: detail
            .enrollments
            .into_iter()
            .map(|item| Enrollment {
                bank: item.enrolled_bank.map(String::into_boxed_str),
                terminal: item.enrolled_terminal_id.map(String::into_boxed_str),
                status: item.enrollment_status.map(String::into_boxed_str),
            })
            .collect(),
    })
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

/// Reads a `currencyCode` off an In-Store response.
///
/// The schema types it a string and calls it a currency code, and the only
/// value iyzico publishes is `"0949"` — ISO 4217's **numeric** code for lira,
/// zero-padded — where every other API of theirs writes `TRY`. Both are read;
/// no other numeric code is guessed at, because this API settles in lira only.
fn currency_code(code: Option<&str>) -> Result<Currency, Error> {
    let Some(code) = code else {
        return Ok(Currency::Try);
    };
    if code.trim_start_matches('0') == "949" {
        return Ok(Currency::Try);
    }
    code.parse().map_err(|e: kasapay_core::UnknownCurrency| {
        Error::new(ErrorKind::Malformed, PROVIDER, e.to_string())
    })
}

/// Reads a `BigDecimal` amount sent as a JSON number.
fn amount_in(amount: Option<&serde_json::Number>, currency: Currency) -> Result<Money, Error> {
    amount
        .map(|n| Money::parse(&n.to_string(), currency))
        .transpose()
        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))
        .map(|parsed| parsed.unwrap_or_else(|| Money::from_minor_units(0, currency)))
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
        id: Some(PaymentId::issued(payment_id.to_string())),
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
///
/// A payment that did not go through carries `paymentFailedResult` in place of
/// the transaction, and is [`Status::Failed`] with the amount that was
/// attempted.
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
    let operation = response.operation.as_ref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "a decrypted callback carried no operation",
        )
    })?;

    let (amount, status) = match (
        operation.transaction.as_ref(),
        operation.payment_failed_result.as_ref(),
    ) {
        (Some(transaction), _) => {
            let currency = currency_code(transaction.currency_code.as_deref())?;
            let receipt = transaction.receipt.as_ref();
            let flag =
                |f: fn(&wire::SettledReceipt) -> Option<bool>| receipt.and_then(f).unwrap_or(false);
            let status = if flag(|r| r.void_approved) {
                Status::Canceled
            } else if flag(|r| r.approved) || flag(|r| r.refund_approved) {
                Status::Captured
            } else {
                Status::Failed
            };
            (amount_in(transaction.amount.as_ref(), currency)?, status)
        }
        // A failed payment reports no currency; this API settles in lira only.
        (None, Some(refused)) => (
            amount_in(refused.transaction_amount.as_ref(), Currency::Try)?,
            Status::Failed,
        ),
        (None, None) => {
            return Err(Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a decrypted callback carried neither a transaction nor a failed payment",
            ));
        }
    };

    Ok(Charge {
        id: Some(payment.clone()),
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
    let currency = currency_code(detail.and_then(|d| d.currency_code.as_deref()))?;
    let amount = amount_in(detail.and_then(|d| d.amount.as_ref()), currency)?;

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
        id: Some(PaymentId::issued(payment_id.to_string())),
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
