//! The Terminal Host client, and what its four operations answer.

use std::fmt;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use kasapay_core::{Currency, Error, ErrorKind, Money, Raw, Secret};
use url::Url;

use crate::terminal::request::{Locale, Query, Refund, Sale, Void};
use crate::terminal::{PROVIDER, errors, transport_error, wire};

/// The only word iyzico documents for a transaction that worked.
const SUCCESS: &str = "SUCCESS";

/// Where the client points, how long it waits, and what language it reads in.
///
/// One type for [`Login`](crate::terminal::Login) and [`Client`] both: they
/// speak to the same host and differ only in what they authenticate with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Url,
    timeout: Duration,
    locale: Locale,
    timestamps: Timestamps,
}

impl Config {
    /// How long a request waits before it is given up on.
    ///
    /// Ninety seconds, where the rest of this crate waits thirty. A Terminal
    /// Host call is not a machine talking to a machine: `POST /payment`
    /// returns when a person has presented a card and, if the bank asks for
    /// one, typed a PIN. iyzico has its own timeout on that wait and reports
    /// it as `TIMEOUT_ERROR`, which is a far better answer than a socket
    /// closing under it.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(90);

    /// The production base, `https://api.iyzipay.com`.
    pub const PRODUCTION: &'static str = "https://api.iyzipay.com/";
    /// The sandbox base, `https://sandbox-api.iyzipay.com`.
    pub const SANDBOX: &'static str = "https://sandbox-api.iyzipay.com/";

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
    /// The base is joined against, so it must end in a slash. Unlike the
    /// In-Store client's, it carries no path: the Terminal API's own
    /// operations sit under `/v2/terminal-host` and its login under
    /// `/in-store/oauth2`, which no one prefix covers.
    pub fn new(base_url: &str) -> Result<Self, url::ParseError> {
        Ok(Self {
            base_url: Url::parse(base_url)?,
            timeout: Self::DEFAULT_TIMEOUT,
            locale: Locale::Turkish,
            timestamps: Timestamps::Seconds,
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Changes what language iyzico answers in, from [`Locale::Turkish`].
    #[must_use]
    pub const fn locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// Changes what unit `request_timestamp` is sent in, from
    /// [`Timestamps::Seconds`].
    ///
    /// The switch for one of the two things #96 could not settle without a
    /// till. Read [`Timestamps`] before flipping it: it exists so that being
    /// wrong is a line of configuration rather than a fork of this crate.
    #[must_use]
    pub const fn timestamps(mut self, timestamps: Timestamps) -> Self {
        self.timestamps = timestamps;
        self
    }

    pub(super) const fn request_timeout(&self) -> Duration {
        self.timeout
    }

    pub(super) const fn timestamp_unit(&self) -> Timestamps {
        self.timestamps
    }

    pub(super) fn endpoint(&self, path: &str) -> Result<Url, Error> {
        self.base_url.join(path).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PROVIDER, "endpoint is not a URL").with_source(e)
        })
    }
}

/// What unit `request_timestamp` is sent in.
///
/// iyzico asks for "the Unix timestamp value of the relevant request" and says
/// nothing more, and their own classic API writes its own `systemTime` in
/// milliseconds — so the two readings are both defensible and only one can be
/// right. There is no Terminal API sandbox without a merchant agreement and a
/// Pavo device, so nobody here has been able to ask.
///
/// This exists because being wrong about it would not be loud. A rejected
/// timestamp comes back as a login that failed, which reads like bad
/// credentials; a caller with a real till can flip this in one line instead of
/// forking the crate to find out. See #96.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Timestamps {
    /// Seconds since the epoch, which is what a Unix timestamp is. The default.
    #[default]
    Seconds,
    /// Milliseconds, which is what iyzico's classic API writes `systemTime` in.
    Milliseconds,
}

/// Drives a POS terminal through iyzico's VUK 509 Terminal Host services.
///
/// Cloning shares one connection pool and one token.
///
/// # The token is the caller's to keep current
///
/// This client never logs in and never refreshes on its own. It sends the
/// token it was given, and reports an expired one as
/// [`ErrorKind::Auth`] — iyzico's `100311`, "Access token expired!" — rather
/// than quietly getting another and trying again. See the module docs for why
/// that is not a convenience worth having on a payment endpoint.
///
/// [`Client::set_access_token`] is how a new one gets in without rebuilding
/// the connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
    access_token: RwLock<Secret>,
}

impl Client {
    /// Builds a client with its own HTTP connection pool.
    ///
    /// The token comes from [`Login::log_in`](crate::terminal::Login::log_in):
    /// `Client::new(config, token.access_token.clone())`.
    pub fn new(config: Config, access_token: impl Into<Secret>) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout())
            .build()?;
        Ok(Self::with_http(http, config, access_token))
    }

    /// Builds a client over an HTTP client the caller already has.
    ///
    /// The caller's own timeout applies; [`Config::timeout`] is ignored here.
    #[must_use]
    pub fn with_http(
        http: reqwest::Client,
        config: Config,
        access_token: impl Into<Secret>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config,
                access_token: RwLock::new(access_token.into()),
            }),
        }
    }

    /// Puts a fresher token in front of the same connection pool.
    ///
    /// Takes `&self`, so every clone of this client sees the new token — a
    /// till holding one and a background task renewing it are the same client.
    /// A request already in flight keeps the token it started with.
    pub fn set_access_token(&self, access_token: impl Into<Secret>) {
        let mut held = self
            .inner
            .access_token
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        *held = access_token.into();
    }

    /// Starts a payment on the terminal and waits for the card.
    ///
    /// This does not return until the payer has finished at the device or
    /// iyzico gives up waiting — see [`Config::DEFAULT_TIMEOUT`]. An answer of
    /// [`ErrorKind::Provider`] means nobody knows whether the card was
    /// charged, and [`Client::payment`] is what settles it.
    pub async fn pay(&self, sale: &Sale) -> Result<Payment, Error> {
        let body = wire::PaymentRequest {
            conversation_id: &sale.reference.conversation_id,
            locale: self.locale(),
            device_unique_id: &sale.reference.device_unique_id,
            transaction_reference_id: &sale.reference.transaction_reference_id,
            price: decimal_number(sale.amount)?,
            currency: sale.amount.currency().code(),
            sales_type: sale.sales_type.as_str(),
            payment_id: sale.payment_id.as_deref(),
            installment: sale.installments,
        };
        self.call("v2/terminal-host/payment", &body).await
    }

    /// Reads a transaction back.
    ///
    /// The one call here that moves no money and is always safe to make. After
    /// any failure that is not a plain refusal, this is what says whether the
    /// payment happened.
    pub async fn payment(&self, query: &Query) -> Result<Payment, Error> {
        let (payment_id, device_unique_id, transaction_reference_id) = match query {
            Query::Payment { payment_id, .. } => (Some(&**payment_id), None, None),
            Query::Transaction {
                device_unique_id,
                transaction_reference_id,
                ..
            } => (
                None,
                Some(&**device_unique_id),
                Some(&**transaction_reference_id),
            ),
            Query::PaymentAndTransaction {
                payment_id,
                transaction_reference_id,
                ..
            } => (Some(&**payment_id), None, Some(&**transaction_reference_id)),
        };
        let body = wire::QueryRequest {
            conversation_id: query.conversation_id(),
            locale: self.locale(),
            payment_id,
            device_unique_id,
            transaction_reference_id,
        };
        self.call("v2/terminal-host/payment/query-transaction-status", &body)
            .await
    }

    /// Gives money back, in whole or in part.
    ///
    /// Like a payment, the payer finishes it at the terminal by presenting the
    /// card. iyzico documents no limit on how many refunds one payment takes.
    pub async fn refund(&self, refund: &Refund) -> Result<Payment, Error> {
        let body = wire::RefundRequest {
            conversation_id: &refund.reference.conversation_id,
            locale: self.locale(),
            payment_id: &refund.payment_id,
            device_unique_id: &refund.reference.device_unique_id,
            price: decimal_number(refund.amount)?,
            transaction_reference_id: &refund.reference.transaction_reference_id,
            payment_date: &refund.payment_date,
            reason: refund.reason.as_deref(),
            description: refund.description.as_deref(),
        };
        self.call("v2/terminal-host/payment/refund", &body).await
    }

    /// Withdraws a payment that has not been settled yet.
    ///
    /// Only before the day's batch closes; after that the way back is
    /// [`Client::refund`]. The payer presents the card for this one too.
    pub async fn void(&self, void: &Void) -> Result<Payment, Error> {
        let body = wire::VoidRequest {
            conversation_id: &void.reference.conversation_id,
            locale: self.locale(),
            payment_id: &void.payment_id,
            payment_date: &void.payment_date,
            device_unique_id: &void.reference.device_unique_id,
            transaction_reference_id: &void.reference.transaction_reference_id,
            reason: void.reason.as_deref(),
            description: void.description.as_deref(),
        };
        self.call("v2/terminal-host/payment/void", &body).await
    }

    /// Closes the day's batch on one device.
    ///
    /// `POST /v2/terminal-host/eod`. A till does this once a day and the bank
    /// expects it: until the batch is closed the day's sales are authorised
    /// and not settled, and a device that never runs this is a shop wondering
    /// where its money is.
    ///
    /// `summary_on_slip` is iyzico's `useSummary`: `true` prints every
    /// transaction on the slip the device produces, `false` prints the totals
    /// alone. It changes what comes out of the printer rather than what this
    /// call answers.
    ///
    /// The answer is one [`EndOfDay`] carrying the batch number and a total
    /// per acquiring bank — a device can be set up with more than one, and
    /// each closes its own batch.
    ///
    /// # Not checked against a live till
    ///
    /// Like everything else in this module. See the module documentation for
    /// what that means and what is most likely to be wrong.
    pub async fn end_of_day(&self, reference: &EndOfDayRequest) -> Result<EndOfDay, Error> {
        let body = wire::EndOfDayRequest {
            conversation_id: &reference.conversation_id,
            locale: self.locale(),
            device_unique_id: &reference.device_unique_id,
            use_summary: reference.summary_on_slip,
        };
        let (response, raw) = self
            .post::<wire::EndOfDayResponse>("v2/terminal-host/eod", &body)
            .await?;
        Ok(EndOfDay::read(response, raw))
    }

    pub(crate) fn locale(&self) -> &'static str {
        self.inner.config.locale.as_str()
    }

    async fn call<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<Payment, Error> {
        let (body, raw) = self.post::<wire::PaymentResponse>(path, body).await?;
        Payment::read(body, raw)
    }

    /// Every Terminal Host call: the bearer token, the JSON, and iyzico's own
    /// failure envelope read the same way whatever the operation answered.
    pub(crate) async fn post<R: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<(R, Raw), Error> {
        let token = self
            .inner
            .access_token
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let response = self
            .inner
            .http
            .post(self.inner.config.endpoint(path)?)
            .bearer_auth(token.expose())
            .json(body)
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let raw = Raw::from_text(String::from_utf8_lossy(&bytes).into_owned());

        // The failure envelope is the same shape for every operation, and it
        // is read before the operation's own answer: a 400 carrying totals is
        // not a batch that closed.
        let failure: Option<wire::PaymentResponse> = serde_json::from_slice(&bytes).ok();
        if !status.is_success() {
            return Err(match failure {
                Some(body) => refused(body),
                None => Error::new(
                    kind_for_status(status),
                    PROVIDER,
                    format!("iyzico refused the request (HTTP {status})"),
                ),
            });
        }
        // iyzico answers 200 with `status: FAILURE` as well, which is a
        // refusal wearing a success's clothes — and an answer with no status
        // at all has not been shown to have worked either. `Payment::read`
        // makes the same two checks for the operations that answer one; this
        // is where the ones that answer something else make them.
        match failure {
            Some(body)
                if !body
                    .status
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(SUCCESS)) =>
            {
                return Err(match body.status {
                    Some(_) => refused(body),
                    None => Error::new(
                        ErrorKind::Malformed,
                        PROVIDER,
                        "a Terminal Host answer carried no status",
                    ),
                });
            }
            _ => {}
        }
        let parsed: R = serde_json::from_slice(&bytes).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "the answer was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        Ok((parsed, raw))
    }
}

/// What closing a day's batch names.
///
/// Three fields and no transaction: end of day is about the device rather than
/// any one sale, which is why it takes this rather than a
/// [`Reference`](crate::terminal::Reference).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EndOfDayRequest {
    /// The caller's own id for the request, echoed back.
    pub conversation_id: Box<str>,
    /// The device whose batch is being closed.
    pub device_unique_id: Box<str>,
    /// Whether every transaction is printed on the slip, or the totals alone.
    pub summary_on_slip: bool,
}

impl EndOfDayRequest {
    /// Closes the batch on one device, printing the totals alone.
    #[must_use]
    pub fn new(
        conversation_id: impl Into<Box<str>>,
        device_unique_id: impl Into<Box<str>>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            device_unique_id: device_unique_id.into(),
            summary_on_slip: false,
        }
    }

    /// Prints every transaction on the slip rather than the totals alone.
    #[must_use]
    pub const fn detailed_slip(mut self) -> Self {
        self.summary_on_slip = true;
        self
    }
}

/// A day's batch, closed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EndOfDay {
    /// The batch number the closed transactions belong to.
    pub batch_no: Option<Box<str>>,
    /// iyzico's own words about how it went.
    pub result_message: Option<Box<str>>,
    /// The caller's id for the request, echoed back.
    pub conversation_id: Option<Box<str>>,
    /// One total per acquiring bank. A device can be set up with more than
    /// one, and each closes its own batch.
    pub totals: Vec<BatchTotal>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

impl EndOfDay {
    /// The same answer for both integrations: VUK 509's own end of day and
    /// [`gmu::Client::end_of_day`](crate::terminal::gmu::Client::end_of_day)
    /// are documented with one body between them.
    pub(crate) fn read(response: wire::EndOfDayResponse, raw: Raw) -> Self {
        Self {
            batch_no: response.batch_no.map(String::into_boxed_str),
            result_message: response.result_message.map(String::into_boxed_str),
            conversation_id: response.conversation_id.map(String::into_boxed_str),
            totals: response
                .totals
                .unwrap_or_default()
                .into_iter()
                .map(|total| BatchTotal {
                    acquirer_id: total.acquirer_id.map(String::into_boxed_str),
                    acquirer_name: total.acquirer_name.map(String::into_boxed_str),
                    terminal_id: total.terminal_id.map(String::into_boxed_str),
                    bank_merchant_id: total.bank_merchant_id.map(String::into_boxed_str),
                    batch_no: total.batch_no.map(String::into_boxed_str),
                    // Kept as iyzico wrote them: their schema types both as
                    // strings and names no currency for the amount, so reading
                    // either as a number here would be inventing one.
                    total_amount: total.total_transaction_amount.map(String::into_boxed_str),
                    total_count: total.total_transaction_count.map(String::into_boxed_str),
                    response_code: total.response_code.map(String::into_boxed_str),
                })
                .collect(),
            raw,
        }
    }
}

/// What one acquiring bank settled in the batch just closed.
///
/// **The amounts are text.** iyzico types `totalTransactionAmount` and
/// `totalTransactionCount` as strings and names no currency for the amount
/// anywhere in this answer, so reading either as a number here would be
/// inventing a unit iyzico did not send. What they wrote is what is here.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BatchTotal {
    /// The acquiring bank's own id.
    pub acquirer_id: Option<Box<str>>,
    /// Its name.
    pub acquirer_name: Option<Box<str>>,
    /// The terminal number.
    pub terminal_id: Option<Box<str>>,
    /// The terminal's number at the bank, which is a different number.
    pub bank_merchant_id: Option<Box<str>>,
    /// The batch this bank closed.
    pub batch_no: Option<Box<str>>,
    /// What the successful transactions in it came to, as iyzico wrote it.
    pub total_amount: Option<Box<str>>,
    /// How many there were, likewise.
    pub total_count: Option<Box<str>>,
    /// The bank's own result code for the close.
    pub response_code: Option<Box<str>>,
}

/// One transaction, as every Terminal Host operation reports it.
///
/// A sale, a refund, a void and a status query all answer the same shape —
/// iyzico's `TerminalPaymentSuccessResponse` — so this is what all four of
/// [`Client`]'s calls return.
///
/// # Why this is not a [`Charge`](kasapay_core::Charge)
///
/// Because the shape cannot say which one it is. iyzico does not echo
/// `salesType` on the answer, so a successful sale and a successful
/// pre-authorisation are the same bytes, and
/// [`Status::Captured`](kasapay_core::Status::Captured) against
/// [`Status::Authorized`](kasapay_core::Status::Authorized) would be a guess
/// about money that is either taken or only held. A caller who knows what they
/// asked for knows which it was; this type does not, and does not pretend to.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payment {
    /// iyzico's own id for the payment. What a refund, a void and a query name it by.
    pub payment_id: Option<Box<str>>,
    /// The day it was posted, `YYYYMMDD`, as iyzico wrote it.
    ///
    /// This is what a [`Refund`] and a [`Void`] have to carry back.
    pub payment_date: Option<Box<str>>,
    /// What moved.
    ///
    /// `None` where iyzico sent no amount, or sent one in a currency
    /// [`Currency`] has no name for. The figure is still in [`Payment::raw`].
    pub amount: Option<Money>,
    /// How many instalments. Zero and one both mean a single payment.
    pub installments: Option<i32>,
    /// The bank's approval code.
    pub auth_code: Option<Box<str>>,
    /// The caller's id for the request, echoed back.
    pub conversation_id: Option<Box<str>>,
    /// The terminal it happened on.
    pub device_unique_id: Option<Box<str>>,
    /// The caller's own reference for this transaction.
    pub transaction_reference_id: Option<Box<str>>,
    /// When it happened at the terminal, as iyzico wrote it.
    ///
    /// Documented as ISO-8601 and kept as text, like [`AuthCode`]'s dates.
    ///
    /// [`AuthCode`]: crate::terminal::AuthCode
    pub transaction_date_time: Option<Box<str>>,
    /// The first six digits of the card.
    pub bin_number: Option<Box<str>>,
    /// The last four digits of the card.
    pub last_four_digits: Option<Box<str>>,
    /// What kind of card it was.
    pub card_type: Option<CardType>,
    /// The bank's own reference for the transaction.
    pub host_reference: Option<Box<str>>,
    /// The bank's reference for the void on this payment, where there is one.
    ///
    /// This and [`Payment::refund_host_reference`] are how a status query says
    /// a payment has been withdrawn or given back: nothing else on the answer
    /// distinguishes a live payment from a voided one.
    pub cancel_host_reference: Option<Box<str>>,
    /// The bank's reference for the refund on this payment, where there is one.
    pub refund_host_reference: Option<Box<str>>,
    /// The acquiring bank.
    pub acquirer_id: Option<Box<str>>,
    /// The issuing bank.
    pub issuer_id: Option<Box<str>>,
    /// The merchant's number at the bank.
    pub bank_merchant_id: Option<Box<str>>,
    /// The terminal's number at the bank.
    pub bank_terminal_id: Option<Box<str>>,
    /// The end-of-day batch this transaction falls in.
    pub batch_no: Option<Box<str>>,
    /// The system trace audit number.
    pub stan_no: Option<Box<str>>,
    /// How the card was read — chip, stripe, contactless.
    pub pos_entry_mode_code: Option<Box<str>>,
    /// When iyzico processed it, in Unix seconds.
    pub system_time: Option<i64>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

impl Payment {
    /// Reads a transaction out of the answer, or turns a refusal into an error.
    fn read(body: wire::PaymentResponse, raw: Raw) -> Result<Self, Error> {
        // A payment that does not say whether it worked has not been shown to
        // have worked.
        if body.status.is_none() {
            return Err(Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a Terminal Host answer carried no status",
            ));
        }
        // "SUCCESS, FAILURE, etc." is all iyzico says the field holds, so
        // anything that is not the one word is read as a refusal rather than
        // guessed at.
        let approved = body
            .status
            .as_deref()
            .is_some_and(|status| status.eq_ignore_ascii_case(SUCCESS));
        if !approved {
            return Err(refused(body));
        }

        let amount = match (body.price.as_deref(), body.currency.as_deref()) {
            (Some(price), Some(code)) => match code.parse::<Currency>() {
                Ok(currency) => Some(
                    Money::parse(wire::text(price), currency)
                        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?,
                ),
                Err(_) => None,
            },
            _ => None,
        };

        Ok(Self {
            payment_id: body.payment_id.map(String::into_boxed_str),
            payment_date: body.payment_date.map(String::into_boxed_str),
            amount,
            installments: body.installment,
            auth_code: body.auth_code.map(String::into_boxed_str),
            conversation_id: body.conversation_id.map(String::into_boxed_str),
            device_unique_id: body.device_unique_id.map(String::into_boxed_str),
            transaction_reference_id: body.transaction_reference_id.map(String::into_boxed_str),
            transaction_date_time: body.transaction_date_time.map(String::into_boxed_str),
            bin_number: body.bin_number.map(String::into_boxed_str),
            last_four_digits: body.last_four_digits.map(String::into_boxed_str),
            card_type: body.card_type.as_deref().map(CardType::from),
            host_reference: body.host_reference.map(String::into_boxed_str),
            cancel_host_reference: body.cancel_host_reference.map(String::into_boxed_str),
            refund_host_reference: body.refund_host_reference.map(String::into_boxed_str),
            acquirer_id: body.acquirer_id.map(String::into_boxed_str),
            issuer_id: body.issuer_id.map(String::into_boxed_str),
            bank_merchant_id: body.bank_merchant_id.map(String::into_boxed_str),
            bank_terminal_id: body.bank_terminal_id.map(String::into_boxed_str),
            batch_no: body.batch_no.map(String::into_boxed_str),
            stan_no: body.stan_no.map(String::into_boxed_str),
            pos_entry_mode_code: body.pos_entry_mode_code.map(String::into_boxed_str),
            system_time: body.system_time,
            raw,
        })
    }
}

/// What kind of card paid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardType {
    /// A credit card.
    Credit,
    /// A debit card.
    Debit,
    /// Something iyzico has started returning since this was written.
    ///
    /// They document the field as "`CREDIT_CARD`, `DEBIT_CARD`, etc.", and the
    /// "etc." is theirs.
    Other(Box<str>),
}

impl CardType {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Credit => "CREDIT_CARD",
            Self::Debit => "DEBIT_CARD",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for CardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for CardType {
    fn from(value: &str) -> Self {
        match value {
            "CREDIT_CARD" => Self::Credit,
            "DEBIT_CARD" => Self::Debit,
            other => Self::Other(other.into()),
        }
    }
}

/// Turns a refusal into an error, from the group and code iyzico sent.
///
/// `consumerErrorMessage` — the wording iyzico writes for the person at the
/// till — is used only when there is no technical message, because
/// [`Error`](kasapay_core::Error) has one message and the technical one is
/// what a log needs. A caller who has to show the other has nowhere to read it
/// from: a refusal carries no [`Raw`].
fn refused(body: wire::PaymentResponse) -> Error {
    let kind = errors::kind_for(body.error_group.as_deref(), body.error_code.as_deref());
    let message = body
        .error_message
        .or(body.consumer_error_message)
        .unwrap_or_else(|| match body.status {
            Some(status) => format!("iyzico answered {status}"),
            None => "iyzico refused the request".to_owned(),
        });
    let error = Error::new(kind, PROVIDER, message);
    match body.error_code {
        Some(code) => error.with_code(code),
        None => error,
    }
}

/// What a refusal with no readable body means, from its status alone.
fn kind_for_status(status: reqwest::StatusCode) -> ErrorKind {
    match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    }
}

/// Writes an amount as a bare JSON number, exactly as the decimal it is.
///
/// iyzico types `price` as a `double` here. Sending one through an `f64` is
/// how 10.10 becomes 10.099999999999999.
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

#[cfg(test)]
mod tests {
    use super::{CardType, Config, kind_for_status};
    use kasapay_core::ErrorKind;

    #[test]
    fn the_bases_iyzico_publishes_parse_and_join() {
        for config in [Config::sandbox(), Config::production()] {
            let url = config
                .endpoint("v2/terminal-host/payment")
                .expect("the path joins");
            assert_eq!(url.path(), "/v2/terminal-host/payment");
            // The login lives outside the terminal-host prefix, on the same host.
            let login = config
                .endpoint("in-store/oauth2/token")
                .expect("the path joins");
            assert_eq!(login.path(), "/in-store/oauth2/token");
        }
    }

    #[test]
    fn the_card_types_iyzico_names_round_trip_and_the_rest_are_kept() {
        assert_eq!(CardType::from("CREDIT_CARD"), CardType::Credit);
        assert_eq!(CardType::from("DEBIT_CARD"), CardType::Debit);
        assert_eq!(CardType::from("PREPAID_CARD").to_string(), "PREPAID_CARD");
    }

    #[test]
    fn a_refusal_with_no_body_is_read_from_its_status() {
        let kind = |code| kind_for_status(reqwest::StatusCode::from_u16(code).expect("a status"));
        assert_eq!(kind(401), ErrorKind::Auth);
        assert_eq!(kind(404), ErrorKind::NotFound);
        assert_eq!(kind(422), ErrorKind::InvalidRequest);
        assert_eq!(kind(500), ErrorKind::Provider);
    }
}
