//! The Stripe client and its [`Provider`] implementation.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Error, ErrorKind, Instrument, InstrumentId, Money,
    NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, RefundId, RefundReason,
    RefundRequest, RefundStatus, Secret,
};
use stripe::{IdempotencyKey, RequestStrategy, StripeRequest};
use stripe_client_core::{RequestBuilder, StripeMethod};
use stripe_core::customer::{ListPaymentMethodsCustomer, ListPaymentMethodsCustomerType};
use stripe_core::payment_intent::{
    CancelPaymentIntent, CapturePaymentIntent, CreatePaymentIntent, CreatePaymentIntentOffSession,
    RetrievePaymentIntent,
};
use stripe_core::refund::{CreateRefund, CreateRefundReason, ListRefund};

use crate::convert;
use crate::saved;

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

/// Stripe's largest page. Its default is ten, which most payments fit in.
const REFUND_PAGE_SIZE: i64 = 100;

/// Stripe's largest page, for [`Stripe::stored_cards`].
const STORED_CARDS_PAGE_SIZE: i64 = 100;

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

    /// Builds a client pointed at somewhere other than Stripe.
    ///
    /// A mock server in a test, or a proxy that logs. Every other adapter in
    /// this workspace takes a base URL, and this is Stripe's — without it,
    /// pointing the client anywhere means depending on `async-stripe`
    /// directly and going through [`Stripe::with_client`].
    ///
    /// # Errors
    ///
    /// [`ErrorKind::InvalidRequest`](kasapay_core::ErrorKind::InvalidRequest)
    /// if `async-stripe` will not take the base.
    pub fn at(base_url: &str, secret_key: &Secret) -> Result<Self, Error> {
        // async-stripe builds `{base}v1{path}`, so the base has to end in a
        // slash or every request loses its version segment.
        let base = format!("{}/", base_url.trim_end_matches('/'));
        let client = stripe::ClientBuilder::new(secret_key.expose())
            .url(base)
            .build()
            .map_err(|e| convert::error(&e))?;
        Ok(Self::with_client(client))
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
        let mut request = RefundRequest::builder(payment.clone());
        if let Some(amount) = amount {
            request = request.amount(amount);
        }
        let request = request.build().map_err(|e| {
            Error::new(
                ErrorKind::InvalidRequest,
                convert::PROVIDER,
                "a refund takes an amount above zero, or None for the lot",
            )
            .with_source(e)
        })?;
        self.create_refund(&request).await
    }

    /// The one request behind both refunds — this crate's own and
    /// [`Provider::refund`].
    async fn create_refund(&self, request: &RefundRequest) -> Result<Refund, Error> {
        let mut create = CreateRefund::new().payment_intent(request.payment.as_str().to_owned());
        if let Some(amount) = request.amount {
            create = create.amount(amount.minor_units());
        }
        if let Some(reason) = request.reason.as_ref().and_then(refund_reason) {
            create = create.reason(reason);
        }
        let metadata = refund_metadata(request);
        if !metadata.is_empty() {
            create = create.metadata(metadata);
        }
        let refund = match &request.idempotency_key {
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

        let refund = into_refund(refund, &request.payment)?;
        let refunded = refund.amount;
        if let Some(asked) = request.amount
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

        Ok(refund)
    }

    /// Lists every refund taken off a payment, newest first.
    ///
    /// This is how "has all of it been given back" is answered: kasapay has no
    /// refunded status — Stripe leaves a refunded PaymentIntent `succeeded`
    /// and models the refunds as objects beside it — so the question is
    /// [`Money::checked_add`] over this list against the charge.
    ///
    /// # It is more than one request
    ///
    /// Stripe pages this endpoint and answers ten at a time by default. Asking
    /// for a page and stopping would silently undercount a payment refunded
    /// more often than that, so this walks the cursor to the end and the
    /// [`DEFAULT_TIMEOUT`] bounds each page rather than the whole walk. A
    /// payment with hundreds of refunds costs a round trip per hundred.
    pub async fn refunds(&self, payment: &PaymentId) -> Result<Vec<Refund>, Error> {
        let mut refunds = Vec::new();
        let mut cursor: Option<String> = None;
        // Every cursor already followed. `has_more` and a cursor that leads
        // back somewhere this walk has been is a loop that never ends and
        // never says so — and this call is how "how much has gone back" is
        // answered, so hanging is worse than answering short.
        let mut followed: HashSet<String> = HashSet::new();
        loop {
            let mut list = ListRefund::new()
                .payment_intent(payment.as_str().to_owned())
                .limit(REFUND_PAGE_SIZE);
            if let Some(after) = cursor.take() {
                list = list.starting_after(after);
            }
            let page = list
                .customize()
                .timeout(DEFAULT_TIMEOUT)
                .send(self.inner.as_ref())
                .await
                .map_err(|e| convert::error(&e).with_source(e))?;

            cursor = page.data.last().map(|last| last.id.as_str().to_owned());
            for refund in page.data {
                refunds.push(into_refund(refund, payment)?);
            }
            // An empty page with `has_more` set would otherwise loop forever.
            if !page.has_more || cursor.is_none() {
                return Ok(refunds);
            }
            if !followed.insert(cursor.clone().unwrap_or_default()) {
                return Ok(refunds);
            }
        }
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

    /// Lists a customer's saved cards.
    ///
    /// `GET /v1/customers/{customer}/payment_methods`, filtered to
    /// `type=card`: this crate charges cards, so a customer's bank-debit or
    /// wallet payment methods are left out rather than answered as something
    /// [`saved::StoredCard`] cannot represent. No card number goes either way
    /// — the request carries the customer's id and the answer carries a
    /// `pm_…`, the scheme, and the last four digits.
    ///
    /// # It is more than one request
    ///
    /// Stripe pages this endpoint the same way it pages refunds — see
    /// [`Stripe::refunds`] for why this walks the cursor rather than
    /// answering one page.
    pub async fn stored_cards(&self, customer: &str) -> Result<Vec<saved::StoredCard>, Error> {
        let mut cards = Vec::new();
        let mut cursor: Option<String> = None;
        // The same guard [`Stripe::refunds`] carries, for the same reason.
        let mut followed: HashSet<String> = HashSet::new();
        loop {
            let mut list = ListPaymentMethodsCustomer::new(customer.to_owned())
                .type_(ListPaymentMethodsCustomerType::Card)
                .limit(STORED_CARDS_PAGE_SIZE);
            if let Some(after) = cursor.take() {
                list = list.starting_after(after);
            }
            let page = list
                .customize()
                .timeout(DEFAULT_TIMEOUT)
                .send(self.inner.as_ref())
                .await
                .map_err(|e| convert::error(&e).with_source(e))?;

            cursor = page.data.last().map(|last| last.id.as_str().to_owned());
            for method in page.data {
                cards.push(saved::StoredCard::try_from(method)?);
            }
            // An empty page with `has_more` set would otherwise loop forever.
            if !page.has_more || cursor.is_none() {
                return Ok(cards);
            }
            if !followed.insert(cursor.clone().unwrap_or_default()) {
                return Ok(cards);
            }
        }
    }

    /// Charges a card Stripe already holds, sending no card number.
    ///
    /// A PaymentIntent with `customer` and `payment_method` set and
    /// `confirm: true`, so this both creates and confirms in the one call —
    /// see [`saved::Payment`] and the module's own doc for what
    /// [`saved::PaymentBuilder::off_session`] changes about authentication and
    /// who is liable if none happens.
    ///
    /// A currency Stripe cannot settle in is refused before a socket opens,
    /// the same way [`Provider::charge`] refuses one.
    pub async fn charge_saved_card(&self, payment: &saved::Payment) -> Result<Charge, Error> {
        let mut create = CreatePaymentIntent::new(
            payment.amount.minor_units(),
            convert::currency(payment.amount.currency())?,
        )
        .customer(payment.customer.to_string())
        .payment_method(payment.instrument.as_str().to_owned())
        .confirm(true)
        .metadata(saved_metadata(payment));
        if payment.off_session {
            create = create.off_session(CreatePaymentIntentOffSession::Bool(true));
        }
        if let Some(description) = &payment.description {
            create = create.description(description.to_string());
        }

        let intent = match &payment.idempotency_key {
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

    /// Forgets a saved card.
    ///
    /// `POST /v1/payment_methods/{id}/detach`. Stripe hands back the detached
    /// `PaymentMethod`; this discards it; a caller wanting it can confirm the
    /// card is gone by reading [`Stripe::stored_cards`] again.
    ///
    /// # Not generated
    ///
    /// `detach` lives in `async-stripe-payment`, a resource crate this
    /// workspace does not otherwise need — everything else here comes from
    /// `stripe_core`. Pulling it in for one call would add thousands of lines
    /// of every other payment-method type this crate never touches, so this
    /// is instead a `StripeRequest` written by hand against
    /// `async-stripe-client-core`, which every one of those generated
    /// requests is built on and which this crate already carries
    /// transitively through `async-stripe`. The wire shape — `POST
    /// /payment_methods/{id}/detach`, no body, a `PaymentMethod` back — is
    /// [documented by Stripe](https://docs.stripe.com/api/payment_methods/detach)
    /// and matches what `async-stripe-payment` itself sends.
    pub async fn forget_card(&self, instrument: &InstrumentId) -> Result<(), Error> {
        let request = DetachPaymentMethod {
            payment_method: instrument.as_str().into(),
        };
        request
            .customize()
            .timeout(DEFAULT_TIMEOUT)
            .send(self.inner.as_ref())
            .await
            .map_err(|e| convert::error(&e).with_source(e))?;
        Ok(())
    }

    /// The underlying `async-stripe` client, for calls kasapay does not model.
    #[must_use]
    pub fn client(&self) -> &stripe::Client {
        &self.inner
    }
}

/// `POST /payment_methods/{id}/detach`, written by hand — see
/// [`Stripe::forget_card`] for why.
struct DetachPaymentMethod {
    payment_method: Box<str>,
}

impl StripeRequest for DetachPaymentMethod {
    type Output = stripe_shared::PaymentMethod;

    fn build(&self) -> RequestBuilder {
        RequestBuilder::new(
            StripeMethod::Post,
            format!("/payment_methods/{}/detach", self.payment_method),
        )
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

    /// Always [`ErrorKind::Unsupported`]: Stripe names a PaymentIntent as it
    /// creates one, so there is no token to resume from.
    ///
    /// [`Provider::charge_status`] on the [`Charge::id`] the intent came back
    /// with is what finishes a Stripe redirect.
    async fn resume(&self, _continuation: &str) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            "Stripe names a PaymentIntent when it opens one; read it back with \
             Provider::charge_status",
        ))
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

    /// Sends `idempotency` as Stripe's own `Idempotency-Key`, the same way
    /// [`Provider::charge`] does — see [`ErrorKind::is_retryable`] for what a
    /// timeout means with and without one.
    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&kasapay_core::IdempotencyKey>,
    ) -> Result<Charge, Error> {
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
        let intent = match idempotency {
            Some(key) => {
                capture
                    .customize()
                    .request_strategy(RequestStrategy::Idempotent(idempotency_key(key)?))
                    .timeout(DEFAULT_TIMEOUT)
                    .send(self.inner.as_ref())
                    .await
            }
            None => {
                capture
                    .customize()
                    .timeout(DEFAULT_TIMEOUT)
                    .send(self.inner.as_ref())
                    .await
            }
        }
        .map_err(|e| convert::error(&e).with_source(e))?;
        into_charge(&intent)
    }

    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error> {
        Stripe::cancel(self, id).await
    }

    /// Gives money back off a payment, through [`Stripe::refund`]'s own
    /// request.
    ///
    /// `amount: None` is one call: Stripe refunds what is left of the payment
    /// without being told the figure, and answers how much that was.
    ///
    /// [`RefundRequest::idempotency_key`] is sent as Stripe's own
    /// `Idempotency-Key`, which is what makes replaying a refund safe — read
    /// [`ErrorKind::is_retryable`] for what a timeout means without one.
    ///
    /// # The reason, and where the other one goes
    ///
    /// Stripe takes three: `duplicate`, `fraudulent` and
    /// `requested_by_customer`, which are exactly
    /// [`RefundReason`]'s three named ones. `fraudulent` is not a label — it
    /// adds the card and the email to the account's Radar block lists — so it
    /// is passed through rather than folded into anything.
    ///
    /// [`RefundReason::Other`] has no field on Stripe's refund, and dropping
    /// the caller's sentence would lose the only record of why the money went
    /// back. It goes into the refund's metadata under
    /// [`REFUND_REASON_METADATA_KEY`] instead, beside whatever
    /// [`RefundRequest::metadata`] carries.
    async fn refund(&self, request: &RefundRequest) -> Result<kasapay_core::Refund, Error> {
        let refund = self.create_refund(request).await?;
        Ok(kasapay_core::Refund {
            id: Some(RefundId::issued(refund.id)),
            payment: refund.payment,
            amount: refund.amount,
            status: refund_status(&refund.status),
            next_action: None,
            provider: convert::PROVIDER,
            raw: refund.raw,
        })
    }

    /// Always [`ErrorKind::Unsupported`], and the alternative is better.
    ///
    /// The only call that finds a PaymentIntent by its metadata is
    /// `POST /v1/payment_intents/search`, and Stripe documents it as
    /// eventually consistent — *"don't use search in read-after-write flows
    /// where strict consistency is necessary"*, searchable "in less than a
    /// minute" and up to an hour behind during an outage. A charge that timed
    /// out thirty seconds ago is exactly a read-after-write flow, and a search
    /// answering "nothing" for one that exists is how a caller charges twice.
    ///
    /// What to do instead: send the charge again with the same
    /// [`ChargeRequest::idempotency_key`](kasapay_core::ChargeRequest::idempotency_key).
    /// Stripe answers the original PaymentIntent rather than opening a second
    /// one, which is the guarantee this method would otherwise be working
    /// around.
    async fn lookup(&self, _order: &OrderRef) -> Result<Option<Charge>, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            convert::PROVIDER,
            "Stripe finds a payment by metadata only through its search API, which it \
             documents as too far behind to answer this; retry the charge with the same \
             idempotency key instead",
        ))
    }

    /// Lists a customer's saved cards, through [`Stripe::stored_cards`].
    async fn instruments(&self, customer: &str) -> Result<Vec<Instrument>, Error> {
        Ok(self
            .stored_cards(customer)
            .await?
            .into_iter()
            .map(instrument_from_stored_card)
            .collect())
    }

    /// Separate capture, partial capture and repeated partial refunds, all as
    /// Stripe documents them for a PaymentIntent — and a card Stripe holds can
    /// be charged: [`Stripe::stored_cards`] lists them and
    /// [`Stripe::charge_saved_card`] charges one.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: true,
            partial_capture: true,
            partial_refund: true,
            repeated_refund: true,
            lookup_by_order: false,
            resume_by_continuation: false,
            saved_instruments: true,
        }
    }
}

/// [`RefundReason::Other`]'s own words travel as refund metadata under this
/// key — Stripe's `reason` takes three values and none of them is free text.
pub const REFUND_REASON_METADATA_KEY: &str = "kasapay_refund_reason";

/// The three reasons Stripe names, and `None` for the one it does not.
fn refund_reason(reason: &RefundReason) -> Option<CreateRefundReason> {
    match reason {
        RefundReason::Duplicate => Some(CreateRefundReason::Duplicate),
        RefundReason::Fraudulent => Some(CreateRefundReason::Fraudulent),
        RefundReason::RequestedByCustomer => Some(CreateRefundReason::RequestedByCustomer),
        // `Unknown` is what `async-stripe` deserializes a value it has not met
        // into, and it says itself that it is not for sending.
        RefundReason::Other(_) => None,
    }
}

fn refund_metadata(request: &RefundRequest) -> std::collections::HashMap<String, String> {
    let mut pairs: std::collections::HashMap<String, String> = request
        .metadata
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Some(RefundReason::Other(words)) = &request.reason {
        pairs.insert(REFUND_REASON_METADATA_KEY.to_owned(), words.to_string());
    }
    pairs
}

/// Stripe's own refund states, in the shared words.
///
/// A state this build has not met reads as [`RefundStatus::Pending`], the same
/// way an unknown PaymentIntent status does: it says the refund may still
/// change, so a caller asks again rather than writing it off.
fn refund_status(state: &RefundState) -> RefundStatus {
    match state {
        RefundState::Pending | RefundState::Other(_) => RefundStatus::Pending,
        RefundState::RequiresAction => RefundStatus::RequiresAction,
        RefundState::Succeeded => RefundStatus::Succeeded,
        RefundState::Failed => RefundStatus::Failed,
        RefundState::Canceled => RefundStatus::Canceled,
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

fn saved_metadata(payment: &saved::Payment) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([(
        ORDER_METADATA_KEY.to_owned(),
        payment.order.as_str().to_owned(),
    )])
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
        id: Some(PaymentId::issued(intent.id.as_str())),
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

fn into_refund(refund: stripe_shared::Refund, payment: &PaymentId) -> Result<Refund, Error> {
    Ok(Refund {
        id: refund.id.as_str().into(),
        payment: payment.clone(),
        amount: convert::amount(refund.amount, &refund.currency)?,
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

/// Turns Stripe's own [`saved::StoredCard`] into the shape
/// [`Provider::instruments`] answers.
///
/// The card's brand and last four go into the label; everything the crate
/// modelled about it goes into [`Instrument::raw`](kasapay_core::Instrument::raw)
/// — a reconstruction rather than the body Stripe sent, the same reason
/// `into_charge`'s `RawIntent` is one: `async-stripe` hands back a typed
/// `PaymentMethod` with the original bytes already gone.
fn instrument_from_stored_card(card: saved::StoredCard) -> Instrument {
    let raw = Raw::from_json(&serde_json::json!({
        "id": card.token.as_str(),
        "brand": card.brand.to_string(),
        "last4": &*card.last_four,
        "funding": card.funding.to_string(),
        "exp_month": card.exp_month,
        "exp_year": card.exp_year,
        "country": card.country.as_deref(),
    }));
    let label = Some(format!("{} •••• {}", card.brand, card.last_four).into());
    Instrument {
        id: card.token,
        label,
        raw,
    }
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
