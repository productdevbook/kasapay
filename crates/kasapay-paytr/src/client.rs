//! The PayTR client.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{
    Charge, ChargeRequest, Currency, Error, ErrorKind, Money, NextAction, OrderRef, PaymentId,
    Provider, ProviderId, Raw, Status,
};
use url::Url;

use crate::payment::Payment;
use crate::signing::Credentials;

/// PayTR.
pub const PAYTR: ProviderId = ProviderId::new("paytr");

/// Where the client points and what it signs with.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Box<str>,
    credentials: Credentials,
    timeout: Duration,
    test_mode: bool,
}

impl Config {
    /// PayTR's only base. There is no separate sandbox host — a test payment
    /// is a live request with `test_mode` set.
    pub const BASE: &'static str = "https://www.paytr.com";
    /// How long a request waits before it is given up on.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Points at PayTR with the given credentials.
    #[must_use]
    pub fn new(credentials: Credentials) -> Self {
        Self::at(Self::BASE, credentials)
            .unwrap_or_else(|_| unreachable!("the base constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    pub fn at(base_url: &str, credentials: Credentials) -> Result<Self, url::ParseError> {
        let trimmed = base_url.trim_end_matches('/');
        Url::parse(trimmed)?;
        Ok(Self {
            base_url: trimmed.into(),
            credentials,
            timeout: Self::DEFAULT_TIMEOUT,
            test_mode: false,
        })
    }

    /// Marks every payment as a test.
    ///
    /// PayTR has no sandbox host: a test payment is a live request carrying
    /// `test_mode=1`, and it goes into the signature, so this cannot be set by
    /// a proxy after the fact.
    #[must_use]
    pub const fn test_mode(mut self) -> Self {
        self.test_mode = true;
        self
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The credentials, for verifying a payment notice off the request thread.
    #[must_use]
    pub const fn credentials(&self) -> &Credentials {
        &self.credentials
    }
}

/// Takes payments through PayTR's hosted form.
///
/// Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct PayTr {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
}

impl PayTr {
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

    /// What this client signs with, for verifying a payment notice.
    #[must_use]
    pub fn credentials(&self) -> &Credentials {
        self.inner.config.credentials()
    }

    /// Opens a payment and hands back where to send the payer.
    ///
    /// PayTR hosts the form, so no card data crosses this process. The
    /// returned [`Charge`] carries the form's address and its token; the token
    /// is not needed again, because PayTR identifies the payment by the
    /// merchant's own order reference.
    pub async fn start_payment(&self, payment: &Payment) -> Result<Charge, Error> {
        let amount = payment.amount.minor_units().to_string();
        let basket = Self::basket(payment)?;
        let no_instalment = u8::from(!payment.instalments).to_string();
        let max_instalment = payment.max_instalments.to_string();
        let test_mode = u8::from(self.inner.config.test_mode).to_string();
        let currency = paytr_currency(payment.amount.currency());

        let token = self.inner.config.credentials.token(&[
            self.inner.config.credentials.merchant_id(),
            &payment.payer_ip,
            payment.order.as_str(),
            &payment.email,
            &amount,
            &basket,
            &no_instalment,
            &max_instalment,
            currency,
            &test_mode,
        ]);

        let form = [
            ("merchant_id", self.inner.config.credentials.merchant_id()),
            ("user_ip", &payment.payer_ip),
            ("merchant_oid", payment.order.as_str()),
            ("email", &payment.email),
            ("payment_amount", &amount),
            ("paytr_token", &token),
            ("user_basket", &basket),
            ("debug_on", "1"),
            ("no_installment", &no_instalment),
            ("max_installment", &max_instalment),
            ("user_name", &payment.name),
            ("user_address", &payment.address),
            ("user_phone", &payment.phone),
            ("merchant_ok_url", payment.success_url.as_str()),
            ("merchant_fail_url", payment.failure_url.as_str()),
            ("test_mode", &test_mode),
            ("currency", currency),
        ];

        let (response, raw) = self
            .post::<crate::wire::TokenResponse>("/odeme/api/get-token", &form)
            .await?;
        if response.status.as_deref() != Some("success") {
            let error = Error::new(
                ErrorKind::InvalidRequest,
                PAYTR,
                response
                    .reason
                    .unwrap_or_else(|| "PayTR refused to open the payment".to_owned()),
            );
            return Err(match response.err_no {
                Some(code) => error.with_code(code),
                None => error,
            });
        }
        let form_token = response.token.ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PAYTR,
                "an opened payment carried no token",
            )
        })?;

        let url = format!("{}/odeme/guvenli/{form_token}", self.inner.config.base_url);
        Ok(Charge {
            // PayTR has no id of its own; the merchant's order reference is
            // what every later call names the payment by.
            id: PaymentId::new(payment.order.as_str()),
            order: Some(payment.order.clone()),
            amount: payment.amount,
            // Nothing is charged yet, so the surcharge is not known: the
            // status query is what reports the pair.
            order_amount: None,
            status: Status::RequiresAction,
            next_action: Some(NextAction::Redirect {
                url: Url::parse(&url).map_err(|e| {
                    Error::new(ErrorKind::Malformed, PAYTR, "the form address is not a URL")
                        .with_source(e)
                })?,
                continuation: Some(form_token.into_boxed_str()),
            }),
            provider: PAYTR,
            raw,
        })
    }

    /// Every refund taken off a payment so far.
    ///
    /// PayTR reports these on the status query rather than as their own
    /// objects, so this reads the same call [`charge_status`] does. Summing
    /// them and comparing against the charge is how a caller answers "is this
    /// fully refunded" — a payment's status cannot say it, and
    /// deliberately so.
    ///
    /// [`charge_status`]: kasapay_core::Provider::charge_status
    pub async fn refunds(&self, order: &OrderRef) -> Result<Vec<RefundRecord>, Error> {
        let (response, _) = self.status(order.as_str()).await?;
        let currency = response
            .currency
            .as_deref()
            .map_or(Ok(Currency::Try), parse_currency)?;
        response
            .returns
            .iter()
            .map(|item| {
                let amount = item
                    .return_amount
                    .as_deref()
                    .map(|value| Money::parse(value, currency))
                    .transpose()
                    .map_err(|e| Error::new(ErrorKind::Malformed, PAYTR, e.to_string()))?
                    .unwrap_or_else(|| Money::from_minor_units(0, currency));
                Ok(RefundRecord {
                    amount,
                    requested: item.return_date.clone().map(String::into_boxed_str),
                    completed: item.date_completed.clone().map(String::into_boxed_str),
                    reference: item.return_ref_num.clone().map(String::into_boxed_str),
                })
            })
            .collect()
    }

    /// Gives money back off a payment.
    ///
    /// Partial refunds are allowed; PayTR takes the amount to return rather
    /// than the amount to keep.
    pub async fn refund(&self, order: &OrderRef, amount: Money) -> Result<Raw, Error> {
        let value = amount.to_decimal_string();
        let token = self.inner.config.credentials.token(&[
            self.inner.config.credentials.merchant_id(),
            order.as_str(),
            &value,
        ]);
        let form = [
            ("merchant_id", self.inner.config.credentials.merchant_id()),
            ("merchant_oid", order.as_str()),
            ("return_amount", value.as_str()),
            ("paytr_token", token.as_str()),
        ];
        let (response, raw) = self
            .post::<crate::wire::PlainResponse>("/odeme/iade", &form)
            .await?;
        if response.status.as_deref() != Some("success") {
            let error = Error::new(
                refund_error_kind(response.err_no.as_deref()),
                PAYTR,
                response
                    .reason
                    .unwrap_or_else(|| "PayTR refused the refund".to_owned()),
            );
            return Err(match response.err_no {
                Some(code) => error.with_code(code),
                None => error,
            });
        }
        Ok(raw)
    }

    /// The basket, as PayTR wants it: base64 of `[[name, price, quantity], …]`.
    fn basket(payment: &Payment) -> Result<String, Error> {
        let lines: Vec<_> = payment
            .basket
            .iter()
            .map(|item| {
                serde_json::json!([
                    item.name.as_ref(),
                    item.price.to_decimal_string(),
                    item.quantity
                ])
            })
            .collect();
        let json = serde_json::to_string(&lines).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PAYTR, "the basket is not JSON").with_source(e)
        })?;
        Ok(BASE64.encode(json))
    }

    /// The status query, which both `charge_status` and `refunds` read.
    async fn status(&self, order: &str) -> Result<(crate::wire::StatusResponse, Raw), Error> {
        let token = self
            .inner
            .config
            .credentials
            .token(&[self.inner.config.credentials.merchant_id(), order]);
        let form = [
            ("merchant_id", self.inner.config.credentials.merchant_id()),
            ("merchant_oid", order),
            ("paytr_token", token.as_str()),
        ];
        let (response, raw) = self
            .post::<crate::wire::StatusResponse>("/odeme/durum-sorgu", &form)
            .await?;
        if response.status.as_deref() != Some("success") {
            let error = Error::new(
                ErrorKind::NotFound,
                PAYTR,
                response
                    .reason
                    .clone()
                    .unwrap_or_else(|| "PayTR does not know this payment".to_owned()),
            );
            return Err(match response.err_no.clone() {
                Some(code) => error.with_code(code),
                None => error,
            });
        }
        Ok((response, raw))
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        form: &[(&str, &str)],
    ) -> Result<(T, Raw), Error> {
        let url = format!("{}{path}", self.inner.config.base_url);
        let response = self
            .inner
            .http
            .post(&url)
            .form(form)
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if !status.is_success() {
            return Err(Error::new(
                match status.as_u16() {
                    401 | 403 => ErrorKind::Auth,
                    404 => ErrorKind::NotFound,
                    429 => ErrorKind::RateLimited,
                    500..=599 => ErrorKind::Provider,
                    _ => ErrorKind::InvalidRequest,
                },
                PAYTR,
                format!("HTTP {status}: {text}"),
            ));
        }
        let typed = serde_json::from_slice(&bytes).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PAYTR,
                "response was not the JSON PayTR documents",
            )
            .with_source(e)
        })?;
        Ok((typed, Raw::from_text(text)))
    }
}

/// PayTR reads a payment back by the merchant's own order reference, and
/// cannot start one from a [`ChargeRequest`].
///
/// The same line as every other provider here: the trait can say what happened
/// to a payment and not how to begin one. PayTR needs an email, the payer's IP,
/// a name, an address, a phone number and two return URLs before it will open
/// a form.
#[async_trait::async_trait]
impl Provider for PayTr {
    fn id(&self) -> ProviderId {
        PAYTR
    }

    async fn charge(&self, _request: &ChargeRequest) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            "PayTR needs an email, the payer's IP, a name, an address, a phone number and \
             two return URLs; call PayTr::start_payment",
        ))
    }

    /// Reads a payment back by the order reference it was opened with.
    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let token = self
            .inner
            .config
            .credentials
            .token(&[self.inner.config.credentials.merchant_id(), id.as_str()]);
        let form = [
            ("merchant_id", self.inner.config.credentials.merchant_id()),
            ("merchant_oid", id.as_str()),
            ("paytr_token", token.as_str()),
        ];
        let (response, raw) = self
            .post::<crate::wire::StatusResponse>("/odeme/durum-sorgu", &form)
            .await?;
        if response.status.as_deref() != Some("success") {
            let error = Error::new(
                ErrorKind::NotFound,
                PAYTR,
                response
                    .reason
                    .unwrap_or_else(|| "PayTR does not know this payment".to_owned()),
            );
            return Err(match response.err_no {
                Some(code) => error.with_code(code),
                None => error,
            });
        }

        let currency = response
            .currency
            .as_deref()
            .map_or(Ok(Currency::Try), parse_currency)?;
        // payment_total is what the payer was charged; payment_amount is what
        // the order came to. They differ under an instalment surcharge, and a
        // Charge's amount is what moved.
        let amount = response
            .payment_total
            .as_deref()
            .or(response.payment_amount.as_deref())
            .map(|value| Money::parse(value, currency))
            .transpose()
            .map_err(|e| Error::new(ErrorKind::Malformed, PAYTR, e.to_string()))?
            .unwrap_or_else(|| Money::from_minor_units(0, currency));
        let order_amount = response
            .payment_amount
            .as_deref()
            .map(|value| Money::parse(value, currency))
            .transpose()
            .map_err(|e| Error::new(ErrorKind::Malformed, PAYTR, e.to_string()))?
            .filter(|order| *order != amount);

        Ok(Charge {
            id: id.clone(),
            order: Some(OrderRef::new(id.as_str())),
            amount,
            order_amount,
            // A durum-sorgu that answers success means the payment succeeded;
            // a failed or unstarted one is a failure status with a reason.
            status: Status::Captured,
            next_action: None,
            provider: PAYTR,
            raw,
        })
    }
}

/// A refund PayTR has recorded against a payment.
#[derive(Debug, Clone)]
pub struct RefundRecord {
    /// How much went back.
    pub amount: Money,
    /// When it was asked for, in PayTR's own format.
    pub requested: Option<Box<str>>,
    /// When it settled, if it has.
    pub completed: Option<Box<str>>,
    /// The bank's reference, for reconciling against a statement.
    pub reference: Option<Box<str>>,
}

/// PayTR's own spelling of a currency.
///
/// Lira is `TL` rather than the ISO code, and it is the default when the field
/// is left out — but it goes into the signature, so it is always sent.
const fn paytr_currency(currency: Currency) -> &'static str {
    match currency {
        Currency::Try => "TL",
        Currency::Usd => "USD",
        Currency::Eur => "EUR",
        Currency::Gbp => "GBP",
        // PayTR takes TL, EUR, USD, GBP and RUB and nothing else.
        Currency::Jpy | Currency::Kwd => "",
    }
}

fn parse_currency(value: &str) -> Result<Currency, Error> {
    match value {
        "TL" | "TRY" => Ok(Currency::Try),
        "USD" => Ok(Currency::Usd),
        "EUR" => Ok(Currency::Eur),
        "GBP" => Ok(Currency::Gbp),
        other => Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            format!("kasapay has no Currency for PayTR's {other}"),
        )),
    }
}

/// What a refund's error number means, from PayTR's own list.
///
/// Two of these say "try again later" in so many words, and reporting them as
/// a flat rejection is how a caller's retry loop gives up on a refund that
/// would have gone through.
///
/// <https://dev.paytr.com/hata-kodlari>
fn refund_error_kind(code: Option<&str>) -> ErrorKind {
    match code {
        // "iade yapilamiyor, daha sonra tekrar deneyin" — the refund service
        // is locked while a payout runs.
        Some("000") => ErrorKind::Provider,
        // "Net bakiyeniz yetersiz" — the balance may be there tomorrow.
        Some("010") => ErrorKind::Provider,
        // "paytr_token gonderilmedi veya gecersiz"
        Some("004") => ErrorKind::Auth,
        // "merchant_oid ile basarili odeme bulunamadi", and the payment that
        // exists but has not been reported yet.
        Some("005" | "007") => ErrorKind::NotFound,
        // "XYZ odeme tipi iade desteklemiyor"
        Some("008") => ErrorKind::Unsupported,
        // Everything else is a request PayTR will never accept: a bad amount,
        // more than was taken, or a payment over a year old.
        _ => ErrorKind::InvalidRequest,
    }
}

fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PAYTR, error.to_string())
}
