//! The Mollie client.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Delivery, Error, ErrorKind, Event, EventId, EventKind,
    IdempotencyKey, Instrument, InstrumentId, Money, NextAction, OrderRef, PaymentId, Provider,
    ProviderId, Raw, RefundReason, RefundRequest, RefundStatus, Secret, Status, Webhook,
};
use url::Url;

use crate::MOLLIE;
use crate::convert;
use crate::id::{CaptureId, CustomerId, RefundId};
use crate::wire;

/// The order reference travels as payment metadata under this key.
///
/// Mollie has no field of its own for the merchant's order number — their
/// `orderId` belongs to the separate Orders API — so it goes in `metadata` and
/// comes back out here.
pub const ORDER_METADATA_KEY: &str = "kasapay_order";

/// Where the client points and what it authenticates with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Box<str>,
    api_key: Secret,
    timeout: Duration,
}

impl Config {
    /// Mollie's only base.
    ///
    /// There is no separate sandbox host. A key beginning `test_` puts every
    /// request it makes into test mode and a key beginning `live_` does not,
    /// so which mode a client is in is a property of the credential rather
    /// than of the address.
    pub const BASE: &'static str = "https://api.mollie.com";
    /// How long a request waits before it is given up on.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Points at Mollie with the given API key.
    #[must_use]
    pub fn new(api_key: Secret) -> Self {
        Self::at(Self::BASE, api_key).unwrap_or_else(|_| unreachable!("the base constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    pub fn at(base_url: &str, api_key: Secret) -> Result<Self, url::ParseError> {
        let trimmed = base_url.trim_end_matches('/');
        Url::parse(trimmed)?;
        Ok(Self {
            base_url: trimmed.into(),
            api_key,
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

/// Takes payments through Mollie's hosted checkout.
///
/// Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct Mollie {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
}

impl Mollie {
    /// Builds a client with its own HTTP connection pool.
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self::with_http(http, config))
    }

    /// Builds a client over an HTTP client the caller already has.
    #[must_use]
    pub fn with_http(http: reqwest::Client, config: Config) -> Self {
        Self {
            inner: Arc::new(Inner { http, config }),
        }
    }

    /// Opens a payment Mollie will **hold** rather than take, for a later
    /// [`capture`](Provider::capture).
    ///
    /// The same call as [`Provider::charge`] with `captureMode: manual` on it.
    /// It is a separate method because it has to be: Mollie decides at
    /// creation whether a payment is captured automatically, and
    /// [`ChargeRequest`] has no field that says so. A payment opened by
    /// `charge` is captured the moment the payer finishes, and asking to
    /// capture it afterwards is Mollie's own 422.
    ///
    /// Not every payment method holds funds — Mollie documents manual capture
    /// for cards and for a handful of pay-later methods — so a payer who picks
    /// another one on the checkout page is why this can answer a payment that
    /// ends up captured outright.
    pub async fn authorize(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        self.create(request, Some("manual"), None, None).await
    }

    /// Opens a payment that also establishes a mandate on
    /// [`ChargeRequest::customer`], for a later [`Mollie::charge_with_mandate`].
    ///
    /// Sent with `sequenceType: first`. Mollie's own documented example for
    /// this — a card payment linked to a customer — carries the new
    /// mandate's `mdt_…` on the payment already, while the payment itself is
    /// still `open`: the mandate exists in [`MandateStatus::Pending`] before
    /// the payer has done anything, and becomes
    /// [`MandateStatus::Valid`] once they finish. [`Mollie::mandate`] is what
    /// reads it back to find out which.
    ///
    /// This still redirects the payer, the same as [`Provider::charge`]. It
    /// is [`Mollie::charge_with_mandate`] whose answer carries no
    /// [`NextAction`].
    pub async fn charge_first(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        if request.customer.is_none() {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                MOLLIE,
                "a first payment establishes a mandate on a customer; set ChargeRequest::customer",
            ));
        }
        self.create(request, None, Some("first"), None).await
    }

    /// Charges a mandate Mollie already holds — this crate's saved
    /// instrument.
    ///
    /// `mandate` is the `mdt_…` [`Mollie::mandates`] or [`Mollie::mandate`]
    /// answered, as an [`InstrumentId`]. [`ChargeRequest::customer`] is the
    /// other half of the name — the same `cst_…` the mandate is listed
    /// under — and this refuses the request before a socket opens if it is
    /// missing, the same as a missing `description` or `return_url` does.
    ///
    /// Sent with `sequenceType: recurring` and `mandateId`. Mollie's schema
    /// describes `mandateId` as optional on a recurring payment — "the ID of
    /// a specific mandate can be supplied" — without saying what is charged
    /// when it is left off a customer with more than one. This call always
    /// sends the one the caller named rather than leaving that unstated
    /// choice to Mollie.
    ///
    /// **Answers no redirect.** Mollie's own documented example for a
    /// recurring payment carries `redirectUrl: null` and no `checkout` link
    /// in `_links` — only `changePaymentState`, which this crate does not
    /// read — so [`Charge::next_action`] is `None` here, where
    /// [`Provider::charge`] would carry a [`NextAction::Redirect`]. The
    /// payment can still be `pending`; it is just not waiting on the payer.
    ///
    /// **A mandate that is not [`MandateStatus::Valid`] is not checked for
    /// here.** Status read moments ago can be stale by the time this call
    /// reaches Mollie — a payer can revoke a mandate between the two — so
    /// Mollie's own answer is the one that has not gone stale. Charging a
    /// pending or invalid mandate is Mollie's ordinary refusal, not a fault
    /// in this call, and it arrives as [`ErrorKind::InvalidRequest`] like any
    /// other refused payment.
    ///
    /// [`ChargeRequest::return_url`] is still required, the same as every
    /// other call this crate opens a payment with. Mollie's own document
    /// says `redirectUrl` "can be omitted for recurring payments" — not that
    /// it is refused if sent — so this keeps the one validation `create`
    /// already does rather than carving out an exception for a field Mollie
    /// accepts either way.
    pub async fn charge_with_mandate(
        &self,
        request: &ChargeRequest,
        mandate: &InstrumentId,
    ) -> Result<Charge, Error> {
        if request.customer.is_none() {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                MOLLIE,
                "a recurring payment is charged against a customer's mandate; set ChargeRequest::customer",
            ));
        }
        self.create(request, None, Some("recurring"), Some(mandate.as_str()))
            .await
    }

    /// Registers a customer, so a mandate has somewhere to be hung.
    ///
    /// Both `name` and `email` are optional on Mollie's own schema; neither
    /// is checked here beyond that.
    pub async fn create_customer(
        &self,
        name: Option<&str>,
        email: Option<&str>,
    ) -> Result<Customer, Error> {
        let raw = self
            .send(
                reqwest::Method::POST,
                "/v2/customers",
                Some(&wire::CreateCustomer { name, email }),
                None,
            )
            .await?;
        let customer: wire::Customer = parse(&raw, "a customer")?;
        Customer::read(&customer, raw)
    }

    /// Every mandate on a customer.
    ///
    /// Mollie paginates this and this call does not: it answers the one page
    /// Mollie sends back, the same choice this crate makes for refunds. Each
    /// [`Mandate::raw`] is that mandate's own object out of the list, not the
    /// list body it arrived in.
    pub async fn mandates(&self, customer: &CustomerId) -> Result<Vec<Mandate>, Error> {
        let raw = self
            .send(
                reqwest::Method::GET,
                &format!("/v2/customers/{}/mandates", customer.as_str()),
                None::<&()>,
                None,
            )
            .await?;
        let value = raw.json().ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                "a list of mandates was not the JSON Mollie documents",
            )
        })?;
        let items = value
            .pointer("/_embedded/mandates")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    MOLLIE,
                    "a list of mandates carried no _embedded.mandates",
                )
            })?;
        items
            .iter()
            .map(|item| {
                let mandate: wire::Mandate = serde_json::from_value(item.clone()).map_err(|e| {
                    Error::new(
                        ErrorKind::Malformed,
                        MOLLIE,
                        "one mandate in the list was not the JSON Mollie documents",
                    )
                    .with_source(e)
                })?;
                Mandate::read(&mandate, Raw::from_json(item))
            })
            .collect()
    }

    /// Reads one mandate back by id.
    pub async fn mandate(
        &self,
        customer: &CustomerId,
        mandate: &InstrumentId,
    ) -> Result<Mandate, Error> {
        let raw = self
            .send(
                reqwest::Method::GET,
                &format!(
                    "/v2/customers/{}/mandates/{}",
                    customer.as_str(),
                    mandate.as_str()
                ),
                None::<&()>,
                None,
            )
            .await?;
        let mandate: wire::Mandate = parse(&raw, "a mandate")?;
        Mandate::read(&mandate, raw)
    }

    /// Takes funds an authorisation is holding, and answers Mollie's own
    /// capture object.
    ///
    /// [`Provider::capture`] is this call with the answer folded into a
    /// [`Charge`]; this one keeps the capture's own identifier, which is not a
    /// payment id and will not fit where one goes.
    ///
    /// `amount` of `None` captures the full authorised amount. `Some` captures
    /// part of it, and Mollie allows more than one capture against a payment
    /// up to what was authorised.
    ///
    /// `idempotency`, when given, is sent as Mollie's own `Idempotency-Key` on
    /// this endpoint the same as it is on opening a payment: a repeated key
    /// answers the cached first response for an hour rather than capturing
    /// again. Without one, replaying this call can take the funds twice.
    pub async fn capture_payment(
        &self,
        payment: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Capture, Error> {
        let amount = amount
            .map(|money| {
                money.require_positive().map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidRequest,
                        MOLLIE,
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
                &format!("/v2/payments/{}/captures", payment.as_str()),
                Some(&wire::CreateCapture { amount }),
                idempotency,
            )
            .await?;
        let capture: wire::Capture = parse(&raw, "a capture")?;
        Capture::read(&capture, raw)
    }

    /// Gives money back off a payment.
    ///
    /// Mollie requires the amount: there is no "refund the rest" request, so a
    /// caller that means a full refund sends what
    /// [`Charge::amount`](kasapay_core::Charge::amount) said. Partial refunds
    /// are allowed and may be repeated up to what was captured.
    ///
    /// A refund is not instant and Mollie says so in its own status — a fresh
    /// one is `queued` or `pending`, not `refunded`. [`Refund::state`] is that
    /// status; nothing on the payment's [`Status`] changes for a refund at
    /// all.
    ///
    /// **Retrying this without an idempotency key can refund twice.** Mollie
    /// names partial refunds as one of the three cases where a replayed
    /// request is harmful. There is no idempotency key on this signature
    /// because [`ChargeRequest`] is what carries one and a refund has no
    /// request object; read the refunds back before sending another.
    pub async fn refund(&self, payment: &PaymentId, amount: Money) -> Result<Refund, Error> {
        let request = RefundRequest::builder(payment.clone())
            .amount(amount)
            .build()
            .map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    MOLLIE,
                    "a refund takes an amount above zero",
                )
                .with_source(e)
            })?;
        self.create_refund(&request).await
    }

    /// The one request behind both refunds — this crate's own and
    /// [`Provider::refund`].
    async fn create_refund(&self, request: &RefundRequest) -> Result<Refund, Error> {
        let amount = match request.amount {
            Some(amount) => amount.require_positive().map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    MOLLIE,
                    "a refund takes an amount above zero",
                )
                .with_source(e)
            })?,
            None => self.amount_remaining(&request.payment).await?,
        };
        let description = match &request.reason {
            Some(RefundReason::Other(words)) => Some(&**words),
            _ => None,
        };
        let metadata = request
            .metadata
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let raw = self
            .send(
                reqwest::Method::POST,
                &format!("/v2/payments/{}/refunds", request.payment.as_str()),
                Some(&wire::CreateRefund {
                    amount: convert::amount_out(amount)?,
                    description,
                    metadata,
                }),
                request.idempotency_key.as_ref(),
            )
            .await?;
        let refund: wire::Refund = parse(&raw, "a refund")?;
        Refund::read(&refund, raw)
    }

    /// What is left to refund on a payment, from Mollie's own
    /// `amountRemaining`.
    ///
    /// The extra request [`Provider::refund`] makes when the caller names no
    /// amount. Mollie sends this field on a payment that has been paid; a
    /// payment carrying none is one there is nothing to work out a full refund
    /// from, and that is [`ErrorKind::InvalidRequest`] rather than a guess at
    /// the payment's own total — which would refund what has already gone
    /// back a second time.
    async fn amount_remaining(&self, payment: &PaymentId) -> Result<Money, Error> {
        let raw = self
            .send(
                reqwest::Method::GET,
                &format!("/v2/payments/{}", payment.as_str()),
                None::<&()>,
                None,
            )
            .await?;
        let read: wire::Payment = parse(&raw, "a payment")?;
        let remaining = read.amount_remaining.as_ref().ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidRequest,
                MOLLIE,
                "this payment carries no amountRemaining, so there is no figure to \
                 refund the rest of it with; name the amount",
            )
        })?;
        convert::money(remaining, "a payment's amountRemaining")
    }

    /// Releases the whole remaining hold on an authorised payment.
    ///
    /// This is what [`Provider::cancel`] would be if the trait could express
    /// it, and it cannot: **Mollie answers `202 Accepted` with no body at
    /// all**, so there is no charge to hand back and nothing that says the
    /// hold is gone. Mollie is explicit that it will try and that the issuing
    /// bank decides if and when the money is released.
    ///
    /// So `Ok(())` means Mollie accepted the request. Read the payment back
    /// with [`Provider::charge_status`] to learn what became of it: a released
    /// payment with no captures on it becomes `canceled`, and one with a
    /// successful capture becomes `paid`.
    ///
    /// [`Provider::cancel`] is Mollie's other call, `DELETE /v2/payments/{id}`,
    /// which withdraws a payment the payer has not finished. A payment that is
    /// already authorised is not cancelled that way, and Mollie answers 422 for
    /// it.
    pub async fn release_authorization(&self, payment: &PaymentId) -> Result<(), Error> {
        self.send(
            reqwest::Method::POST,
            &format!("/v2/payments/{}/release-authorization", payment.as_str()),
            None::<&()>,
            None,
        )
        .await
        .map(|_| ())
    }

    async fn create(
        &self,
        request: &ChargeRequest,
        capture_mode: Option<&'static str>,
        sequence_type: Option<&'static str>,
        mandate_id: Option<&str>,
    ) -> Result<Charge, Error> {
        // Before a socket opens: a currency Mollie does not take, sent, is a
        // payment in a currency nobody chose.
        let amount = convert::amount_out(request.amount)?;

        let required = |field: &str, also: &str| {
            Error::new(
                ErrorKind::InvalidRequest,
                MOLLIE,
                format!("Mollie requires {field} on every payment; set ChargeRequest::{also}"),
            )
        };
        let description = request
            .description
            .as_deref()
            .ok_or_else(|| required("a description", "description"))?;
        let redirect_url = request
            .return_url
            .as_ref()
            .ok_or_else(|| required("somewhere to send the payer back to", "return_url"))?;

        let mut metadata: BTreeMap<&str, &str> = request
            .metadata
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        // Last, so a caller's own key of this name cannot shadow the order.
        metadata.insert(ORDER_METADATA_KEY, request.order.as_str());

        let body = wire::CreatePayment {
            amount,
            description,
            redirect_url: redirect_url.as_str(),
            capture_mode,
            customer_id: request.customer.as_deref(),
            sequence_type,
            mandate_id,
            metadata,
        };
        let raw = self
            .send(
                reqwest::Method::POST,
                "/v2/payments",
                Some(&body),
                request.idempotency_key.as_ref(),
            )
            .await?;
        let payment: wire::Payment = parse(&raw, "a payment")?;
        into_charge(&payment, raw)
    }

    async fn send<B: serde::Serialize>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&B>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Raw, Error> {
        let url = format!("{}{path}", self.inner.config.base_url);
        let mut request = self
            .inner
            .http
            .request(method, &url)
            .header(
                "Authorization",
                format!("Bearer {}", self.inner.config.api_key.expose()),
            )
            .header("Accept", "application/hal+json");
        if let Some(key) = idempotency {
            request = request.header("Idempotency-Key", key.as_str());
        }
        if let Some(body) = body {
            let payload = serde_json::to_string(body).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest, MOLLIE, "request is not JSON").with_source(e)
            })?;
            request = request
                .header("Content-Type", "application/json")
                .body(payload);
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

/// Mollie's own refund states, in the shared words.
///
/// A state this build has not met reads as [`RefundStatus::Pending`], the same
/// way an unknown payment status reads as [`Status::Pending`]: it says the
/// refund may still change, so a caller asks again rather than writing it off.
fn refund_status(state: &RefundState) -> RefundStatus {
    match state {
        RefundState::Queued
        | RefundState::Pending
        | RefundState::Processing
        | RefundState::Other(_) => RefundStatus::Pending,
        RefundState::Refunded => RefundStatus::Succeeded,
        RefundState::Failed => RefundStatus::Failed,
        RefundState::Canceled => RefundStatus::Canceled,
    }
}

/// Reads one of Mollie's objects out of a body that was already accepted.
fn parse<T: serde::de::DeserializeOwned>(raw: &Raw, what: &str) -> Result<T, Error> {
    serde_json::from_str(raw.as_str()).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            MOLLIE,
            format!("{what} was not the JSON Mollie documents"),
        )
        .with_source(e)
    })
}

/// A payment, as Mollie sent it.
fn into_charge(payment: &wire::Payment, raw: Raw) -> Result<Charge, Error> {
    let missing = |field: &str| {
        Error::new(
            ErrorKind::Malformed,
            MOLLIE,
            format!("a payment carried no {field}"),
        )
    };
    let id = payment.id.as_deref().ok_or_else(|| missing("id"))?;
    let amount = convert::money(
        payment.amount.as_ref().ok_or_else(|| missing("amount"))?,
        "a payment",
    )?;
    let status = convert::status(payment.status.as_deref().ok_or_else(|| missing("status"))?);

    // Mollie drops `_links.checkout` once a payment is decided. Reading it
    // only while the payment can still move keeps a settled charge from
    // answering "send the payer somewhere" if it ever sends one anyway.
    let next_action = match payment.links.as_ref().and_then(|links| {
        links
            .checkout
            .as_ref()
            .and_then(|link| link.href.as_deref())
            .filter(|_| status.is_open())
    }) {
        Some(href) => Some(NextAction::Redirect {
            url: Url::parse(href).map_err(|e| {
                Error::new(
                    ErrorKind::Malformed,
                    MOLLIE,
                    "the checkout address is not a URL",
                )
                .with_source(e)
            })?,
            // Mollie wants nothing back. The payer returns to the merchant's
            // own `redirectUrl` carrying whatever the merchant put on it, and
            // the payment is read by its id.
            continuation: None,
        }),
        None => None,
    };

    Ok(Charge {
        id: Some(PaymentId::issued(id)),
        order: payment
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(ORDER_METADATA_KEY))
            .and_then(serde_json::Value::as_str)
            .map(OrderRef::new),
        amount,
        // Mollie's `amount` is what the payer is charged and what the order
        // came to at once: there is no instalment surcharge and no second
        // figure. `lines` is a basket, not a different total.
        order_amount: None,
        status,
        next_action,
        provider: MOLLIE,
        raw,
    })
}

/// What Mollie's HTTP status means.
///
/// Mollie's error object carries `status`, `title`, `detail` and sometimes
/// `field`, and **no machine-readable code of any kind** — so
/// [`Error::code`](kasapay_core::Error::code) is `None` for every failure this
/// crate reports, and the field that caused one is named in the message
/// instead.
fn refused(status: reqwest::StatusCode, text: &str) -> Error {
    let kind = match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        // 410 is Mollie's answer for an object that has been removed.
        404 | 410 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        // Including the 503 Mollie answers when a payment method's own
        // supplier is down, which is the one worth retrying.
        500..=599 => ErrorKind::Provider,
        _ => ErrorKind::InvalidRequest,
    };
    let body: Option<wire::ErrorBody> = serde_json::from_str(text).ok();
    let message = match body {
        Some(body) => {
            let detail = body
                .detail
                .or(body.title)
                .unwrap_or_else(|| format!("Mollie answered {status} with no detail"));
            match body.field {
                Some(field) => format!("{detail} (field: {field})"),
                None => detail,
            }
        }
        None => format!("HTTP {status}: {text}"),
    };
    Error::new(kind, MOLLIE, message)
}

fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, MOLLIE, error.to_string())
}

/// One capture Mollie has taken off an authorised payment.
///
/// Its identifier is a [`CaptureId`] rather than a
/// [`PaymentId`](kasapay_core::PaymentId): `cpt_…` and `tr_…` are both
/// Mollie's own strings and neither belongs in the other's call, so the
/// compiler is what keeps them apart.
#[derive(Debug, Clone)]
pub struct Capture {
    /// Mollie's identifier for this capture — `cpt_…`.
    pub id: CaptureId,
    /// The payment it was taken off.
    pub payment: PaymentId,
    /// How much was taken.
    pub amount: Money,
    /// Where the capture stands. A fresh one is usually
    /// [`CaptureState::Pending`].
    pub state: CaptureState,
    /// Mollie's own answer, kept whole.
    pub raw: Raw,
}

impl Capture {
    /// The capture as a [`Charge`], which is what [`Provider::capture`] hands back.
    ///
    /// **This is built from the capture, not from the payment.** Its
    /// [`amount`](Charge::amount) is what was captured rather than what was
    /// authorised, which is what the trait asks for, and its
    /// [`raw`](Charge::raw) is the capture object. Its
    /// [`status`](Charge::status) is the capture's, so a capture Mollie has
    /// accepted and not yet settled is [`Status::Pending`] even though the
    /// payment it belongs to still reads `authorized`.
    #[must_use]
    pub fn into_charge(self) -> Charge {
        Charge {
            id: Some(self.payment),
            // The capture carries none of the payment's metadata, so the order
            // reference is not on this answer. `charge_status` has it.
            order: None,
            amount: self.amount,
            order_amount: None,
            status: self.state.status(),
            next_action: None,
            provider: MOLLIE,
            raw: self.raw,
        }
    }

    fn read(capture: &wire::Capture, raw: Raw) -> Result<Self, Error> {
        let missing = |field: &str| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                format!("a capture carried no {field}"),
            )
        };
        Ok(Self {
            id: CaptureId::issued(capture.id.as_deref().ok_or_else(|| missing("id"))?),
            payment: PaymentId::issued(
                capture
                    .payment_id
                    .as_deref()
                    .ok_or_else(|| missing("paymentId"))?,
            ),
            amount: convert::money(
                capture.amount.as_ref().ok_or_else(|| missing("amount"))?,
                "a capture",
            )?,
            state: CaptureState::from(capture.status.as_deref().ok_or_else(|| missing("status"))?),
            raw,
        })
    }
}

/// Where one of Mollie's captures stands.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureState {
    /// Accepted and not settled.
    Pending,
    /// The money has been taken.
    Succeeded,
    /// It will not be taken.
    Failed,
    /// A state Mollie has started returning since this was written.
    Other(Box<str>),
}

impl CaptureState {
    /// How this reads as a payment's [`Status`].
    #[must_use]
    pub fn status(&self) -> Status {
        match self {
            Self::Succeeded => Status::Captured,
            Self::Failed => Status::Failed,
            Self::Pending | Self::Other(_) => Status::Pending,
        }
    }
}

impl From<&str> for CaptureState {
    fn from(value: &str) -> Self {
        match value {
            "pending" => Self::Pending,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            other => Self::Other(other.into()),
        }
    }
}

impl fmt::Display for CaptureState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Succeeded => f.write_str("succeeded"),
            Self::Failed => f.write_str("failed"),
            Self::Other(state) => f.write_str(state),
        }
    }
}

/// One refund Mollie has taken off a payment.
#[derive(Debug, Clone)]
pub struct Refund {
    /// Mollie's identifier for this refund — `re_…`.
    pub id: RefundId,
    /// The payment it came off.
    pub payment: PaymentId,
    /// How much went back.
    pub amount: Money,
    /// Where the refund stands. A fresh one is [`RefundState::Queued`] or
    /// [`RefundState::Pending`], never `Refunded`.
    pub state: RefundState,
    /// Mollie's own answer, kept whole.
    pub raw: Raw,
}

impl Refund {
    fn read(refund: &wire::Refund, raw: Raw) -> Result<Self, Error> {
        let missing = |field: &str| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                format!("a refund carried no {field}"),
            )
        };
        Ok(Self {
            id: RefundId::issued(refund.id.as_deref().ok_or_else(|| missing("id"))?),
            payment: PaymentId::issued(
                refund
                    .payment_id
                    .as_deref()
                    .ok_or_else(|| missing("paymentId"))?,
            ),
            amount: convert::money(
                refund.amount.as_ref().ok_or_else(|| missing("amount"))?,
                "a refund",
            )?,
            state: RefundState::from(refund.status.as_deref().ok_or_else(|| missing("status"))?),
            raw,
        })
    }
}

/// Where one of Mollie's refunds stands.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RefundState {
    /// Waiting for the balance to cover it.
    Queued,
    /// Accepted and not yet sent to the bank.
    Pending,
    /// On its way to the payer.
    Processing,
    /// The money is back with the payer.
    Refunded,
    /// It will not be sent.
    Failed,
    /// Withdrawn before it was sent.
    Canceled,
    /// A state Mollie has started returning since this was written.
    Other(Box<str>),
}

impl From<&str> for RefundState {
    fn from(value: &str) -> Self {
        match value {
            "queued" => Self::Queued,
            "pending" => Self::Pending,
            "processing" => Self::Processing,
            "refunded" => Self::Refunded,
            "failed" => Self::Failed,
            "canceled" => Self::Canceled,
            other => Self::Other(other.into()),
        }
    }
}

impl fmt::Display for RefundState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queued => f.write_str("queued"),
            Self::Pending => f.write_str("pending"),
            Self::Processing => f.write_str("processing"),
            Self::Refunded => f.write_str("refunded"),
            Self::Failed => f.write_str("failed"),
            Self::Canceled => f.write_str("canceled"),
            Self::Other(state) => f.write_str(state),
        }
    }
}

/// A customer at Mollie — enough to hang a mandate on.
///
/// [`Mollie::create_customer`] answers one. Nothing else in this crate reads
/// a customer back by id: [`ChargeRequest::customer`] carries the `cst_…`
/// wherever it is needed again, as it does for any other provider.
#[derive(Debug, Clone)]
pub struct Customer {
    /// Mollie's identifier for this customer — `cst_…`.
    pub id: CustomerId,
    /// The name it was given, if any.
    pub name: Option<Box<str>>,
    /// The email address it was given, if any.
    pub email: Option<Box<str>>,
    /// Mollie's own answer, kept whole.
    pub raw: Raw,
}

impl Customer {
    fn read(customer: &wire::Customer, raw: Raw) -> Result<Self, Error> {
        let id = customer
            .id
            .as_deref()
            .ok_or_else(|| Error::new(ErrorKind::Malformed, MOLLIE, "a customer carried no id"))?;
        Ok(Self {
            id: CustomerId::issued(id),
            name: customer.name.clone().map(String::into_boxed_str),
            email: customer.email.clone().map(String::into_boxed_str),
            raw,
        })
    }
}

/// Mollie's saved instrument: a mandate, held against a [`Customer`] and
/// charged with [`Mollie::charge_with_mandate`].
///
/// [`Mandate::id`] is an [`InstrumentId`] — the same kind Stripe's `pm_…` and
/// iyzico's `cardToken` are — and [`Mandate::customer`] is the other half of
/// the name: charging one sends the id here and the customer as
/// [`ChargeRequest::customer`], the same split iyzico's `cardToken` and
/// `cardUserKey` make.
#[derive(Debug, Clone)]
pub struct Mandate {
    /// Mollie's identifier for this mandate — `mdt_…`.
    pub id: InstrumentId,
    /// Which customer it is hung on.
    pub customer: CustomerId,
    /// Whether it can be charged.
    pub status: MandateStatus,
    /// `directdebit`, `creditcard` or `paypal` — Mollie's own spelling, kept
    /// as sent rather than modelled as an enum this crate has no other use
    /// for.
    pub method: Option<Box<str>>,
    /// Mollie's own answer, kept whole — this mandate's own object, whether
    /// it came from [`Mollie::mandate`] or out of the list
    /// [`Mollie::mandates`] answers.
    pub raw: Raw,
}

impl Mandate {
    fn read(mandate: &wire::Mandate, raw: Raw) -> Result<Self, Error> {
        let missing = |field: &str| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                format!("a mandate carried no {field}"),
            )
        };
        Ok(Self {
            id: InstrumentId::issued(mandate.id.as_deref().ok_or_else(|| missing("id"))?),
            customer: CustomerId::issued(
                mandate
                    .customer_id
                    .as_deref()
                    .ok_or_else(|| missing("customerId"))?,
            ),
            status: MandateStatus::from(
                mandate.status.as_deref().ok_or_else(|| missing("status"))?,
            ),
            method: mandate.method.clone().map(String::into_boxed_str),
            raw,
        })
    }
}

impl From<Mandate> for Instrument {
    /// `method` — `directdebit`, `creditcard`, `paypal` — is the only thing
    /// worth showing somebody: this crate models no IBAN, no card, nothing
    /// that would tell two direct-debit mandates apart. `None` where Mollie
    /// sent no `method` at all.
    fn from(mandate: Mandate) -> Self {
        Self {
            id: mandate.id,
            label: mandate.method,
            raw: mandate.raw,
        }
    }
}

/// Whether a mandate can be charged.
///
/// Mollie's own enum has three values and no fourth for "revoked": revoking a
/// mandate — not a call this crate makes — leaves it reading `invalid`
/// exactly like one that never signed, and there is nothing in the status
/// alone that tells the two apart. `mandateReference` and `signatureDate`
/// stay on the object either way, for a caller that keeps its own record of
/// which happened.
///
/// [`Mollie::charge_with_mandate`] does not read this before sending a
/// charge — see its own documentation for why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MandateStatus {
    /// Signed, and can be charged.
    Valid,
    /// The first payment establishing it has not yet finished.
    Pending,
    /// Cannot be charged — revoked, or never became valid. Mollie's own enum
    /// does not say which.
    Invalid,
    /// A status Mollie has started returning since this was written. Their
    /// `mandate-status` schema marks the response `x-speakeasy-unknown-values:
    /// allow`, which is their own way of saying the three documented values
    /// are not a promise never to add a fourth.
    Other(Box<str>),
}

impl From<&str> for MandateStatus {
    fn from(value: &str) -> Self {
        match value {
            "valid" => Self::Valid,
            "pending" => Self::Pending,
            "invalid" => Self::Invalid,
            other => Self::Other(other.into()),
        }
    }
}

impl fmt::Display for MandateStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Valid => f.write_str("valid"),
            Self::Pending => f.write_str("pending"),
            Self::Invalid => f.write_str("invalid"),
            Self::Other(state) => f.write_str(state),
        }
    }
}

#[async_trait::async_trait]
impl Provider for Mollie {
    fn id(&self) -> ProviderId {
        MOLLIE
    }

    /// Opens a payment and hands back where to send the payer.
    ///
    /// The answer is [`Status::RequiresAction`] with a
    /// [`NextAction::Redirect`] to Mollie's hosted checkout, whose
    /// `continuation` is `None` — Mollie wants no token back, only the payment
    /// id, which is on the charge.
    ///
    /// Two of [`ChargeRequest`]'s optional fields are not optional here.
    /// Mollie requires a `description` and a `redirectUrl`, and a request
    /// carrying neither is [`ErrorKind::InvalidRequest`] before a socket
    /// opens rather than a 422 from Mollie.
    ///
    /// `ChargeRequest::customer` is sent as Mollie's `customerId`, which is a
    /// `cst_…` they issued; anything else is their 422.
    ///
    /// The payment is captured automatically. [`Mollie::authorize`] is the one
    /// that holds the funds instead.
    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        self.create(request, None, None, None).await
    }

    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let raw = self
            .send(
                reqwest::Method::GET,
                &format!("/v2/payments/{}", id.as_str()),
                None::<&()>,
                None,
            )
            .await?;
        let payment: wire::Payment = parse(&raw, "a payment")?;
        into_charge(&payment, raw)
    }

    /// Takes funds an authorisation is holding.
    ///
    /// [`Mollie::capture_payment`] is the same call keeping Mollie's own
    /// capture object, and [`Capture::into_charge`] says what this answer is
    /// built from — the capture, not the payment.
    ///
    /// Mollie holds funds only for a payment opened with `captureMode: manual`,
    /// which is [`Mollie::authorize`] rather than [`Provider::charge`].
    ///
    /// `idempotency` is forwarded to [`Mollie::capture_payment`], which sends
    /// it as Mollie's own `Idempotency-Key` on the captures endpoint.
    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        self.capture_payment(id, amount, idempotency)
            .await
            .map(Capture::into_charge)
    }

    /// Withdraws a payment the payer has not finished.
    ///
    /// Mollie's `DELETE /v2/payments/{id}`, which answers the cancelled
    /// payment. Whether a payment can be cancelled at all depends on the
    /// method the payer chose, and Mollie says which on the payment's own
    /// `isCancelable` — readable from [`Charge::raw`].
    ///
    /// **This is not how an authorised payment is released.** That is
    /// [`Mollie::release_authorization`], and it is a different call because
    /// Mollie answers it with no body at all. Cancelling a payment that is
    /// already authorised, or already paid, is Mollie's 422 and arrives as
    /// [`ErrorKind::InvalidRequest`].
    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error> {
        let raw = self
            .send(
                reqwest::Method::DELETE,
                &format!("/v2/payments/{}", id.as_str()),
                None::<&()>,
                None,
            )
            .await?;
        let payment: wire::Payment = parse(&raw, "a payment")?;
        into_charge(&payment, raw)
    }

    /// Gives money back off a payment.
    ///
    /// # `amount: None` costs a second request
    ///
    /// Mollie has no "refund the rest" request: `POST /v2/payments/{id}/refunds`
    /// takes an amount and nothing else means anything. So a refund with no
    /// amount reads the payment first and asks for its `amountRemaining` —
    /// Mollie's own figure for what is still refundable, which is the payment
    /// less every refund already taken off it. Naming the amount is one call
    /// rather than two, and is what [`Mollie::refund`] does.
    ///
    /// A payment Mollie sends no `amountRemaining` for is
    /// [`ErrorKind::InvalidRequest`] rather than a full refund of the
    /// payment's own total: that total is what was paid, not what is left, and
    /// refunding it after a partial refund would give back money that has
    /// already gone.
    ///
    /// # The reason is shown to the payer
    ///
    /// Mollie's `description` on a refund is what the payer sees on their bank
    /// statement, so only [`RefundReason::Other`] reaches it — the caller's own
    /// sentence, written for a person. `Fraudulent` is not something to print
    /// on somebody's statement, and Mollie has no acquirer-facing field for it
    /// the way Stripe and iyzico do, so the three named reasons are not sent.
    ///
    /// [`RefundRequest::idempotency_key`] goes as Mollie's own
    /// `Idempotency-Key`. Mollie names a repeated partial refund as one of the
    /// three cases where replaying a request is harmful, so it is worth
    /// sending.
    async fn refund(&self, request: &RefundRequest) -> Result<kasapay_core::Refund, Error> {
        let refund = self.create_refund(request).await?;
        Ok(kasapay_core::Refund {
            id: Some(refund.id),
            payment: refund.payment,
            amount: refund.amount,
            status: refund_status(&refund.state),
            next_action: None,
            provider: MOLLIE,
            raw: refund.raw,
        })
    }

    /// Lists a customer's mandates, through [`Mollie::mandates`].
    ///
    /// `customer` is Mollie's `cst_…`.
    async fn instruments(&self, customer: &str) -> Result<Vec<Instrument>, Error> {
        Ok(self
            .mandates(&CustomerId::issued(customer))
            .await?
            .into_iter()
            .map(Instrument::from)
            .collect())
    }

    /// Mollie separates authorisation from capture, captures partially, and
    /// refunds partially and repeatedly.
    ///
    /// `separate_capture` is about Mollie rather than about a particular
    /// payment: it is true because Mollie will hold funds, and
    /// [`Mollie::authorize`] is what asks it to. A payment opened by
    /// [`Provider::charge`] is captured outright, and capturing it afterwards
    /// fails — the same way it does for a Stripe intent that already
    /// succeeded.
    ///
    /// **`saved_instruments` is true.** Mollie's version of a saved
    /// instrument is a mandate — `mdt_…`, held against a customer — and
    /// [`Mollie::charge_with_mandate`] sends `sequenceType: recurring` and
    /// the `mandateId` to charge one, with
    /// [`ChargeRequest::customer`](kasapay_core::ChargeRequest::customer)
    /// carrying the other half of the name the same way iyzico's
    /// `cardUserKey` does beside its `cardToken`. [`Mollie::charge_first`]
    /// is how one comes to exist in the first place.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: true,
            partial_capture: true,
            partial_refund: true,
            repeated_refund: true,
            saved_instruments: true,
        }
    }
}

/// What a composed event identifier is composed of.
///
/// Mollie names the delivery by nothing at all — the body is one field — so
/// the identifier is the payment it named and the status that payment was in
/// when it was read back.
const EVENT_NAMED_BY: &[&str] = &["id", "status"];

/// The one field Mollie posts to a webhook address.
#[derive(serde::Deserialize)]
struct WebhookBody {
    id: String,
}

/// Mollie signs nothing, so the payment is read back instead.
///
/// This is why [`Webhook::verify`] is `async`. Mollie's own documentation is
/// explicit that the delivery is not to be trusted and that the merchant is to
/// fetch the resource over their own authenticated connection — the body
/// carries one field, `id`, and no signature, no timestamp and no secret.
///
/// **The delivery is a hint; the answer is Mollie's.** Anybody who knows the
/// address can post an identifier to it, and all that can do is have one of
/// the merchant's own payments read back and reported truthfully. What cannot
/// happen is a payment being reported as paid because somebody said so.
#[async_trait::async_trait]
impl Webhook for Mollie {
    fn provider(&self) -> ProviderId {
        MOLLIE
    }

    /// Reads back whatever the delivery named, and reports what Mollie says
    /// about it.
    ///
    /// An identifier that is not a payment's is Mollie's own 404, which
    /// arrives as [`ErrorKind::NotFound`] — Mollie posts a payment id here
    /// even for a change to a refund of it, so there is nothing else this
    /// could sensibly fetch.
    ///
    /// A status with no word in [`EventKind`] — `open`, `pending` — is
    /// [`EventKind::Other`] carrying Mollie's own name for it. Those are real
    /// deliveries and answering an error for one earns a redelivery rather
    /// than a fix.
    async fn verify(&self, delivery: &Delivery<'_>) -> Result<Event, Error> {
        let body = delivery.body_str().ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                "a delivery whose body is not UTF-8",
            )
        })?;
        let posted: WebhookBody = serde_urlencoded::from_str(body).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                MOLLIE,
                "a delivery that is not the `id=…` form Mollie posts",
            )
            .with_source(e)
        })?;

        let payment = PaymentId::issued(posted.id);
        let charge = self.charge_status(&payment).await?;
        let status = charge
            .raw
            .text_at("/status")
            .unwrap_or_else(|| "unknown".to_owned());
        Ok(Event {
            id: EventId::derived(format!("{payment}:{status}"), EVENT_NAMED_BY),
            kind: match charge.status {
                Status::Captured => EventKind::Captured,
                Status::Authorized => EventKind::Authorized,
                Status::Failed => EventKind::Failed,
                Status::Canceled => EventKind::Canceled,
                // `open` and `pending` are deliveries about a payment that has
                // not decided yet, and there is no shared word for one.
                _ => EventKind::Other(status.into()),
            },
            payment: Some(payment),
            amount: Some(charge.amount),
            provider: MOLLIE,
            raw: charge.raw,
        })
    }
}
