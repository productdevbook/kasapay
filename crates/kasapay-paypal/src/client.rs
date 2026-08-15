//! The PayPal client.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::{Duration, SystemTime};

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Error, ErrorKind, IdempotencyKey, Instrument, Money,
    NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, Secret, Status,
};
use url::Url;

use crate::PAYPAL;
use crate::convert;
use crate::id::{AuthorizationId, CaptureId, RefundId};
use crate::wire;

/// The one OAuth2 `grant_type` this crate ever sends.
const CLIENT_CREDENTIALS: &str = "client_credentials";

/// How much life is left on a cached token before this client bothers PayPal
/// for another one.
///
/// Sixty seconds is generous against PayPal's own lifetime — their documented
/// example is `expires_in: 31668`, nearly nine hours — and cheap: it costs at
/// most one token call per minute of idle time, which nothing here is.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// Where the client points and what it authenticates with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Box<str>,
    client_id: Secret,
    client_secret: Secret,
    timeout: Duration,
}

impl Config {
    /// PayPal's sandbox base.
    pub const SANDBOX: &'static str = "https://api-m.sandbox.paypal.com";
    /// PayPal's live base.
    pub const PRODUCTION: &'static str = "https://api-m.paypal.com";
    /// How long a request waits before it is given up on.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Points at PayPal's sandbox.
    #[must_use]
    pub fn sandbox(client_id: impl Into<Secret>, client_secret: impl Into<Secret>) -> Self {
        Self::at(Self::SANDBOX, client_id, client_secret)
            .unwrap_or_else(|_| unreachable!("the sandbox constant parses"))
    }

    /// Points at PayPal in production.
    #[must_use]
    pub fn production(client_id: impl Into<Secret>, client_secret: impl Into<Secret>) -> Self {
        Self::at(Self::PRODUCTION, client_id, client_secret)
            .unwrap_or_else(|_| unreachable!("the production constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    pub fn at(
        base_url: &str,
        client_id: impl Into<Secret>,
        client_secret: impl Into<Secret>,
    ) -> Result<Self, url::ParseError> {
        let trimmed = base_url.trim_end_matches('/');
        Url::parse(trimmed)?;
        Ok(Self {
            base_url: trimmed.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
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

/// A cached bearer token and when it stops being trusted.
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: Secret,
    obtained_at: SystemTime,
    expires_in: Duration,
}

impl CachedToken {
    fn expires_within(&self, margin: Duration) -> bool {
        let Some(expiry) = self.obtained_at.checked_add(self.expires_in) else {
            return true;
        };
        SystemTime::now()
            .checked_add(margin)
            .is_none_or(|deadline| deadline >= expiry)
    }
}

/// Takes payments through PayPal's Orders v2 API.
///
/// Cloning shares one connection pool and one cached token.
///
/// # Getting and renewing the token
///
/// PayPal authenticates with OAuth2 client-credentials: `client_id` and
/// `client_secret` buy a bearer token at `POST /v1/oauth2/token`, good for
/// hours rather than the Terminal API's two, and this client fetches and
/// caches it itself — a caller here never sees a token at all.
///
/// That is a different choice from `kasapay_iyzico::terminal`, which takes
/// a token from the caller and never renews it, and it is worth saying why
/// the two adapters in the same workspace disagree. Terminal's reason is that
/// renewing and silently replaying **the payment itself** risks putting a
/// physical terminal back into its card-reading state for a sale that may
/// already have been taken — an ambiguity about whether a real-world action
/// happened, which retrying cannot resolve. Getting a PayPal access token has
/// no real-world action in it at all: it is a bearer credential fetched
/// before any order is touched, and refreshing it early changes nothing about
/// whether `create_order` or `capture_order` runs once.
///
/// So this client refreshes **proactively**, before a stale token is ever
/// sent: a margin against the cached token's own expiry is checked ahead of
/// every call, not after one fails. What it does not do is retry
/// automatically. If PayPal still
/// answers `401` — a token revoked early, a clock skew this client's own
/// check did not catch — that comes back as [`ErrorKind::Auth`], the same as
/// Terminal's expired-token answer, and nothing here retries the call that
/// failed. Whether that retry is safe is a fact about the *business* call, not
/// about the token, and is exactly what [`ChargeRequest::idempotency_key`]
/// and [`PayPal::capture_order`]'s `request_id` are for. Reading whether the
/// call already worked, with [`Provider::charge_status`], is always safe.
#[derive(Debug, Clone)]
pub struct PayPal {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
    token: RwLock<Option<CachedToken>>,
}

impl PayPal {
    /// Builds a client with its own HTTP connection pool.
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self::with_http(http, config))
    }

    /// Builds a client over an HTTP client the caller already has.
    #[must_use]
    pub fn with_http(http: reqwest::Client, config: Config) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config,
                token: RwLock::new(None),
            }),
        }
    }

    /// Creates an order with `intent: CAPTURE`.
    ///
    /// [`Provider::charge`] is this call. This crate never creates one with
    /// `intent: AUTHORIZE` — see the crate docs for why — so every order this
    /// client makes still needs an explicit [`PayPal::capture_order`]
    /// afterwards regardless: PayPal's `CAPTURE` intent decides what a
    /// capture call does to the order, not whether one is still required.
    ///
    /// `ChargeRequest::customer` is not read: Orders v2 has no field for a
    /// payer's identity outside PayPal's Vault API, which is out of scope
    /// here. `ChargeRequest::metadata` is not sent either — PayPal's
    /// `purchase_unit` has no free-form key/value bag the way Stripe's
    /// `metadata` is; `ChargeRequest::order` goes in the one field that comes
    /// closest, `custom_id`, and a caller with more to attach has nowhere on
    /// this call to put it.
    ///
    /// `ChargeRequest::return_url`, when set, is sent as **both**
    /// `experience_context.return_url` and `.cancel_url` — the same
    /// simplification `kasapay_mollie::Mollie` makes with its one
    /// `redirectUrl`, because `ChargeRequest` has a field for where the payer
    /// goes back to and none for where they go back to on giving up, and
    /// sending a mismatched pair would be worse than sending the one URL
    /// twice. Either way the payer returns to the same address, and which it
    /// was is what [`Provider::charge_status`] is for. Left unset, PayPal
    /// falls back to whatever the merchant's own account is configured with.
    pub async fn create_order(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        self.create(request, "CAPTURE").await
    }

    /// Creates an order with `intent: AUTHORIZE`, for a later
    /// [`PayPal::authorize_order`] rather than an immediate
    /// [`PayPal::capture_order`].
    ///
    /// The same call as [`PayPal::create_order`] but for the intent that buys
    /// a longer hold through PayPal's Authorizations resource — see the
    /// crate docs for what changes and what does not. Everything
    /// [`PayPal::create_order`]'s own documentation says about
    /// `ChargeRequest::customer`, `::metadata` and `::return_url` applies
    /// here too: this differs from that call only in the one field PayPal
    /// reads to decide what a later capture does.
    ///
    /// **Still needs an explicit follow-up.** Creating the order does not
    /// place the hold; [`PayPal::authorize_order`] does, once the payer has
    /// approved it, the same two-step shape [`PayPal::create_order`] and
    /// [`PayPal::capture_order`] already have for `intent: CAPTURE`.
    pub async fn authorize(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        self.create(request, "AUTHORIZE").await
    }

    async fn create(&self, request: &ChargeRequest, intent: &'static str) -> Result<Charge, Error> {
        // Before a socket opens: a currency PayPal will not read.
        let amount = convert::amount_out(request.amount)?;

        let unit = wire::PurchaseUnitOut {
            custom_id: Some(request.order.as_str()),
            description: request.description.as_deref(),
            amount,
        };
        let payment_source = request
            .return_url
            .as_ref()
            .map(|url| wire::PaymentSourceOut {
                paypal: wire::PayPalWalletOut {
                    experience_context: wire::ExperienceContext {
                        return_url: url.as_str(),
                        cancel_url: url.as_str(),
                    },
                },
            });
        let body = wire::CreateOrder {
            intent,
            purchase_units: [unit],
            payment_source,
        };
        let raw = self
            .send(
                reqwest::Method::POST,
                "/v2/checkout/orders",
                Some(&body),
                true,
                request.idempotency_key.as_ref().map(IdempotencyKey::as_str),
            )
            .await?;
        let order: wire::Order = parse(&raw, "an order")?;
        into_charge(&order, raw, Some(request))
    }

    /// Places the hold an `intent: AUTHORIZE` order asks for, once the payer
    /// has approved it.
    ///
    /// `POST /v2/checkout/orders/{id}/authorize`, sent with
    /// `Prefer: return=representation` — every one of PayPal's own documented
    /// examples for this call shows the full authorization, id and
    /// `expiration_time` included, regardless of which `Prefer` value the
    /// request carried, so this asks for the shape it actually reads.
    ///
    /// The answer is a [`Charge`] built the same way [`PayPal::order`]'s is:
    /// [`Charge::id`] is the *order's* id, [`Charge::status`] is
    /// [`Status::Authorized`] once PayPal's own `CREATED` or `PENDING`
    /// authorization status is on it, and the authorization's own id —
    /// needed for [`PayPal::capture_authorization`] — is not a field on
    /// `Charge` at all. [`authorization_id`] reads it back out of
    /// [`Charge::raw`].
    ///
    /// **This is not how a `CAPTURE`-intent order is captured.** Calling this
    /// on one, or calling [`PayPal::capture_order`] on an `AUTHORIZE`-intent
    /// order, is PayPal's own `ACTION_DOES_NOT_MATCH_INTENT` — each intent
    /// has exactly one of the two follow-up calls that works.
    ///
    /// `request_id`, sent as `PayPal-Request-Id`, is the same idempotency
    /// story [`PayPal::capture_order`]'s own documentation has: PayPal
    /// documents this call as idempotent against a repeated key, and a
    /// caller retrying without one risks a second hold PayPal itself would
    /// not have placed.
    pub async fn authorize_order(
        &self,
        order: &PaymentId,
        request_id: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        let raw = self
            .send(
                reqwest::Method::POST,
                &format!("/v2/checkout/orders/{}/authorize", order.as_str()),
                Some(&wire::AuthorizeOrder::default()),
                true,
                request_id.map(IdempotencyKey::as_str),
            )
            .await?;
        let order: wire::Order = parse(&raw, "an order")?;
        into_charge(&order, raw, None)
    }

    /// Reads an order back.
    ///
    /// [`Provider::charge_status`] is this call. `GET` documents no `Prefer`
    /// header of its own — a read always answers the complete order.
    pub async fn order(&self, id: &PaymentId) -> Result<Charge, Error> {
        let raw = self
            .send(
                reqwest::Method::GET,
                &format!("/v2/checkout/orders/{}", id.as_str()),
                None::<&()>,
                false,
                None,
            )
            .await?;
        let order: wire::Order = parse(&raw, "an order")?;
        into_charge(&order, raw, None)
    }

    /// Takes the funds an approved order is holding.
    ///
    /// [`Provider::capture`] is this call with `request_id` fixed to `None`,
    /// which is the gap worth knowing about: PayPal takes a
    /// `PayPal-Request-Id` here exactly as it does on
    /// [`PayPal::create_order`], and replaying a capture without one **can
    /// capture twice** — but [`Provider::capture`]'s signature carries no
    /// idempotency key for the trait to send. This method is the one place a
    /// caller working directly against this crate can pass one; a caller
    /// going through [`Provider`] alone cannot make this call retry-safe at
    /// all.
    ///
    /// `amount` on [`Provider::capture`] is honoured only as `None`. PayPal's
    /// Orders-level capture takes no amount at all — it is documented with an
    /// empty request body — so [`Capabilities::partial_capture`] is `false`
    /// and a `Some` is refused before a socket opens rather than silently
    /// capturing the whole order when part of it was asked for.
    pub async fn capture_order(
        &self,
        order: &PaymentId,
        request_id: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        let raw = self
            .send(
                reqwest::Method::POST,
                &format!("/v2/checkout/orders/{}/capture", order.as_str()),
                Some(&wire::CaptureOrder::default()),
                true,
                request_id.map(IdempotencyKey::as_str),
            )
            .await?;
        let captured: wire::Order = parse(&raw, "an order")?;
        into_charge(&captured, raw, None)
    }

    /// Takes the funds an `intent: AUTHORIZE` hold is holding.
    ///
    /// `POST /v2/payments/authorizations/{id}/capture`, keyed by
    /// [`AuthorizationId`] rather than [`PaymentId`] — [`authorization_id`]
    /// is how a caller gets one, off the [`Charge`]
    /// [`PayPal::authorize_order`] answered. This is not
    /// [`Provider::capture`]: the trait method's signature takes a
    /// [`PaymentId`], and reaching this call from one would mean reading the
    /// order back first to find the hold's own id, a second request this
    /// method does not make on a caller's behalf. Going through the
    /// Authorizations resource at all is calling this directly.
    ///
    /// Sent with `Prefer: return=representation`, the header every one of
    /// PayPal's own documented examples for this call carries — the default,
    /// `return=minimal`, answers `id`, `status` and links alone, and this
    /// crate wants the amount too.
    ///
    /// `amount` of `None` captures the whole authorization, sent as `{}`,
    /// PayPal's own `authorizations_capture_empty_request` example. `Some`
    /// captures part of it — PayPal answers `MAX_CAPTURE_AMOUNT_EXCEEDED`
    /// for more than what remains.
    ///
    /// `request_id`, sent as `PayPal-Request-Id`. The same caution
    /// [`PayPal::capture_order`]'s own documentation gives: replaying this
    /// without one can capture the same authorization twice.
    pub async fn capture_authorization(
        &self,
        authorization: &AuthorizationId,
        amount: Option<Money>,
        request_id: Option<&IdempotencyKey>,
    ) -> Result<Capture, Error> {
        let amount = amount
            .map(|money| {
                money.require_positive().map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidRequest,
                        PAYPAL,
                        "a capture takes an amount above zero, or None for the lot",
                    )
                    .with_source(e)
                })?;
                convert::amount_out(money)
            })
            .transpose()?;
        let raw = self
            .send(
                reqwest::Method::POST,
                &format!(
                    "/v2/payments/authorizations/{}/capture",
                    authorization.as_str()
                ),
                Some(&wire::CreateAuthorizationCapture { amount }),
                true,
                request_id.map(IdempotencyKey::as_str),
            )
            .await?;
        let captured: wire::AuthorizationCapture = parse(&raw, "a capture")?;
        Capture::read(&captured, authorization.clone(), raw)
    }

    /// Gives money back off a capture — **not off the order, and not off an
    /// authorization**.
    ///
    /// `POST /v2/payments/captures/{id}/refund`, keyed by [`CaptureId`]:
    /// [`capture_id`] is how a caller gets one, off the [`Charge`]
    /// [`PayPal::capture_order`] or [`Provider::charge_status`] answered.
    /// This is the same split `kasapay_mollie::Mollie::refund` draws
    /// between a payment and the capture it was taken with, for the same
    /// underlying reason: the money moved when the capture happened, and a
    /// refund reverses that, not the order.
    ///
    /// `amount` of `None` sends `{}` — PayPal's own
    /// `captures_refund_empty_request` example — and their `refund_request`
    /// schema documents exactly what that refunds: `captured amount -
    /// previous refunds`, computed when the request is processed rather
    /// than fixed at the capture's original amount. So calling this with
    /// `None` a second time, on a capture a first partial refund already
    /// touched, refunds what is left rather than attempting the original
    /// total again. `Some` refunds a specific amount instead, and PayPal
    /// answers `REFUND_AMOUNT_EXCEEDED` once the sum of every refund on this
    /// capture would pass what was captured; its own answer's
    /// `seller_payable_breakdown.total_refunded_amount` carries that running
    /// total either way. What PayPal does not document is which of the two
    /// rules wins when a retried `None` refund's own idempotency key matches
    /// a request sent before some other refund landed — see the crate docs,
    /// "Unverified against a live account".
    ///
    /// Sent with `Prefer: return=representation`. Every one of PayPal's own
    /// documented examples for this call carries that header; none shows the
    /// minimal shape, so this crate does not guess at one.
    ///
    /// **Money leaving needs a key.** PayPal takes `PayPal-Request-Id` here
    /// exactly as it does on [`PayPal::capture_order`], and replaying a
    /// refund without one can give the money back twice — the same caution,
    /// with a sharper cost. `request_id` is that key.
    pub async fn refund(
        &self,
        capture: &CaptureId,
        amount: Option<Money>,
        request_id: Option<&IdempotencyKey>,
    ) -> Result<Refund, Error> {
        let amount = amount
            .map(|money| {
                money.require_positive().map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidRequest,
                        PAYPAL,
                        "a refund takes an amount above zero, or None for the lot",
                    )
                    .with_source(e)
                })?;
                convert::amount_out(money)
            })
            .transpose()?;
        let raw = self
            .send(
                reqwest::Method::POST,
                &format!("/v2/payments/captures/{}/refund", capture.as_str()),
                Some(&wire::CreateRefund { amount }),
                true,
                request_id.map(IdempotencyKey::as_str),
            )
            .await?;
        let refunded: wire::Refund = parse(&raw, "a refund")?;
        Refund::read(&refunded, capture.clone(), raw)
    }

    async fn token(&self) -> Result<Secret, Error> {
        {
            let cached = self
                .inner
                .token
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(token) = cached.as_ref()
                && !token.expires_within(TOKEN_REFRESH_MARGIN)
            {
                return Ok(token.access_token.clone());
            }
        }
        let fresh = self.fetch_token().await?;
        let access_token = fresh.access_token.clone();
        *self
            .inner
            .token
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Some(fresh);
        Ok(access_token)
    }

    async fn fetch_token(&self) -> Result<CachedToken, Error> {
        let url = format!("{}/v1/oauth2/token", self.inner.config.base_url);
        let response = self
            .inner
            .http
            .post(&url)
            .basic_auth(
                self.inner.config.client_id.expose(),
                Some(self.inner.config.client_secret.expose()),
            )
            .form(&wire::ClientCredentialsGrant {
                grant_type: CLIENT_CREDENTIALS,
            })
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        // Read the clock before the body, so a slow parse cannot make a token
        // look longer-lived than it is.
        let obtained_at = SystemTime::now();
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if !status.is_success() {
            return Err(token_refused(status, &text));
        }
        let parsed: wire::TokenResponse = serde_json::from_str(&text).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "the token answer was not the JSON PayPal documents",
            )
            .with_source(e)
        })?;
        let access_token = parsed
            .access_token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PAYPAL,
                    "the token answer carried no access_token",
                )
            })?;
        let expires_in = parsed.expires_in.map(Duration::from_secs).ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "the token answer carried no expires_in",
            )
        })?;
        Ok(CachedToken {
            access_token: Secret::new(access_token),
            obtained_at,
            expires_in,
        })
    }

    async fn send<B: serde::Serialize>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
        prefer_representation: bool,
        request_id: Option<&str>,
    ) -> Result<Raw, Error> {
        let token = self.token().await?;
        let url = format!("{}{path}", self.inner.config.base_url);
        let mut request = self
            .inner
            .http
            .request(method, &url)
            .bearer_auth(token.expose())
            .header("Accept", "application/json");
        if prefer_representation {
            request = request.header("Prefer", "return=representation");
        }
        if let Some(id) = request_id {
            request = request.header("PayPal-Request-Id", id);
        }
        if let Some(body) = body {
            // reqwest's `.header` appends rather than replaces, so setting
            // Content-Type here too would duplicate what `.json` already sends.
            request = request.json(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if status.is_success() {
            Ok(Raw::from_text(text))
        } else {
            Err(refused(status, &text))
        }
    }
}

/// Reads one of PayPal's objects out of a body that was already accepted.
fn parse<T: serde::de::DeserializeOwned>(raw: &Raw, what: &str) -> Result<T, Error> {
    serde_json::from_str(raw.as_str()).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("{what} was not the JSON PayPal documents"),
        )
        .with_source(e)
    })
}

/// An order, as PayPal sent it.
///
/// `fallback` is the request this order was just created from, when there is
/// one — see "Reading a freshly created order" below for why.
///
/// # Where the status comes from
///
/// **PayPal's own OpenAPI document never shows a top-level `status` on an
/// order, in any example this crate's three calls answer with** — not on
/// "Create Order - Minimal Request and Response", not on "Get Order - Show
/// Order Details", not on "Capture Order - PayPal Wallet as Payment Source",
/// even though `order`'s schema declares `status` and the `Prefer` header's
/// own prose says a minimal response includes it. So this reads a status from
/// four places, in order, and only fails if none of them says anything:
///
/// 1. The first entry in `purchase_units[0].payments.captures`, if there is
///    one — [`convert::capture_status`]. This is what
///    [`PayPal::capture_order`]'s own documented example carries.
/// 2. Failing that, the first entry in `purchase_units[0].payments.authorizations` —
///    [`convert::authorization_status`]. Never produced by this crate, but
///    readable on an order [`Provider::charge_status`] is asked about.
/// 3. Failing that, the top-level `status` PayPal's schema promises but its
///    examples never show — [`convert::order_status`].
/// 4. Failing *that* too, an `approve` or `payer-action` link with nothing
///    captured or authorised is read as [`Status::RequiresAction`]: this is
///    what [`PayPal::create_order`]'s own documented example carries, and it
///    is the last signal left once the other three are absent.
///
/// This has not been checked against a live order that a real sandbox
/// account answers — see the crate docs.
///
/// # Reading a freshly created order
///
/// PayPal's own "Create Order - Minimal Request and Response" example answers
/// `id` and `links` and **nothing else at all** — no `purchase_units`, so
/// nothing to read [`Charge::amount`] or [`Charge::order`] from, even with
/// `Prefer: return=representation` sent. Rather than invent a shape PayPal
/// does not document, [`PayPal::create_order`] hands its own request in as
/// `fallback`: an order that says nothing about its amount or reference is
/// read as the amount and reference this crate just asked PayPal to accept,
/// which is the one thing here that is not a guess about PayPal's answer —
/// it is what was actually sent. [`PayPal::order`] and
/// [`PayPal::capture_order`] pass no fallback, so a stripped-down answer to
/// either of those is [`ErrorKind::Malformed`] rather than silently
/// substituting a request that was not this one.
fn into_charge(
    order: &wire::Order,
    raw: Raw,
    fallback: Option<&ChargeRequest>,
) -> Result<Charge, Error> {
    let missing = |field: &str| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            format!("an order carried no {field}"),
        )
    };
    let id = order.id.as_deref().ok_or_else(|| missing("id"))?;
    let unit = order.purchase_units.first();

    let status = resolve_status(order, unit)?;
    let amount = match resolve_amount(unit) {
        Ok(amount) => amount,
        Err(err) => fallback.map(|request| request.amount).ok_or(err)?,
    };
    let order_ref = unit
        .and_then(|unit| unit.custom_id.as_deref())
        .map(OrderRef::new)
        .or_else(|| fallback.map(|request| OrderRef::new(request.order.as_str())));

    let next_action = if status.is_open() {
        order
            .links
            .iter()
            .find(|link| matches!(link.rel.as_deref(), Some("approve" | "payer-action")))
            .and_then(|link| link.href.as_deref())
            .map(|href| {
                Url::parse(href).map(|url| NextAction::Redirect {
                    url,
                    // PayPal wants nothing back but the payer arriving at the
                    // return_url; the order id on the charge is what
                    // Provider::charge_status reads next.
                    continuation: None,
                })
            })
            .transpose()
            .map_err(|e| {
                Error::new(
                    ErrorKind::Malformed,
                    PAYPAL,
                    "the approval address is not a URL",
                )
                .with_source(e)
            })?
    } else {
        None
    };

    Ok(Charge {
        id: Some(PaymentId::issued(id)),
        order: order_ref,
        amount,
        // purchase_units[].amount.breakdown sums to the same figure this
        // reads, rather than a second, different total the way iyzico's price
        // and PayTR's payment_amount are.
        order_amount: None,
        status,
        next_action,
        provider: PAYPAL,
        raw,
    })
}

fn resolve_status(order: &wire::Order, unit: Option<&wire::PurchaseUnit>) -> Result<Status, Error> {
    if let Some(payments) = unit.and_then(|unit| unit.payments.as_ref()) {
        if let Some(capture) = payments.captures.first() {
            let status = capture
                .status
                .as_deref()
                .ok_or_else(|| missing_status("a capture"))?;
            return Ok(convert::capture_status(status));
        }
        if let Some(authorization) = payments.authorizations.first() {
            let status = authorization
                .status
                .as_deref()
                .ok_or_else(|| missing_status("an authorization"))?;
            return Ok(convert::authorization_status(status));
        }
    }
    if let Some(status) = order.status.as_deref() {
        return Ok(convert::order_status(status));
    }
    if order
        .links
        .iter()
        .any(|link| matches!(link.rel.as_deref(), Some("approve" | "payer-action")))
    {
        return Ok(Status::RequiresAction);
    }
    Err(Error::new(
        ErrorKind::Malformed,
        PAYPAL,
        "an order carried no status, no capture, no authorization and no approval link to read one from",
    ))
}

fn missing_status(what: &str) -> Error {
    Error::new(
        ErrorKind::Malformed,
        PAYPAL,
        format!("{what} carried no status"),
    )
}

fn resolve_amount(unit: Option<&wire::PurchaseUnit>) -> Result<Money, Error> {
    let unit = unit.ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            "an order carried no purchase_units",
        )
    })?;
    if let Some(payments) = unit.payments.as_ref() {
        if let Some(amount) = payments.captures.first().and_then(|c| c.amount.as_ref()) {
            return convert::money(amount, "a capture");
        }
        if let Some(amount) = payments
            .authorizations
            .first()
            .and_then(|a| a.amount.as_ref())
        {
            return convert::money(amount, "an authorization");
        }
    }
    let amount = unit.amount.as_ref().ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PAYPAL,
            "a purchase unit carried no amount",
        )
    })?;
    convert::money(amount, "a purchase unit")
}

/// Reads the capture id off an order's first capture, for [`PayPal::refund`].
///
/// PayPal nests it at `purchase_units[0].payments.captures[0].id` on
/// whatever answered the [`Charge`] — [`PayPal::capture_order`],
/// [`Provider::charge_status`] on an order that was captured — rather than
/// carrying it on a field of [`Charge`] itself, the same reason this crate
/// already reads an order's amount out of the raw shape rather than a
/// dedicated one.
///
/// [`ErrorKind::Malformed`] for a [`Charge`] with no capture on it at all —
/// an order that has not been captured has nothing to refund.
pub fn capture_id(charge: &Charge) -> Result<CaptureId, Error> {
    charge
        .raw
        .text_at("/purchase_units/0/payments/captures/0/id")
        .map(CaptureId::issued)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "this order carries no captured payment to refund",
            )
        })
}

/// Reads the authorization id off an order's first authorization, for
/// [`PayPal::capture_authorization`].
///
/// PayPal nests it at `purchase_units[0].payments.authorizations[0].id` on
/// whatever answered the [`Charge`] — [`PayPal::authorize_order`],
/// [`Provider::charge_status`] on an order somebody else authorized — the
/// same gap [`capture_id`] closes for a capture.
///
/// [`ErrorKind::Malformed`] for a [`Charge`] with no authorization on it at
/// all — an order nobody has authorized has no hold to capture.
pub fn authorization_id(charge: &Charge) -> Result<AuthorizationId, Error> {
    charge
        .raw
        .text_at("/purchase_units/0/payments/authorizations/0/id")
        .map(AuthorizationId::issued)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                "this order carries no authorization to capture",
            )
        })
}

/// What PayPal's HTTP status means, from their standard `error` object.
fn refused(status: reqwest::StatusCode, text: &str) -> Error {
    let kind = match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Provider,
        // 422 — PayPal's UNPROCESSABLE_ENTITY — falls through to here too.
        _ => ErrorKind::InvalidRequest,
    };
    let body: Option<wire::ErrorBody> = serde_json::from_str(text).ok();
    let message = match &body {
        Some(body) => {
            let mut message = body
                .message
                .clone()
                .unwrap_or_else(|| format!("PayPal answered {status} with no message"));
            if let Some(detail) = body.details.first() {
                if let Some(description) = &detail.description {
                    message = format!("{message}: {description}");
                }
                if let Some(field) = &detail.field {
                    message = format!("{message} (field: {field})");
                }
            }
            if let Some(debug_id) = &body.debug_id {
                message = format!("{message} (debug_id: {debug_id})");
            }
            message
        }
        None => format!("HTTP {status}: {text}"),
    };
    let error = Error::new(kind, PAYPAL, message);
    match body.and_then(|b| b.name) {
        Some(name) => error.with_code(name),
        None => error,
    }
}

/// `/v1/oauth2/token` answers plain OAuth2 — `{"error": …, "error_description": …}` —
/// which is not PayPal's `error` object every other endpoint here answers with.
fn token_refused(status: reqwest::StatusCode, text: &str) -> Error {
    let body: Option<wire::OAuthError> = serde_json::from_str(text).ok();
    let message = body
        .as_ref()
        .and_then(|b| b.error_description.clone())
        .unwrap_or_else(|| format!("PayPal refused the token request (HTTP {status})"));
    // A 400 here is OAuth2's invalid_client/invalid_grant and their
    // neighbours: a wrong client id or secret. An authentication failure
    // however it is numbered.
    let error = Error::new(ErrorKind::Auth, PAYPAL, message);
    match body.and_then(|b| b.error) {
        Some(code) => error.with_code(code),
        None => error,
    }
}

fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PAYPAL, error.to_string())
}

/// One capture PayPal has taken off an authorization —
/// [`PayPal::capture_authorization`]'s own answer.
///
/// Its identifier is a [`CaptureId`], the same kind [`PayPal::refund`] takes
/// — not a [`PaymentId`], which names the order rather than this.
#[derive(Debug, Clone)]
pub struct Capture {
    /// PayPal's identifier for this capture.
    pub id: CaptureId,
    /// The authorization it was taken off.
    ///
    /// **Not read from PayPal's answer.** The capture object this is built
    /// from carries no authorization id and no order id anywhere in its
    /// body — only in `links[].href`, under `rel: "up"` — so this is the id
    /// the caller passed to [`PayPal::capture_authorization`], echoed back
    /// rather than parsed out of a link.
    pub authorization: AuthorizationId,
    /// How much was taken.
    pub amount: Money,
    /// Where the capture stands, mapped through the same `capture_status`
    /// enum PayPal's Orders-level capture answers with, unchanged between
    /// the two APIs.
    pub status: Status,
    /// PayPal's own answer, kept whole.
    pub raw: Raw,
}

impl Capture {
    fn read(
        capture: &wire::AuthorizationCapture,
        authorization: AuthorizationId,
        raw: Raw,
    ) -> Result<Self, Error> {
        let missing = |field: &str| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                format!("a capture carried no {field}"),
            )
        };
        Ok(Self {
            id: CaptureId::issued(capture.id.as_deref().ok_or_else(|| missing("id"))?),
            authorization,
            amount: convert::money(
                capture.amount.as_ref().ok_or_else(|| missing("amount"))?,
                "a capture",
            )?,
            status: convert::capture_status(
                capture.status.as_deref().ok_or_else(|| missing("status"))?,
            ),
            raw,
        })
    }
}

/// One refund PayPal has taken off a capture — [`PayPal::refund`]'s own
/// answer.
#[derive(Debug, Clone)]
pub struct Refund {
    /// PayPal's identifier for this refund.
    pub id: RefundId,
    /// The capture it was taken off.
    ///
    /// **Not read from PayPal's answer**, for the same reason
    /// [`Capture::authorization`] is not: the refund object carries no
    /// capture id of its own, only an `up` link. This is the id the caller
    /// passed to [`PayPal::refund`].
    pub capture: CaptureId,
    /// How much went back.
    pub amount: Money,
    /// Where the refund stands. PayPal's own documented examples for this
    /// call all answer [`RefundState::Completed`]; the other three are on
    /// their `refund_status` schema and read here rather than guessed at.
    pub state: RefundState,
    /// PayPal's own answer, kept whole.
    pub raw: Raw,
}

impl Refund {
    fn read(refund: &wire::Refund, capture: CaptureId, raw: Raw) -> Result<Self, Error> {
        let missing = |field: &str| {
            Error::new(
                ErrorKind::Malformed,
                PAYPAL,
                format!("a refund carried no {field}"),
            )
        };
        Ok(Self {
            id: RefundId::issued(refund.id.as_deref().ok_or_else(|| missing("id"))?),
            capture,
            amount: convert::money(
                refund.amount.as_ref().ok_or_else(|| missing("amount"))?,
                "a refund",
            )?,
            state: RefundState::from(refund.status.as_deref().ok_or_else(|| missing("status"))?),
            raw,
        })
    }
}

/// Where one of PayPal's refunds stands, from their `refund_status` schema.
///
/// Unlike a capture's own status, this names no fold-in of anything else —
/// PayPal's refund is its own resource and this is only ever read off one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefundState {
    /// Accepted and not yet sent.
    Pending,
    /// The money is back with the payer.
    Completed,
    /// It will not be sent.
    Failed,
    /// Withdrawn before it was sent.
    Cancelled,
    /// A state PayPal has started returning since this was written.
    Other(Box<str>),
}

impl From<&str> for RefundState {
    fn from(value: &str) -> Self {
        match value {
            "PENDING" => Self::Pending,
            "COMPLETED" => Self::Completed,
            "FAILED" => Self::Failed,
            "CANCELLED" => Self::Cancelled,
            other => Self::Other(other.into()),
        }
    }
}

impl fmt::Display for RefundState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Completed => f.write_str("completed"),
            Self::Failed => f.write_str("failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Other(state) => f.write_str(state),
        }
    }
}

#[async_trait::async_trait]
impl Provider for PayPal {
    fn id(&self) -> ProviderId {
        PAYPAL
    }

    /// Creates an order with `intent: CAPTURE`, through [`PayPal::create_order`].
    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        self.create_order(request).await
    }

    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        self.order(id).await
    }

    /// Takes the funds an approved order is holding, through
    /// [`PayPal::capture_order`] with no `PayPal-Request-Id` — see that
    /// method's own documentation for what that costs a caller going through
    /// [`Provider`] alone.
    ///
    /// `amount` of anything but `None` is refused before a socket opens:
    /// PayPal's Orders-level capture has no field for one.
    async fn capture(&self, id: &PaymentId, amount: Option<Money>) -> Result<Charge, Error> {
        if amount.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PAYPAL,
                "PayPal's order capture takes the whole order; there is no field for a smaller amount",
            ));
        }
        self.capture_order(id, None).await
    }

    /// Always refused. **`/v2/checkout/orders` itself has no cancel or void
    /// operation** — `POST` to create, `GET` to read, `PATCH` to edit,
    /// `POST .../confirm-payment-source`, `POST .../authorize` and
    /// `POST .../capture`, and nothing that withdraws an order by its own
    /// id. An order the payer never approves is simply left: PayPal's own
    /// prose says an unapproved order is eligible for deletion after it has
    /// aged, without naming a fixed duration in this crate's scope, and
    /// there is no call here to hurry that along.
    ///
    /// **This is still true with `intent: AUTHORIZE` in the picture.** The
    /// Authorizations resource this now reaches through
    /// [`PayPal::authorize_order`] and [`PayPal::capture_authorization`]
    /// does document `POST /v2/payments/authorizations/{id}/void`, which
    /// releases a hold PayPal will not otherwise release — but it is keyed
    /// by the authorization's own id, not the order's, and
    /// [`Provider::cancel`]'s signature takes only a [`PaymentId`]. This
    /// crate does not implement it: a caller who needs to release a hold
    /// without capturing it has no call here, the same honest gap this
    /// method already had for a plain `intent: CAPTURE` order.
    async fn cancel(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYPAL,
            "PayPal's Orders v2 API has no operation that cancels or voids an order by its own id",
        ))
    }

    /// Always refused. PayPal's saved-instrument story is its Vault API — a
    /// different, versioned resource this crate does not implement — so
    /// there is nothing here to list.
    async fn instruments(&self, _customer: &str) -> Result<Vec<Instrument>, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYPAL,
            "vaulting is PayPal's separate Vault API, which this crate does not implement",
        ))
    }

    /// Every PayPal order needs an explicit [`PayPal::capture_order`] or
    /// [`PayPal::authorize_order`]-then-[`PayPal::capture_authorization`]
    /// after the payer approves, regardless of intent, so `separate_capture`
    /// is `true` unconditionally — there is no PayPal equivalent of Mollie's
    /// automatic-capture payment or Stripe's succeeding PaymentIntent.
    ///
    /// `partial_capture` is `false`: [`Provider::capture`] is
    /// [`PayPal::capture_order`], the Orders-level call, which takes no
    /// amount at all. [`PayPal::capture_authorization`] does take one — this
    /// flag is about what the trait's own `capture` can do, and it is not
    /// wired to that call; see its own documentation for why.
    ///
    /// **`partial_refund` and `repeated_refund` are now `true`.** PayPal's
    /// own `capture_status` enum has named `PARTIALLY_REFUNDED` since before
    /// this crate existed — the API always supported both — and
    /// [`PayPal::refund`] is what closes the gap: its `amount` takes less
    /// than the full capture, and its own answer's
    /// `seller_payable_breakdown.total_refunded_amount` tracks the sum
    /// across as many calls as the capture has left to give back.
    ///
    /// `saved_instruments` is `false`: see [`Provider::instruments`].
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: true,
            partial_capture: false,
            partial_refund: true,
            repeated_refund: true,
            saved_instruments: false,
        }
    }
}
