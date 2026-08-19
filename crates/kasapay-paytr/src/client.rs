//! The PayTR client.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Currency, Error, ErrorKind, IdempotencyKey, Instrument,
    Money, NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, Refund, RefundRequest,
    RefundStatus, Status,
};
use url::Url;

use crate::card::{CardDetails, CardKind, CardScheme};
use crate::payment::Payment;
use crate::signing::Credentials;

/// PayTR.
pub const PAYTR: ProviderId = ProviderId::new("paytr");

/// The field PayTR names a payment by, since it issues no identifier of its own.
const NAMED_BY: &[&str] = &["merchant_oid"];

/// How PayTR names the payment opened against an order reference.
///
/// PayTR issues no identifier for a payment: `merchant_oid` — the reference the
/// merchant chose and sent — is what reads it back, refunds it and arrives on
/// the payment notice. So the identifier here is
/// [`IdSource::Derived`](kasapay_core::IdSource::Derived), and what its
/// uniqueness rests on is whatever the merchant does about reusing a reference.
///
/// This is what [`Provider::charge_status`] takes for this provider.
#[must_use]
pub fn payment_id(order: &OrderRef) -> PaymentId {
    PaymentId::derived(order.as_str(), NAMED_BY)
}

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
        let currency = paytr_currency(payment.amount.currency())?;

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
            id: Some(payment_id(&payment.order)),
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

    /// What PayTR knows about a card, from the first 6 or 8 digits of its
    /// number.
    ///
    /// `bin` must be exactly 6 or 8 digits — PayTR takes no other length, and
    /// says to prefer 8 because 6 no longer identifies an issuer on its own.
    ///
    /// `Ok(None)` means PayTR has no record of this BIN. That is a documented
    /// answer rather than a failure: a card issued outside Turkey is the usual
    /// reason, and PayTR answers `status=failed` for it.
    ///
    /// The token here hashes **the BIN before the merchant id**, which is the
    /// opposite of every other call in this crate.
    ///
    /// <https://dev.paytr.com/direkt-api/bin-sorgulama-servisi>
    pub async fn bin_details(&self, bin: &str) -> Result<Option<CardDetails>, Error> {
        if !matches!(bin.len(), 6 | 8) || !bin.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PAYTR,
                "a BIN is 6 or 8 digits",
            ));
        }
        let merchant_id = self.inner.config.credentials.merchant_id();
        let token = self.inner.config.credentials.token(&[bin, merchant_id]);
        let form = [
            ("merchant_id", merchant_id),
            ("bin_number", bin),
            ("paytr_token", token.as_str()),
        ];

        let (response, _) = self
            .post::<crate::wire::BinResponse>("/odeme/api/bin-detail", &form)
            .await?;
        match response.status.as_deref() {
            Some("success") => card_details(response).map(Some),
            // Documented, and not an error: PayTR does not have this BIN.
            Some("failed") => Ok(None),
            _ => Err(Error::new(
                ErrorKind::InvalidRequest,
                PAYTR,
                response
                    .err_msg
                    .unwrap_or_else(|| "PayTR refused the BIN query".to_owned()),
            )),
        }
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
    ///
    /// Anything but a success is [`ErrorKind::NotFound`]: PayTR answers this
    /// call for a payment that succeeded, and gives a refused payment and an
    /// order it does not know the same reply.
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
                response.reason.clone().unwrap_or_else(|| {
                    "PayTR reports no successful payment against this reference, which is \
                     also what it answers for one it refused"
                        .to_owned()
                }),
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
    ///
    /// [`payment_id`] builds what this takes out of an [`OrderRef`].
    ///
    /// # A refused payment is not distinguishable here
    ///
    /// PayTR answers this call for a payment that succeeded and nothing else.
    /// A payment it refused and an order it has never heard of both come back
    /// as [`ErrorKind::NotFound`], carrying PayTR's own `err_no`, because that
    /// is all PayTR sends for either. The refusal itself is reported on the
    /// payment notice, which [`Notice::charge`](crate::Notice::charge) reads as
    /// a [`Status::Failed`] charge.
    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let (response, raw) = self.status(id.as_str()).await?;

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

        let order = OrderRef::new(id.as_str());
        Ok(Charge {
            // Rebuilt rather than echoed: what came in may claim PayTR issued
            // it, and PayTR issues nothing.
            id: Some(payment_id(&order)),
            order: Some(order),
            amount,
            order_amount,
            // durum-sorgu answers a payment that succeeded and nothing else.
            status: Status::Captured,
            next_action: None,
            provider: PAYTR,
            raw,
        })
    }

    /// Always [`ErrorKind::Unsupported`]: PayTR's form takes the money outright.
    ///
    /// A payment PayTR reports on is one the payer finished, and there is no
    /// authorisation held for a later call to take.
    /// [`Capabilities::separate_capture`] says so before a caller gets here.
    /// `idempotency` is ignored: there is no capture request to send it on.
    async fn capture(
        &self,
        _id: &PaymentId,
        _amount: Option<Money>,
        _idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            "PayTR takes the money when the payer finishes the form and \
             documents no capture step",
        ))
    }

    /// Always [`ErrorKind::Unsupported`]: there is no authorisation to release.
    ///
    /// Giving the money back is [`PayTr::refund`], which takes an order
    /// reference and an amount.
    async fn cancel(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            "PayTR holds no authorisation to release; giving the money back is PayTr::refund",
        ))
    }

    /// Gives money back off a payment, through [`PayTr::refund`].
    ///
    /// [`RefundRequest::payment`] is what [`payment_id`] built out of the
    /// order reference, and the reference is read back out of it — PayTR names
    /// a payment by nothing else, so the two are the same string and
    /// `/odeme/iade` takes it as `merchant_oid`.
    ///
    /// # Three of the request's fields have nowhere to go
    ///
    /// `amount: None` is [`ErrorKind::InvalidRequest`] before a socket opens.
    /// PayTR's refund takes `return_amount` and has no form that means "the
    /// rest", and the amount is signed into the token, so there is nothing to
    /// send for a full refund but the figure — which the caller has and this
    /// crate does not.
    ///
    /// [`RefundRequest::reason`] is dropped: PayTR's refund carries the
    /// merchant id, the order, the amount and the hash of those three, and
    /// nothing else — a reason has nowhere to go and losing it costs the
    /// caller a note, not money.
    ///
    /// [`RefundRequest::idempotency_key`] is [`ErrorKind::Unsupported`]
    /// instead, because losing *that* does cost money: PayTR documents no
    /// idempotency mechanism for `/odeme/iade`, and a refund sent without the
    /// key the caller asked for is one that can give the money back twice.
    ///
    /// # The answer says the request was taken, not that the money went back
    ///
    /// [`RefundStatus::Pending`] always. PayTR answers `status: success` for a
    /// refund it has accepted and reports its completion later, as a
    /// `date_completed` on the payment's own status query —
    /// [`PayTr::refunds`] is where that is read. Nothing in the answer names
    /// the refund, either, so [`Refund::id`] is `None`: PayTR issues no
    /// identifier for one, the same way it issues none for a payment.
    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error> {
        if request.idempotency_key.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PAYTR,
                "PayTR documents no idempotency mechanism for a refund; \
                 read PayTr::refunds before sending another",
            ));
        }
        let amount = request.amount.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidRequest,
                PAYTR,
                "PayTR's refund takes the amount to return and has no form that means \
                 the rest of it; name the amount",
            )
        })?;
        let order = OrderRef::new(request.payment.as_str());
        let raw = PayTr::refund(self, &order, amount).await?;
        Ok(Refund {
            id: None,
            payment: request.payment.clone(),
            amount,
            status: RefundStatus::Pending,
            next_action: None,
            provider: PAYTR,
            raw,
        })
    }

    /// Always [`ErrorKind::Unsupported`].
    ///
    /// PayTR's hosted form does store a card against a `utoken`, so a vault
    /// exists — but listing what is in it is a call this crate does not
    /// implement, for the same reason charging one is not: the hash that
    /// signs those calls is documented only as "see the sample code", and a
    /// signature guessed from a field table fails against every real
    /// merchant.
    async fn instruments(&self, _customer: &str) -> Result<Vec<Instrument>, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            "PayTR's card vault is not reachable from this crate; the hash that signs \
             cardstorage calls is documented only as \"see the sample code\"",
        ))
    }

    /// No separate capture; refunds are partial and may be repeated.
    ///
    /// PayTR has a vault — its hosted form stores a card against a `utoken`
    /// when asked to, and `ctoken` names one of them — but nothing here charges
    /// or lists one: the hash that signs those calls is documented only as
    /// "see the sample code", and a signature guessed from a field table fails
    /// against every real merchant. So [`Capabilities::saved_instruments`] is
    /// false, and [`Provider::instruments`] answers
    /// [`ErrorKind::Unsupported`] rather than an empty list.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: false,
            partial_capture: false,
            partial_refund: true,
            repeated_refund: true,
            saved_instruments: false,
        }
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
/// What PayTR calls a currency, where PayTR has a name for it.
///
/// PayTR takes TL, EUR, USD, GBP and RUB and nothing else, and lira is `TL`
/// rather than `TRY`. Anything else is refused here rather than sent: this
/// used to answer an empty string, which was signed into the token and posted,
/// so a payment in yen went to PayTR with no currency at all.
fn paytr_currency(currency: Currency) -> Result<&'static str, Error> {
    match currency {
        Currency::Try => Ok("TL"),
        Currency::Usd => Ok("USD"),
        Currency::Eur => Ok("EUR"),
        Currency::Gbp => Ok("GBP"),
        Currency::Rub => Ok("RUB"),
        Currency::Jpy | Currency::Kwd | Currency::Chf | Currency::Nok => Err(Error::new(
            ErrorKind::Unsupported,
            PAYTR,
            format!("PayTR does not settle in {currency}"),
        )),
    }
}

/// The card behind a BIN PayTR does know.
///
/// Every field the BIN service documents under `status=success` is required
/// here, and a value outside the documented set is [`ErrorKind::Malformed`]
/// rather than a silent default: this answer decides whether a payer is offered
/// instalments and whether a card may go without 3-D Secure.
fn card_details(response: crate::wire::BinResponse) -> Result<CardDetails, Error> {
    let missing = |field: &str| {
        Error::new(
            ErrorKind::Malformed,
            PAYTR,
            format!("a known BIN carried no {field}"),
        )
    };
    let unreadable = |field: &str, value: &str| {
        Error::new(
            ErrorKind::Malformed,
            PAYTR,
            format!("PayTR answered {value} for {field}, which it does not document"),
        )
    };

    let card_type = response.card_type.ok_or_else(|| missing("cardType"))?;
    let schema = response.schema.ok_or_else(|| missing("schema"))?;
    let business_card = response
        .business_card
        .ok_or_else(|| missing("businessCard"))?;
    let allow_non3d = response.allow_non3d.ok_or_else(|| missing("allow_non3d"))?;

    Ok(CardDetails {
        kind: CardKind::parse(&card_type).ok_or_else(|| unreadable("cardType", &card_type))?,
        business: parse_yes_no(&business_card)
            .ok_or_else(|| unreadable("businessCard", &business_card))?,
        bank: response
            .bank
            .ok_or_else(|| missing("bank"))?
            .into_boxed_str(),
        bank_code: response
            .bank_code
            .ok_or_else(|| missing("bankCode"))?
            .into_string()
            .into_boxed_str(),
        // "none" is PayTR saying the card is in no instalment programme, not a
        // field it left out.
        programme: response
            .brand
            .filter(|brand| brand != "none")
            .map(String::into_boxed_str),
        scheme: CardScheme::parse(&schema).ok_or_else(|| unreadable("schema", &schema))?,
        non_3d_allowed: parse_yes_no(&allow_non3d)
            .ok_or_else(|| unreadable("allow_non3d", &allow_non3d))?,
    })
}

/// PayTR's booleans on the BIN service: `y`/`n` for a company card, `Y`/`N`
/// for the non-3-D permission.
fn parse_yes_no(value: &str) -> Option<bool> {
    match value {
        "y" | "Y" => Some(true),
        "n" | "N" => Some(false),
        _ => None,
    }
}

/// What PayTR calls a currency, read back.
///
/// The same five [`paytr_currency`] sends, lira included under both spellings.
fn parse_currency(value: &str) -> Result<Currency, Error> {
    match value {
        "TL" | "TRY" => Ok(Currency::Try),
        "USD" => Ok(Currency::Usd),
        "EUR" => Ok(Currency::Eur),
        "GBP" => Ok(Currency::Gbp),
        "RUB" => Ok(Currency::Rub),
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
        // is locked while a payout runs — and "Net bakiyeniz yetersiz", where
        // the balance may be there tomorrow.
        Some("000" | "010") => ErrorKind::Provider,
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

#[cfg(test)]
mod tests {
    use super::{parse_currency, paytr_currency};
    use kasapay_core::Currency;

    /// Whatever is sent to PayTR can be read back from PayTR.
    ///
    /// The two directions are separate matches and only one of them is
    /// exhaustive: `parse_currency` matches on text, so the compiler cannot
    /// see a currency missing from it. Roubles were sent and unreadable for
    /// exactly that reason.
    #[test]
    fn every_currency_paytr_is_sent_can_be_read_back() {
        for currency in every_currency() {
            if let Ok(sent) = paytr_currency(currency) {
                assert_eq!(
                    parse_currency(sent).ok(),
                    Some(currency),
                    "{currency} is sent to PayTR as `{sent}` and cannot be read back"
                );
            }
        }
    }

    /// Every currency there is.
    ///
    /// The `match` is what keeps this list honest: adding a variant to
    /// `Currency` stops it compiling until somebody comes here and decides
    /// whether PayTR takes it.
    fn every_currency() -> Vec<Currency> {
        let every = vec![
            Currency::Try,
            Currency::Usd,
            Currency::Eur,
            Currency::Gbp,
            Currency::Jpy,
            Currency::Kwd,
            Currency::Rub,
            Currency::Chf,
            Currency::Nok,
        ];
        for currency in &every {
            match currency {
                Currency::Try
                | Currency::Usd
                | Currency::Eur
                | Currency::Gbp
                | Currency::Jpy
                | Currency::Kwd
                | Currency::Rub
                | Currency::Chf
                | Currency::Nok => {}
            }
        }
        every
    }
}
