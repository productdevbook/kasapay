//! The softpos client: `PayPOS`'s three payment-session services.

use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use kasapay_core::{Currency, Error, ErrorKind, Money, Raw, Secret};
use url::Url;

use crate::softpos::request::{InitReversal, InitSale};
use crate::softpos::{PROVIDER, transport_error, wire};

/// Where the client points, and how long it waits.
///
/// Same two addresses as [`crate::agent::Config`] — see its documentation for
/// why they are Paynet's own hosts and not iyzico's.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Url,
    timeout: Duration,
}

impl Config {
    /// How long a request waits before it is given up on.
    ///
    /// `PayPOS` returns as soon as it has produced a `deeplink_url` or an
    /// answer to a status query — nothing here waits on a payer the way a
    /// Terminal Host call does — so this uses the crate's ordinary default
    /// rather than [`crate::terminal::Config::DEFAULT_TIMEOUT`].
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// The production base, `https://api.paynet.com.tr`.
    pub const PRODUCTION: &'static str = crate::agent::Config::PRODUCTION;
    /// The sandbox base, `https://pts-api.paynet.com.tr`.
    pub const SANDBOX: &'static str = crate::agent::Config::SANDBOX;

    /// Points at the sandbox.
    #[must_use]
    pub fn sandbox() -> Self {
        Self::new(Self::SANDBOX).unwrap_or_else(|_| unreachable!("the sandbox constant parses"))
    }

    /// Points at production.
    #[must_use]
    pub fn production() -> Self {
        Self::new(Self::PRODUCTION)
            .unwrap_or_else(|_| unreachable!("the production constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    ///
    /// The base is joined against, so it must end in a slash.
    pub fn new(base_url: &str) -> Result<Self, url::ParseError> {
        Ok(Self {
            base_url: Url::parse(base_url)?,
            timeout: Self::DEFAULT_TIMEOUT,
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn endpoint(&self, path: &str) -> Result<Url, Error> {
        self.base_url.join(path).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PROVIDER, "endpoint is not a URL").with_source(e)
        })
    }
}

/// Starts, reverses and reads back a `PayPOS` softpos payment.
///
/// Cloning shares one connection pool and one session key.
///
/// # The session key is the caller's to keep current
///
/// This client never calls [`crate::agent::Client::get_auth_key`] and never
/// renews on its own — it sends the `Session-Key` it was given and, on a
/// refusal, reports [`ErrorKind::Auth`] rather than quietly fetching another
/// and retrying. [`Client::set_session_key`] is how a fresh one gets in
/// without rebuilding the connection pool, the same shape
/// [`crate::terminal::Client::set_access_token`] uses and for the same
/// reason: retrying a sale on this crate's own initiative, after a refusal
/// whose cause was never confirmed, is not a decision this crate gets to make
/// for a caller moving money.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
    session_key: RwLock<Secret>,
}

impl Client {
    /// Builds a client with its own HTTP connection pool.
    ///
    /// The session key comes from
    /// [`agent::Client::get_auth_key`](crate::agent::Client::get_auth_key):
    /// `Client::new(config, session.session_key.clone())`.
    pub fn new(config: Config, session_key: impl Into<Secret>) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self::with_http(http, config, session_key))
    }

    /// Builds a client over an HTTP client the caller already has.
    ///
    /// The caller's own timeout applies; [`Config::timeout`] is ignored here.
    #[must_use]
    pub fn with_http(
        http: reqwest::Client,
        config: Config,
        session_key: impl Into<Secret>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config,
                session_key: RwLock::new(session_key.into()),
            }),
        }
    }

    /// Puts a fresher session key in front of the same connection pool.
    ///
    /// Takes `&self`, so every clone of this client sees the new key. A
    /// request already in flight keeps the key it started with.
    pub fn set_session_key(&self, session_key: impl Into<Secret>) {
        let mut held = self
            .inner
            .session_key
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *held = session_key.into();
    }

    /// Starts a sale, and answers the deeplink the payer's phone opens.
    ///
    /// This only *starts* the flow: iyzico's own description says it "checks
    /// whether the required configurations have been completed" and returns
    /// where to send the payer, not whether they have paid.
    /// [`Client::check_transaction`] is what says that, once
    /// `payment_session_id` has been through the payer's device.
    pub async fn init_sale_transaction(&self, sale: &InitSale) -> Result<PaymentFlow, Error> {
        if sale.amount.currency() != Currency::Try {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                format!(
                    "PayPOS softpos is not documented in any currency but TRY, was asked for {}",
                    sale.amount.currency()
                ),
            ));
        }
        let body = wire::InitSaleRequest {
            amount: wire::amount(sale.amount)?,
            add_commission: sale.add_commission,
            instalment: sale.instalment,
            card_holder_phone: sale.card_holder_phone.as_deref(),
            card_holder_mail: sale.card_holder_mail.as_deref(),
            description: sale.description.as_deref(),
            reference_no: sale.reference_no.as_deref(),
            selected_agent_id: sale.selected_agent_id.as_deref(),
            callback_url: sale.callback_url.as_deref(),
        };
        let (status, bytes) = self.call("v1/softpos/init_sale_transaction", &body).await?;
        flow(status, &bytes, "iyzico refused to start the sale")
    }

    /// Starts a cancel or refund, and answers the deeplink the payer's phone opens.
    ///
    /// Like [`Client::init_sale_transaction`], this starts the flow rather
    /// than finishing it — the payer completes it on the same device, and
    /// [`Client::check_transaction`] is what confirms it happened.
    pub async fn init_reversal_transaction(
        &self,
        reversal: &InitReversal,
    ) -> Result<PaymentFlow, Error> {
        let xact_id = non_empty(&reversal.xact_id, "xact_id")?;
        let body = wire::InitReversalRequest {
            xact_id,
            reference_no: reversal.reference_no.as_deref(),
            callback_url: reversal.callback_url.as_deref(),
        };
        let (status, bytes) = self
            .call("v1/softpos/init_reversal_transaction", &body)
            .await?;
        flow(status, &bytes, "iyzico refused to start the reversal")
    }

    /// Reads back every transaction `PayPOS` recorded for a payment session.
    ///
    /// The one call here that moves no money. `Data` is documented as an
    /// array — `PayPOS` does not say when it holds more than one entry — so
    /// every one it sent is returned, oldest first as `PayPOS` wrote them.
    pub async fn check_transaction(
        &self,
        payment_session_id: &str,
    ) -> Result<Vec<Transaction>, Error> {
        let payment_session_id = non_empty(payment_session_id, "payment_session_id")?;
        let body = wire::CheckTransactionRequest { payment_session_id };
        let (status, bytes) = self.call("v1/softpos/check_transaction", &body).await?;
        if !status.is_success() {
            return Err(refused(status, &bytes, "iyzico refused the status inquiry"));
        }
        let response: wire::CheckTransactionResponse = parse(&bytes)?;
        response
            .data
            .unwrap_or_default()
            .iter()
            .map(|line| Transaction::read(line))
            .collect()
    }

    async fn call<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), Error> {
        let session_key = self
            .inner
            .session_key
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let request = self
            .inner
            .http
            .post(self.inner.config.endpoint(path)?)
            .header("Session-Key", session_key.expose())
            .json(body);
        let response = request
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        Ok((status, bytes.to_vec()))
    }
}

/// What `init_sale_transaction` and `init_reversal_transaction` both answer:
/// where to send the payer, and how to read what comes back from them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentFlow {
    /// Names this payment session — what [`Client::check_transaction`] asks
    /// about once the payer has been through the flow.
    pub payment_session_id: Option<Box<str>>,
    /// The deeplink the caller's mobile app opens to hand the payer off to
    /// the `PayPOS` app.
    pub deeplink_url: Option<Box<str>>,
    /// Decrypts what the `PayPOS` app returns to `callback_url`.
    pub encryption_key: Option<Box<str>>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// One transaction `PayPOS` recorded against a payment session.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Transaction {
    /// The encrypted transaction id — what [`InitReversal::xact_id`] wants.
    pub xact_id: Option<Box<str>>,
    /// When it happened, exactly as `PayPOS` wrote it.
    pub xact_date: Option<Box<str>>,
    /// `PayPOS`'s own numeric code for what kind of transaction this is.
    /// Undocumented beyond the field's existence.
    pub transaction_type: Option<i32>,
    /// `PayPOS`'s own numeric code for the POS type. Undocumented beyond that.
    pub pos_type: Option<i32>,
    /// The dealer this transaction belongs to.
    pub agent_id: Option<Box<str>>,
    /// Whether 3-D Secure applied.
    pub is_tds: Option<bool>,
    /// The acquiring bank's code.
    pub bank_id: Option<Box<str>>,
    /// How many instalments.
    pub instalment: Option<i32>,
    /// The card number, masked as `PayPOS` chose to send it.
    pub card_no: Option<Box<str>>,
    /// The name on the card.
    pub card_holder: Option<Box<str>>,
    /// What kind of card. `PayPOS` documents no enum for this field.
    pub card_type: Option<Box<str>>,
    /// The commission rate `PayPOS` applied.
    ///
    /// Kept as `PayPOS` wrote it rather than parsed into a number: it is a
    /// rate, not an amount, and nothing here claims to know its precision.
    pub ratio: Option<Box<str>>,
    /// What the transaction moved, read in [`Currency::Try`] — see
    /// [`crate::softpos::InitSale::new`] for why. `None` if `currency` was
    /// not `TRY` or `PayPOS`'s number could not be read as one; either way the
    /// figure is still in [`Transaction::raw`].
    pub amount: Option<Money>,
    /// What was left after `PayPOS`'s commission, same currency handling as
    /// [`Transaction::amount`].
    pub net_amount: Option<Money>,
    /// `PayPOS`'s commission, same currency handling as [`Transaction::amount`].
    pub commission_amount: Option<Money>,
    /// Tax on `PayPOS`'s commission, same currency handling as
    /// [`Transaction::amount`].
    pub commission_tax: Option<Money>,
    /// The bank's approval code.
    pub authorization_code: Option<Box<str>>,
    /// `PayPOS`'s own reference code for the transaction.
    pub reference_code: Option<Box<str>>,
    /// The caller's own order number, echoed back.
    pub order_id: Option<Box<str>>,
    /// Whether the transaction succeeded. The one field this module treats
    /// as the actual verdict on a `check_transaction` line.
    pub is_succeed: Option<bool>,
    /// `PayPOS`'s own transaction id, distinct from [`Transaction::xact_id`].
    pub xact_transaction_id: Option<Box<str>>,
    /// The payer's email, where `init_sale_transaction` was given one.
    pub email: Option<Box<str>>,
    /// The payer's phone, where `init_sale_transaction` was given one.
    pub phone: Option<Box<str>>,
    /// Free text carried on the transaction.
    pub note: Option<Box<str>>,
    /// The dealer's own reference code.
    pub agent_reference: Option<Box<str>>,
    /// iyzico's own answer for this line, untouched.
    pub raw: Raw,
}

impl Transaction {
    /// Reads one transaction out of the bytes `PayPOS` sent for it.
    fn read(value: &serde_json::value::RawValue) -> Result<Self, Error> {
        let wire: wire::TransactionWire = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a softpos transaction was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        let currency = wire
            .currency
            .as_deref()
            .and_then(|code| code.parse::<Currency>().ok());
        let amount = currency.and_then(|c| wire::money(wire.amount.as_deref(), c));
        let net_amount = currency.and_then(|c| wire::money(wire.net_amount.as_deref(), c));
        let commission_amount = currency.and_then(|c| wire::money(wire.comission.as_deref(), c));
        let commission_tax = currency.and_then(|c| wire::money(wire.comission_tax.as_deref(), c));
        let ratio = wire.ratio.as_deref().map(wire::text).map(Into::into);
        Ok(Self {
            amount,
            net_amount,
            commission_amount,
            commission_tax,
            ratio,
            raw: Raw::from_text(value.get()),
            xact_id: wire.xact_id.map(String::into_boxed_str),
            xact_date: wire.xact_date.map(String::into_boxed_str),
            transaction_type: wire.transaction_type,
            pos_type: wire.pos_type,
            agent_id: wire.agent_id.map(String::into_boxed_str),
            is_tds: wire.is_tds,
            bank_id: wire.bank_id.map(String::into_boxed_str),
            instalment: wire.instalment,
            card_no: wire.card_no.map(String::into_boxed_str),
            card_holder: wire.card_holder.map(String::into_boxed_str),
            card_type: wire.card_type.map(String::into_boxed_str),
            authorization_code: wire.authorization_code.map(String::into_boxed_str),
            reference_code: wire.reference_code.map(String::into_boxed_str),
            order_id: wire.order_id.map(String::into_boxed_str),
            is_succeed: wire.is_succeed,
            xact_transaction_id: wire.xact_transaction_id.map(String::into_boxed_str),
            email: wire.email.map(String::into_boxed_str),
            phone: wire.phone.map(String::into_boxed_str),
            note: wire.note.map(String::into_boxed_str),
            agent_reference: wire.agent_reference.map(String::into_boxed_str),
        })
    }
}

fn flow(status: reqwest::StatusCode, bytes: &[u8], fallback: &str) -> Result<PaymentFlow, Error> {
    if !status.is_success() {
        return Err(refused(status, bytes, fallback));
    }
    let response: wire::FlowResponse = parse(bytes)?;
    Ok(PaymentFlow {
        payment_session_id: response.payment_session_id.map(String::into_boxed_str),
        deeplink_url: response.deeplink_url.map(String::into_boxed_str),
        encryption_key: response.encryption_key.map(String::into_boxed_str),
        raw: raw(bytes),
    })
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "the answer was not the JSON iyzico documents",
        )
        .with_source(e)
    })
}

fn raw(bytes: &[u8]) -> Raw {
    Raw::from_text(String::from_utf8_lossy(bytes).into_owned())
}

/// Reads a refusal off whatever body came back, or from the status alone.
///
/// No error-code registry applies here either — see
/// [`crate::agent`]'s equivalent function for why.
fn refused(status: reqwest::StatusCode, body: &[u8], fallback: &str) -> Error {
    let parsed: Option<wire::ErrorResponse> = serde_json::from_slice(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|e| e.message.clone())
        .unwrap_or_else(|| format!("{fallback} (HTTP {status})"));
    let error = Error::new(kind_for_status(status), PROVIDER, message);
    match parsed.and_then(|e| e.code) {
        Some(code) => error.with_code(code.to_string()),
        None => error,
    }
}

fn kind_for_status(status: reqwest::StatusCode) -> ErrorKind {
    match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    }
}

/// Refuses a blank identifier before it opens a socket.
fn non_empty<'a>(value: &'a str, field: &'static str) -> Result<&'a str, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            format!("PayPOS requires {field}, and none was given"),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{Config, kind_for_status, non_empty};
    use kasapay_core::ErrorKind;

    #[test]
    fn the_paynet_bases_match_agents_and_join() {
        assert_eq!(Config::PRODUCTION, crate::agent::Config::PRODUCTION);
        assert_eq!(Config::SANDBOX, crate::agent::Config::SANDBOX);
        for config in [Config::sandbox(), Config::production()] {
            let url = config
                .endpoint("v1/softpos/init_sale_transaction")
                .expect("the path joins");
            assert_eq!(url.path(), "/v1/softpos/init_sale_transaction");
        }
    }

    #[test]
    fn a_blank_identifier_is_refused() {
        assert!(non_empty("", "payment_session_id").is_err());
        assert!(non_empty("   ", "xact_id").is_err());
        assert!(non_empty("ps-1", "payment_session_id").is_ok());
    }

    #[test]
    fn a_refusal_with_no_body_is_read_from_its_status() {
        let kind = |code| kind_for_status(reqwest::StatusCode::from_u16(code).expect("a status"));
        assert_eq!(kind(401), ErrorKind::Auth);
        assert_eq!(kind(404), ErrorKind::NotFound);
        assert_eq!(kind(400), ErrorKind::InvalidRequest);
        assert_eq!(kind(500), ErrorKind::Provider);
    }
}
