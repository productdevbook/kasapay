//! The classic API client.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kasapay_core::{
    Capabilities, Charge, ChargeRequest, Currency, Error, ErrorKind, IdempotencyKey, Instrument,
    InstrumentId, Money, NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, Refund,
    RefundReason, RefundRequest, RefundStatus, Status,
};
use url::Url;

use crate::classic::checkout::CheckoutForm;
use crate::classic::{FormToken, saved, signature, wire};
use crate::reporting;
use crate::signing::Credentials;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// A payment that takes the money.
const PAYMENT_AUTH: &str = "/payment/auth";
/// The same request, holding the money instead.
const PAYMENT_PREAUTH: &str = "/payment/preauth";
/// Turning a hold into a sale.
const PAYMENT_POSTAUTH: &str = "/payment/postauth";
/// The hosted form that takes the money.
const CHECKOUT_FORM_AUTH: &str = "/payment/iyzipos/checkoutform/initialize/auth/ecom";
/// The same form, holding the money instead.
const CHECKOUT_FORM_PREAUTH: &str = "/payment/iyzipos/checkoutform/initialize/preauth/ecom";

/// Where the classic client points and what it signs with.
#[derive(Debug, Clone)]
pub struct Config {
    /// Held as text, not as a `Url`: `Url`'s own rendering puts a trailing
    /// slash back on a bare authority, and `{base}{path}` then signs
    /// `//payment/bin/check` while the server sees `/payment/bin/check`.
    base_url: Box<str>,
    credentials: Credentials,
    timeout: Duration,
    require_signature: bool,
}

impl Config {
    /// The production base.
    pub const PRODUCTION: &'static str = "https://api.iyzipay.com";
    /// The sandbox base.
    pub const SANDBOX: &'static str = "https://sandbox-api.iyzipay.com";
    /// How long a request waits before it is given up on.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// Points at the sandbox.
    #[must_use]
    pub fn sandbox(credentials: Credentials) -> Self {
        Self::new(Self::SANDBOX, credentials)
            .unwrap_or_else(|_| unreachable!("the sandbox constant parses"))
    }

    /// Points at production.
    #[must_use]
    pub fn production(credentials: Credentials) -> Self {
        Self::new(Self::PRODUCTION, credentials)
            .unwrap_or_else(|_| unreachable!("the production constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    ///
    /// A trailing slash is trimmed: the signature covers the path alone, and
    /// `//payment/bin/check` would sign something the server never sees.
    pub fn new(base_url: &str, credentials: Credentials) -> Result<Self, url::ParseError> {
        let trimmed = base_url.trim_end_matches('/');
        Url::parse(trimmed)?;
        Ok(Self {
            base_url: trimmed.into(),
            credentials,
            timeout: Self::DEFAULT_TIMEOUT,
            require_signature: true,
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Accepts a response that carries no `signature`.
    ///
    /// Off by default, and leaving it off is the right thing: a response whose
    /// signature is missing is one that has not been shown to come from
    /// iyzico, and a forged callback is how a merchant ships against a payment
    /// that never happened. Turn it on only for an endpoint iyzico is known
    /// not to sign, and say which one in a comment.
    ///
    /// A signature that is present but wrong is refused either way.
    #[must_use]
    pub const fn allow_unsigned(mut self) -> Self {
        self.require_signature = false;
        self
    }
}

/// Talks to iyzico's classic API.
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
    #[must_use]
    pub fn with_http(http: reqwest::Client, config: Config) -> Self {
        Self {
            inner: Arc::new(Inner { http, config }),
        }
    }

    /// Asks what kind of card a BIN belongs to.
    ///
    /// A BIN is the leading six or eight digits of a card number, which name
    /// the issuer rather than the card, so this is not card data in the sense
    /// that matters — no PCI scope comes with it. Worth asking before offering
    /// instalments, which most issuers only allow on a credit card.
    pub async fn bin_check(&self, bin: &str) -> Result<BinDetails, Error> {
        let body = wire::BinCheckRequest {
            locale: "tr",
            bin_number: bin,
            conversation_id: None,
        };
        let (response, raw) = self
            .post::<_, wire::BinCheckResponse>("/payment/bin/check", &body)
            .await?;
        into_bin_details(response, raw)
    }

    /// Opens a hosted checkout form and hands back where to send the payer.
    ///
    /// iyzico hosts the form and collects the card, so no card data crosses
    /// this process. The returned [`Charge`] is
    /// [`Status::RequiresAction`] with a
    /// [`NextAction::Redirect`] whose `continuation` is the form's token —
    /// keep it, because [`Client::checkout_result`] needs it and the callback
    /// carries nothing else that identifies the payment.
    ///
    /// [`Charge::id`] is `None` here: iyzico issues no `paymentId` until the
    /// payer finishes, and the token is a [`FormToken`] rather than a payment
    /// id. The first charge to carry one is the one `checkout_result` answers.
    ///
    /// This is also how a card gets into iyzico's vault without a number
    /// crossing this process: the form offers the payer a save-my-card box, and
    /// [`CheckoutFormBuilder::card_user_key`](crate::classic::checkout::CheckoutFormBuilder::card_user_key)
    /// decides whose vault it joins.
    pub async fn start_checkout_form(&self, form: &CheckoutForm) -> Result<Charge, Error> {
        self.open_form(form, false).await
    }

    /// Opens a hosted checkout form that **holds** the money rather than
    /// taking it.
    ///
    /// The same form, the same [`CheckoutForm`], the same token and the same
    /// [`Client::checkout_result`] afterwards —
    /// `/payment/iyzipos/checkoutform/initialize/preauth/ecom` instead of
    /// `…/auth/ecom`, which is the whole difference on the wire. What changes
    /// is what the payer's approval does: the funds are authorised and wait
    /// for [`Provider::capture`], rather than being taken there and then.
    ///
    /// A hold nobody captures is not free money returned instantly — it sits
    /// against the payer's limit until iyzico or the issuer releases it, which
    /// is what [`Client::cancel`] is for. Same-day only; after that it is a
    /// refund.
    ///
    /// **The charge this answers is still [`Status::RequiresAction`]**, for
    /// the reason the ordinary form's is: nothing has happened until the payer
    /// finishes. It is [`Client::checkout_result`] that answers
    /// [`Status::Authorized`] once they have.
    pub async fn start_checkout_form_preauth(&self, form: &CheckoutForm) -> Result<Charge, Error> {
        self.open_form(form, true).await
    }

    /// Both forms, which differ in the path and in nothing else.
    async fn open_form(&self, form: &CheckoutForm, holds: bool) -> Result<Charge, Error> {
        let currency = form.price.currency();
        let body = wire::CheckoutFormRequest {
            locale: "tr",
            conversation_id: form.order.as_str(),
            price: form.price.to_decimal_string(),
            paid_price: form.paid_price.to_decimal_string(),
            currency: currency.code(),
            basket_id: form.order.as_str(),
            callback_url: form.callback_url.to_string(),
            enabled_installments: form.instalments.clone(),
            card_user_key: form.card_user_key.as_deref(),
            buyer: buyer_body(&form.buyer),
            billing_address: address_body(&form.billing_address),
            shipping_address: address_body(&form.shipping_address),
            basket_items: form.basket.iter().map(basket_item_body).collect(),
        };

        let (response, raw) = if holds {
            self.post::<_, wire::CheckoutFormResponse>(CHECKOUT_FORM_PREAUTH, &body)
                .await?
        } else {
            self.post::<_, wire::CheckoutFormResponse>(CHECKOUT_FORM_AUTH, &body)
                .await?
        };
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to open the form",
        ) {
            return Err(error);
        }
        let token = response.token.ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "an opened form carried no token",
            )
        })?;
        let page = response.payment_page_url.ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "an opened form carried no paymentPageUrl",
            )
        })?;

        let conversation_id = response
            .conversation_id
            .as_deref()
            .unwrap_or(form.order.as_str());
        self.check_signature(
            response.signature.as_deref(),
            &[conversation_id, token.as_str()],
        )?;

        Ok(Charge {
            id: None,
            order: Some(form.order.clone()),
            amount: form.paid_price,
            order_amount: (form.price != form.paid_price).then_some(form.price),
            status: Status::RequiresAction,
            next_action: Some(NextAction::Redirect {
                url: Url::parse(&page).map_err(|e| {
                    Error::new(
                        ErrorKind::Malformed,
                        PROVIDER,
                        "paymentPageUrl is not a URL",
                    )
                    .with_source(e)
                })?,
                continuation: Some(token.into_boxed_str()),
            }),
            provider: PROVIDER,
            raw,
        })
    }

    /// Reads what became of a checkout form, by the token it was opened with.
    ///
    /// A [`FormToken`], not a [`PaymentId`]: this endpoint answers for the
    /// form, and the payment id iyzico issues once the payer finishes is not
    /// one it will accept. Passing one does not compile.
    ///
    /// ```compile_fail
    /// use kasapay_core::PaymentId;
    /// use kasapay_iyzico::classic::Client;
    ///
    /// async fn read_back(iyzipay: &Client, payment: &PaymentId) {
    ///     iyzipay.checkout_result(payment).await.ok();
    /// }
    /// ```
    ///
    /// # A card the payer chose to save comes back here
    ///
    /// On [`Charge::raw`], at `/cardUserKey` and `/cardToken`, and not on the
    /// [`Charge`] itself: a saved instrument is one provider's idea and the
    /// shared type has no field for it.
    ///
    /// ```no_run
    /// # use kasapay_core::{Charge, InstrumentId};
    /// # use kasapay_iyzico::classic::saved;
    /// # fn read(charge: &Charge) -> Option<saved::Card> {
    /// let key = charge.raw.text_at("/cardUserKey")?;
    /// let token = charge.raw.text_at("/cardToken")?;
    /// saved::Card::new(key, InstrumentId::issued(token)).ok()
    /// # }
    /// ```
    ///
    /// Neither field is in `specs/` — iyzico's documentation of this response
    /// lists neither — and both are in their own SDKs and in the sample result
    /// on their documentation site. A form the payer did not save a card on
    /// carries neither.
    pub async fn checkout_result(&self, token: &FormToken) -> Result<Charge, Error> {
        self.form_result(token, false).await
    }

    /// Reads what became of a form opened by
    /// [`Client::start_checkout_form_preauth`].
    ///
    /// The same endpoint, the same token, the same signed answer — and a
    /// different word for the same `paymentStatus`. A pre-authorisation that
    /// succeeded is [`Status::Authorized`]: the money is held and
    /// [`Provider::capture`] is what takes it.
    ///
    /// # Why the caller has to say which
    ///
    /// iyzico has one result endpoint for both forms and its answer does not
    /// say which one was opened. There is a `phase` field on the payment, but
    /// iyzico documents it as "the transaction phase" and names no values for
    /// it, so reading a hold out of it would be a guess — and the guess that
    /// fails writes a sale into a shop's ledger for money nobody has taken.
    /// The caller opened the form and knows.
    pub async fn checkout_result_preauth(&self, token: &FormToken) -> Result<Charge, Error> {
        self.form_result(token, true).await
    }

    async fn form_result(&self, token: &FormToken, holds: bool) -> Result<Charge, Error> {
        let body = wire::CheckoutResultRequest {
            locale: "tr",
            token: token.as_str(),
        };
        let (response, raw) = self
            .post::<_, wire::PaymentResultResponse>(
                "/payment/iyzipos/checkoutform/auth/ecom/detail",
                &body,
            )
            .await?;
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message.clone(),
            response.error_code.clone(),
            "iyzico refused to read the form",
        ) {
            return Err(error);
        }
        self.check_signature(
            response.signature.as_deref(),
            &[
                response.payment_status.as_deref().unwrap_or_default(),
                response.payment_id.as_deref().unwrap_or_default(),
                response.currency.as_deref().unwrap_or_default(),
                response.basket_id.as_deref().unwrap_or_default(),
                response.conversation_id.as_deref().unwrap_or_default(),
                signature::signed_amount(response.paid_price.as_deref().unwrap_or_default()),
                signature::signed_amount(response.price.as_deref().unwrap_or_default()),
                response.token.as_deref().unwrap_or_default(),
            ],
        )?;
        into_payment_charge(response, raw, holds)
    }

    /// Reads a payment back by the id iyzico gave it.
    ///
    /// `POST /payment/detail`. This is the one that takes a payment id, and
    /// the only way a classic payment is read back once the payer has
    /// finished — [`Client::checkout_result`] takes the form's own token and
    /// answers the same shape while the payment is still being made.
    ///
    /// The response is signed over `paymentId`, `currency`, `basketId`,
    /// `conversationId`, `paidPrice` and `price`, which iyzico documents in
    /// both languages, and an answer that does not match is
    /// [`ErrorKind::Untrusted`] rather than a charge.
    ///
    /// # A payment only authorised reads as captured here
    ///
    /// The known limit of this call. iyzico answers `paymentStatus: SUCCESS`
    /// for a payment taken and for one
    /// [`Client::preauth_with_saved_card`] has only held, and nothing in the
    /// documented response separates them — there is a `phase` field, and
    /// iyzico documents it as "the transaction phase" and names no values for
    /// it. So a caller who authorised and has not captured yet knows that from
    /// their own ledger rather than from here, the same way
    /// [`Client::checkout_result_preauth`] has them say which form they
    /// opened. Reading `phase` for a word iyzico has not written down would be
    /// a guess, and the guess that fails reports money taken that is only
    /// held.
    pub async fn payment(&self, id: &PaymentId) -> Result<Charge, Error> {
        let body = wire::PaymentDetailRequest {
            locale: "tr",
            payment_id: id.as_str(),
        };
        let (response, raw) = self
            .post::<_, wire::PaymentResultResponse>("/payment/detail", &body)
            .await?;
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message.clone(),
            response.error_code.clone(),
            "iyzico refused to read the payment",
        ) {
            return Err(error);
        }
        self.check_signature(
            response.signature.as_deref(),
            &[
                response.payment_id.as_deref().unwrap_or_default(),
                response.currency.as_deref().unwrap_or_default(),
                response.basket_id.as_deref().unwrap_or_default(),
                response.conversation_id.as_deref().unwrap_or_default(),
                signature::signed_amount(response.paid_price.as_deref().unwrap_or_default()),
                signature::signed_amount(response.price.as_deref().unwrap_or_default()),
            ],
        )?;
        into_payment_charge(response, raw, false)
    }

    /// Charges a card iyzico already holds, sending no card number.
    ///
    /// `POST /payment/auth`, with `paymentCard` filled by the `cardUserKey` and
    /// `cardToken` of a [`saved::Card`] rather than by a number. Everything
    /// else iyzico wants of an ordinary card payment it wants here too — the
    /// buyer, both addresses, the itemised basket — which is why this takes a
    /// [`saved::Payment`] and not a
    /// [`ChargeRequest`](kasapay_core::ChargeRequest).
    ///
    /// The answer is signed over the same six fields as any other payment, and
    /// one that does not match is [`ErrorKind::Untrusted`] rather than a
    /// charge.
    ///
    /// # This is a payment without 3-D Secure
    ///
    /// `/payment/auth` runs no challenge, so the chargeback liability for a
    /// payment taken this way sits with the merchant. There is no authenticated
    /// variant here because this crate implements neither 3-D Secure call, not
    /// because a stored card could not go through one — see [`saved`].
    ///
    /// # The status comes from `fraudStatus`
    ///
    /// A payment iyzico's fraud filters are still reviewing is
    /// [`Status::Pending`] rather than [`Status::Captured`]: iyzico says to
    /// wait for their notification. This mapping has not been checked against a
    /// live account — see the crate's `CLAUDE.md`.
    pub async fn pay_with_saved_card(&self, payment: &saved::Payment) -> Result<Charge, Error> {
        self.charge_saved_card(payment, false).await
    }

    /// Holds money on a card iyzico already holds, without taking it.
    ///
    /// `POST /payment/preauth`, which is `/payment/auth`'s own request body
    /// sent to a path that authorises instead of charging — everything
    /// [`Client::pay_with_saved_card`] says about what iyzico wants, about
    /// 3-D Secure and about who is liable applies here unchanged.
    ///
    /// What comes back is [`Status::Authorized`] rather than
    /// [`Status::Captured`], and the money is taken by [`Provider::capture`].
    /// A hold that will never be taken is released by [`Client::cancel`],
    /// same-day; after that it is a refund.
    ///
    /// The answer is signed over the same six fields as any other payment, and
    /// one that does not match is [`ErrorKind::Untrusted`] rather than a
    /// charge.
    pub async fn preauth_with_saved_card(&self, payment: &saved::Payment) -> Result<Charge, Error> {
        self.charge_saved_card(payment, true).await
    }

    /// Both stored-card payments, which differ in the path and in what a
    /// success means.
    async fn charge_saved_card(
        &self,
        payment: &saved::Payment,
        holds: bool,
    ) -> Result<Charge, Error> {
        let currency = payment.price.currency();
        let body = wire::SavedCardPaymentRequest {
            locale: "tr",
            conversation_id: payment.order.as_str(),
            price: payment.price.to_decimal_string(),
            paid_price: payment.paid_price.to_decimal_string(),
            currency: currency.code(),
            basket_id: payment.order.as_str(),
            installment: payment.instalment,
            payment_card: wire::SavedCardBody {
                card_user_key: payment.card.user_key(),
                card_token: payment.card.token().as_str(),
            },
            buyer: buyer_body(&payment.buyer),
            billing_address: address_body(&payment.billing_address),
            shipping_address: address_body(&payment.shipping_address),
            basket_items: payment.basket.iter().map(basket_item_body).collect(),
        };

        let (response, raw) = if holds {
            self.post::<_, wire::PaymentResultResponse>(PAYMENT_PREAUTH, &body)
                .await?
        } else {
            self.post::<_, wire::PaymentResultResponse>(PAYMENT_AUTH, &body)
                .await?
        };
        self.read_payment_answer(response, raw, holds)
    }

    /// Reads what `/payment/auth`, `/payment/preauth` and `/payment/postauth`
    /// all answer: the same body, signed over the same six fields.
    ///
    /// `holds` is what separates them. A pre-authorisation that passed fraud
    /// review is money **held**, and saying `Captured` for it would put a sale
    /// in the caller's ledger for money nobody has taken.
    fn read_payment_answer(
        &self,
        response: wire::PaymentResultResponse,
        raw: Raw,
        holds: bool,
    ) -> Result<Charge, Error> {
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message.clone(),
            response.error_code.clone(),
            "iyzico refused the payment",
        ) {
            return Err(error);
        }
        self.check_signature(
            response.signature.as_deref(),
            &[
                response.payment_id.as_deref().unwrap_or_default(),
                response.currency.as_deref().unwrap_or_default(),
                response.basket_id.as_deref().unwrap_or_default(),
                response.conversation_id.as_deref().unwrap_or_default(),
                signature::signed_amount(response.paid_price.as_deref().unwrap_or_default()),
                signature::signed_amount(response.price.as_deref().unwrap_or_default()),
            ],
        )?;
        let status = match fraud_status(response.fraud_status) {
            Status::Captured if holds => Status::Authorized,
            status => status,
        };
        charge_from(response, raw, status)
    }

    /// Refuses a response that is not signed, or is signed wrongly.
    fn check_signature(&self, signature: Option<&str>, values: &[&str]) -> Result<(), Error> {
        match signature {
            Some(signature) => {
                if self
                    .inner
                    .config
                    .credentials
                    .verify_response(signature, values)
                {
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorKind::Untrusted,
                        PROVIDER,
                        "the response signature does not match what iyzico should have sent",
                    ))
                }
            }
            None if self.inner.config.require_signature => Err(Error::new(
                ErrorKind::Untrusted,
                PROVIDER,
                "the response carried no signature; Config::allow_unsigned accepts one anyway",
            )),
            None => Ok(()),
        }
    }

    /// Takes an amount back off a payment.
    ///
    /// `/v2/payment/refund`, which refunds against the payment as a whole and
    /// lets iyzico decide which basket line it comes off.
    ///
    /// # Not for a basket with more than one line
    ///
    /// iyzico's own words: *"It is strictly not recommended to use the Refund
    /// V2 service for orders with more than one product in the basket."* Which
    /// line the refund lands on is then iyzico's choice, and a shop's ledger
    /// says something different from iyzico's.
    ///
    /// Use [`Client::refund_transaction`] for anything with more than one
    /// line. It names the line, which is what a shop refunding one returned
    /// item of three actually means.
    ///
    /// # Repeated refunds
    ///
    /// Allowed, and documented: a refund must not exceed the amount still
    /// refundable, and *"as long as that rule is followed, more than one
    /// refund may be made in succession"*. That is what
    /// [`Capabilities::repeated_refund`](kasapay_core::Capabilities::repeated_refund)
    /// reports for this provider.
    ///
    /// # The reason
    ///
    /// [`Reason`] is what iyzico is told the money went back for, and `None`
    /// sends neither field. It is worth sending: `Fraud` and `DoublePayment`
    /// are what a shop tells its acquirer, and they end up in chargeback and
    /// reconciliation reporting where `Other` says nothing.
    pub async fn refund(
        &self,
        payment: &PaymentId,
        amount: Money,
        reason: Option<&Reason>,
    ) -> Result<Reversal, Error> {
        let body = wire::RefundRequest {
            locale: "tr",
            conversation_id: payment.as_str(),
            payment_id: payment.as_str(),
            price: amount.to_decimal_string(),
            currency: amount.currency().code(),
            reason: reason.map(|reason| reason.code().as_str()),
            description: reason.and_then(Reason::description),
        };
        let (response, raw) = self
            .post::<_, wire::ReversalResponse>("/v2/payment/refund", &body)
            .await?;
        self.read_reversal(response, raw, true)
    }

    /// Takes an amount back off one line of a payment.
    ///
    /// `paymentTransactionId` names the line, and comes from the payment's own
    /// `itemTransactions`. It is not the payment id.
    ///
    /// This is the one to use for a basket with more than one line — see
    /// [`Client::refund`] for why. The amount must not exceed that line's own
    /// price, rather than the payment's.
    ///
    /// `reason` is the same as on [`Client::refund`].
    pub async fn refund_transaction(
        &self,
        transaction: &str,
        amount: Money,
        reason: Option<&Reason>,
    ) -> Result<Reversal, Error> {
        let body = wire::RefundTransactionRequest {
            locale: "tr",
            conversation_id: transaction,
            payment_transaction_id: transaction,
            price: amount.to_decimal_string(),
            currency: amount.currency().code(),
            reason: reason.map(|reason| reason.code().as_str()),
            description: reason.and_then(Reason::description),
        };
        let (response, raw) = self
            .post::<_, wire::ReversalResponse>("/payment/refund", &body)
            .await?;
        self.read_reversal(response, raw, true)
    }

    /// Voids a payment outright, before it settles.
    ///
    /// Same-day only, and all of it — there is no partial cancel. After
    /// settlement the answer is a refund instead.
    ///
    /// # Not signed
    ///
    /// iyzico's cancel response carries no `signature`, so unlike a refund it
    /// cannot be shown to have come from them. A forged one would say a
    /// payment was voided when it was not. Read [`Client::checkout_result`]
    /// afterwards if that matters to the caller's ledger.
    ///
    /// `reason` is the same as on [`Client::refund`].
    pub async fn cancel(
        &self,
        payment: &PaymentId,
        reason: Option<&Reason>,
    ) -> Result<Reversal, Error> {
        let body = wire::CancelRequest {
            locale: "tr",
            conversation_id: payment.as_str(),
            payment_id: payment.as_str(),
            reason: reason.map(|reason| reason.code().as_str()),
            description: reason.and_then(Reason::description),
        };
        let (response, raw) = self
            .post::<_, wire::ReversalResponse>("/payment/cancel", &body)
            .await?;
        self.read_reversal(response, raw, false)
    }

    /// Reads a refund or cancel, verifying it where iyzico signs it.
    fn read_reversal(
        &self,
        response: wire::ReversalResponse,
        raw: Raw,
        signed: bool,
    ) -> Result<Reversal, Error> {
        if matches!(response.status.as_deref(), Some(s) if !s.eq_ignore_ascii_case("success")) {
            // iyzico says itself whether this is worth sending again, so the
            // kind follows their word rather than a guess about the message.
            let kind = if response.retryable == Some(true) {
                ErrorKind::Provider
            } else {
                ErrorKind::InvalidRequest
            };
            let error = Error::new(
                kind,
                PROVIDER,
                response
                    .error_message
                    .unwrap_or_else(|| "iyzico refused the reversal".to_owned()),
            );
            return Err(match response.error_code {
                Some(code) => error.with_code(code),
                None => error,
            });
        }

        let price = response.price.clone().unwrap_or_default();
        if signed {
            self.check_signature(
                response.signature.as_deref(),
                &[
                    response.payment_id.as_deref().unwrap_or_default(),
                    signature::signed_amount(&price),
                    response.currency.as_deref().unwrap_or_default(),
                    response.conversation_id.as_deref().unwrap_or_default(),
                ],
            )?;
        }

        let currency = response
            .currency
            .as_deref()
            .map_or(Ok(Currency::Try), str::parse)
            .map_err(|e: kasapay_core::UnknownCurrency| {
                Error::new(ErrorKind::Malformed, PROVIDER, e.to_string())
            })?;
        let amount = if price.is_empty() {
            Money::from_minor_units(0, currency)
        } else {
            Money::parse(&price, currency)
                .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?
        };

        Ok(Reversal {
            payment: response
                .payment_id
                .filter(|id| !id.is_empty())
                .map(PaymentId::issued),
            amount,
            host_reference: response.host_reference.map(String::into_boxed_str),
            raw,
        })
    }

    /// Lists the cards stored against a user key.
    ///
    /// No card number goes either way: the request carries the user key and
    /// the answer carries tokens, a BIN and the last four digits. Everything
    /// needed to let somebody pick a saved card, and nothing that puts the
    /// caller in PCI scope.
    pub async fn stored_cards(&self, card_user_key: &str) -> Result<Vec<StoredCard>, Error> {
        let body = wire::CardListRequest {
            locale: "tr",
            card_user_key,
        };
        let (response, _) = self
            .post::<_, wire::CardListResponse>("/cardstorage/cards", &body)
            .await?;
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the card list",
        ) {
            return Err(error);
        }
        Ok(response
            .card_details
            .unwrap_or_default()
            .into_iter()
            .map(StoredCard::from)
            .collect())
    }

    /// Forgets a stored card.
    ///
    /// Both arguments come from [`Client::stored_cards`]; neither is card data.
    /// The token is an [`InstrumentId`](kasapay_core::InstrumentId), so a
    /// payment id cannot be passed here by mistake.
    pub async fn forget_card(
        &self,
        card_user_key: &str,
        card_token: &InstrumentId,
    ) -> Result<(), Error> {
        let body = wire::CardDeleteRequest {
            locale: "tr",
            card_user_key,
            card_token: card_token.as_str(),
        };
        let (response, _) = self
            .delete::<_, wire::BaseResponse>("/cardstorage/card", &body)
            .await?;
        match refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to forget the card",
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Signs and sends one request.
    ///
    /// The body is serialised once and both signed and sent as those exact
    /// bytes. Signing a re-serialised copy signs something the server will not
    /// receive, and the failure looks like bad credentials.
    async fn post<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(T, Raw), Error> {
        self.send(reqwest::Method::POST, path, body).await
    }

    /// Signs and sends one request that carries a body on a DELETE.
    ///
    /// iyzico deletes with a JSON body rather than a path parameter, which is
    /// unusual and is why this cannot go through [`Client::post`].
    async fn delete<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(T, Raw), Error> {
        self.send(reqwest::Method::DELETE, path, body).await
    }

    async fn send<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &B,
    ) -> Result<(T, Raw), Error> {
        self.request(method, path, "", Some(body)).await
    }

    /// Signs and sends one request, body or no body, query or no query.
    ///
    /// `query` is everything from the `?` onwards and is **not signed**: the
    /// signature covers `path` alone. That is what iyzico's own SDKs do —
    /// their PHP client cuts the URL at the `?` before hashing it and their
    /// Python one calls `split('?')[0]` — and the endpoints that take query
    /// parameters are the ones this matters for.
    ///
    /// `None` for the body signs and sends nothing at all, which is what
    /// iyzico's authentication page describes for a request without one.
    /// `Some` is serialised once and both signed and sent as those exact
    /// bytes: signing a re-serialised copy signs something the server will not
    /// receive, and the failure looks like bad credentials.
    pub(crate) async fn request<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &str,
        body: Option<&B>,
    ) -> Result<(T, Raw), Error> {
        let payload = match body {
            Some(body) => serde_json::to_string(body).map_err(|e| {
                Error::new(ErrorKind::InvalidRequest, PROVIDER, "request is not JSON")
                    .with_source(e)
            })?,
            None => String::new(),
        };
        let random_key = random_key();
        let authorization =
            self.inner
                .config
                .credentials
                .authorization(&random_key, path, &payload);

        let url = format!("{}{path}{query}", self.inner.config.base_url);
        let mut request = self
            .inner
            .http
            .request(method, &url)
            .header("Authorization", authorization)
            .header("x-iyzi-rnd", &random_key)
            .header("Content-Type", "application/json");
        if !payload.is_empty() {
            request = request.body(payload);
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
        if !status.is_success() {
            return Err(http_error(status, &text));
        }
        let typed = serde_json::from_slice(&bytes).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "response was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        Ok((typed, Raw::from_text(text)))
    }
}

/// What kind of card a BIN belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardType {
    /// A credit card. The only kind most issuers allow instalments on.
    Credit,
    /// A debit card.
    Debit,
    /// A prepaid card.
    Prepaid,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl fmt::Display for CardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Credit => f.write_str("CREDIT_CARD"),
            Self::Debit => f.write_str("DEBIT_CARD"),
            Self::Prepaid => f.write_str("PREPAID_CARD"),
            Self::Other(name) => f.write_str(name),
        }
    }
}

impl From<&str> for CardType {
    fn from(value: &str) -> Self {
        match value {
            "CREDIT_CARD" => Self::Credit,
            "DEBIT_CARD" => Self::Debit,
            "PREPAID_CARD" => Self::Prepaid,
            other => Self::Other(other.into()),
        }
    }
}

/// The scheme a card runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Association {
    /// Visa.
    Visa,
    /// Mastercard.
    MasterCard,
    /// American Express.
    Amex,
    /// Troy, Türkiye's own scheme.
    Troy,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl fmt::Display for Association {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Visa => f.write_str("VISA"),
            Self::MasterCard => f.write_str("MASTER_CARD"),
            Self::Amex => f.write_str("AMERICAN_EXPRESS"),
            Self::Troy => f.write_str("TROY"),
            Self::Other(name) => f.write_str(name),
        }
    }
}

impl From<&str> for Association {
    fn from(value: &str) -> Self {
        match value {
            "VISA" => Self::Visa,
            "MASTER_CARD" => Self::MasterCard,
            "AMERICAN_EXPRESS" => Self::Amex,
            "TROY" => Self::Troy,
            other => Self::Other(other.into()),
        }
    }
}

fn address_body(address: &crate::classic::checkout::Address) -> wire::AddressBody<'_> {
    wire::AddressBody {
        contact_name: &address.contact_name,
        address: &address.address,
        city: &address.city,
        country: &address.country,
        zip_code: address.zip_code.as_deref(),
    }
}

fn buyer_body(buyer: &crate::classic::checkout::Buyer) -> wire::BuyerBody<'_> {
    wire::BuyerBody {
        id: &buyer.id,
        name: &buyer.name,
        surname: &buyer.surname,
        identity_number: &buyer.identity_number,
        email: &buyer.email,
        gsm_number: &buyer.phone,
        registration_address: &buyer.registration_address,
        city: &buyer.city,
        country: &buyer.country,
        zip_code: buyer.zip_code.as_deref(),
        ip: buyer.ip.as_deref(),
    }
}

fn basket_item_body(item: &crate::classic::checkout::BasketItem) -> wire::BasketItemBody<'_> {
    wire::BasketItemBody {
        id: &item.id,
        name: &item.name,
        category1: &item.category,
        item_type: item.kind.as_str(),
        price: item.price.to_decimal_string(),
    }
}

/// Reads a finished checkout form as a [`Charge`].
///
/// `paymentStatus` is the field that matters, and `status: "success"` only
/// means the query worked — a refused card comes back as a successful query
/// reporting a failure.
///
/// `holds` is the caller saying which form this was: iyzico answers `SUCCESS`
/// for a payment taken and for one only authorised, and its answer does not
/// say which. See [`Client::checkout_result_preauth`].
fn into_payment_charge(
    response: wire::PaymentResultResponse,
    raw: Raw,
    holds: bool,
) -> Result<Charge, Error> {
    let status = match response.payment_status.as_deref() {
        Some("SUCCESS") if holds => Status::Authorized,
        Some("SUCCESS") => Status::Captured,
        Some("FAILURE") => Status::Failed,
        Some("INIT_THREEDS" | "CALLBACK_THREEDS" | "BKM_POS_SELECTED") => Status::RequiresAction,
        // No paymentStatus at all means the payer has not finished.
        _ => Status::Pending,
    };
    charge_from(response, raw, status)
}

/// Reads iyzico's `fraudStatus` the way this crate has always read it.
///
/// iyzico documents 1 as approved, 0 as under review and -1 as rejected, and a
/// payment under review is money not yet taken rather than money taken.
/// `None` — no fraud check ran, or the field was simply absent — is read the
/// same as approved, which is what a stored-card charge means by getting this
/// far at all.
///
/// Shared with [`crate::reporting`], which answers the same three codes about
/// a payment already made rather than one just taken. Its own `paymentStatus`
/// cannot be folded in here the same way — see that module's documentation
/// for why.
pub(crate) const fn fraud_status(value: Option<i64>) -> Status {
    match value {
        Some(0) => Status::Pending,
        Some(-1) => Status::Failed,
        _ => Status::Captured,
    }
}

/// The amounts, the order and the identifier, common to every payment answer.
fn charge_from(
    response: wire::PaymentResultResponse,
    raw: Raw,
    status: Status,
) -> Result<Charge, Error> {
    let currency = response
        .currency
        .as_deref()
        .map_or(Ok(Currency::Try), str::parse)
        .map_err(|e: kasapay_core::UnknownCurrency| {
            Error::new(ErrorKind::Malformed, PROVIDER, e.to_string())
        })?;
    let amount = response
        .paid_price
        .as_deref()
        .map(|price| Money::parse(price, currency))
        .transpose()
        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?
        .unwrap_or_else(|| Money::from_minor_units(0, currency));
    let order_amount = response
        .price
        .as_deref()
        .map(|price| Money::parse(price, currency))
        .transpose()
        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?
        .filter(|basket| *basket != amount);

    Ok(Charge {
        // A form the payer has not finished has no paymentId, and an empty one
        // would be a handle to nothing.
        id: response
            .payment_id
            .filter(|id| !id.is_empty())
            .map(PaymentId::issued),
        order: response.basket_id.map(kasapay_core::OrderRef::new),
        amount,
        order_amount,
        status,
        next_action: None,
        provider: PROVIDER,
        raw,
    })
}

/// Why money went back, and optionally what to say about it.
///
/// iyzico takes a `reason` and a free-text `description` on both refunds and a
/// cancel, with one rule that is easy to get wrong: **a description is only
/// accepted alongside a reason.** So the two are one value rather than two
/// options — a description can only be written onto a `Reason` that already
/// carries a code, and `None` sends neither field.
///
/// ```
/// use kasapay_iyzico::classic::{Reason, ReasonCode};
///
/// let plain = Reason::new(ReasonCode::Fraud);
/// let noted = Reason::new(ReasonCode::BuyerRequest).describe("returned unopened");
///
/// assert_eq!(plain.description(), None);
/// assert_eq!(noted.description(), Some("returned unopened"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    code: ReasonCode,
    description: Option<Box<str>>,
}

impl Reason {
    /// A reason on its own, with nothing said beside it.
    #[must_use]
    pub const fn new(code: ReasonCode) -> Self {
        Self {
            code,
            description: None,
        }
    }

    /// Adds the free text iyzico keeps beside the reason.
    #[must_use]
    pub fn describe(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// What iyzico is told this was.
    #[must_use]
    pub const fn code(&self) -> ReasonCode {
        self.code
    }

    /// The free text, if there is any.
    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}

impl From<ReasonCode> for Reason {
    fn from(code: ReasonCode) -> Self {
        Self::new(code)
    }
}

/// The four reasons iyzico documents for a refund or a cancel.
///
/// Not a label for the shop's own use: this is what a merchant tells their
/// acquirer about why money went back, and it lands in chargeback and
/// reconciliation reporting. A fraudulent order refunded as
/// [`ReasonCode::Other`] has told iyzico nothing, and cannot tell it later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    /// None of the other three.
    Other,
    /// The payment was not the cardholder's.
    Fraud,
    /// The buyer asked for their money back.
    BuyerRequest,
    /// The same money was taken twice.
    DoublePayment,
}

impl ReasonCode {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Other => "OTHER",
            Self::Fraud => "FRAUD",
            Self::BuyerRequest => "BUYER_REQUEST",
            Self::DoublePayment => "DOUBLE_PAYMENT",
        }
    }
}

impl fmt::Display for ReasonCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A reported payment as a [`Charge`], for [`Provider::lookup`].
///
/// The amount is what was collected — `paidPrice` — falling back to the basket
/// total where iyzico sent no collected figure, which is a payment nobody has
/// taken money for yet. A payment with neither is a zero in the currency
/// iyzico named, and it is `None` for the currency too that leaves lira: the
/// classic API settles in one by default and reporting names it on every
/// payment it has, so a missing one is a payment with no amounts to be in a
/// currency at all.
fn detail_into_charge(detail: reporting::PaymentDetail, order: &OrderRef) -> Charge {
    let currency = detail
        .paid_price
        .or(detail.price)
        .map_or(Currency::Try, Money::currency);
    let amount = detail
        .paid_price
        .or(detail.price)
        .unwrap_or_else(|| Money::from_minor_units(0, currency));
    let order_amount = detail.price.filter(|price| *price != amount);
    Charge {
        id: detail.payment_id,
        order: Some(order.clone()),
        amount,
        order_amount,
        status: match detail.payment_status {
            Some(reporting::PaymentStatus::Success) => Status::Captured,
            Some(reporting::PaymentStatus::CallbackThreeDs) => Status::RequiresAction,
            // `2` is a refusal or a payment still at 3-D Secure, and iyzico
            // does not say which; `Pending` is the reading that does not send
            // a second payment after the first one's payer.
            _ => Status::Pending,
        },
        next_action: None,
        provider: PROVIDER,
        raw: detail.raw,
    }
}

/// The shared reason in iyzico's own words.
///
/// Nothing is lost either way: iyzico's fourth code is `OTHER` and it takes a
/// free-text `description` beside it, which is exactly what
/// [`RefundReason::Other`] carries.
fn refund_reason(reason: &RefundReason) -> Reason {
    match reason {
        RefundReason::Duplicate => Reason::new(ReasonCode::DoublePayment),
        RefundReason::Fraudulent => Reason::new(ReasonCode::Fraud),
        RefundReason::RequestedByCustomer => Reason::new(ReasonCode::BuyerRequest),
        RefundReason::Other(words) => Reason::new(ReasonCode::Other).describe(&**words),
    }
}

/// Money taken back off a payment, by a refund or a cancel.
#[derive(Debug, Clone)]
pub struct Reversal {
    /// The payment it came off, as iyzico named it in the answer.
    ///
    /// iyzico documents a `paymentId` on all three reversals, so an answer
    /// shaped the way they document it carries one. `None` is one that did
    /// not, rather than an identifier with nothing in it.
    pub payment: Option<PaymentId>,
    /// How much was taken back.
    pub amount: Money,
    /// The bank's own reference, for reconciling against a statement.
    pub host_reference: Option<Box<str>>,
    /// iyzico's own response, untouched.
    pub raw: Raw,
}

/// A card iyzico holds for a user, named by a token rather than a number.
///
/// Everything here is safe to show somebody so they can pick a card, and none
/// of it is card data. [`StoredCard::token`] plus the `cardUserKey` this was
/// listed under is what [`saved::Card`] takes, and what
/// [`Client::pay_with_saved_card`] charges.
#[derive(Debug, Clone)]
pub struct StoredCard {
    /// What a payment names this card by — iyzico's `cardToken`.
    pub token: InstrumentId,
    /// The name the cardholder gave it — "my Bonus card".
    pub alias: Option<Box<str>>,
    /// The leading digits, which name the issuer.
    pub bin: Option<Box<str>>,
    /// The last four digits, for showing somebody which card this is.
    pub last_four: Option<Box<str>>,
    /// Credit, debit or prepaid.
    pub card_type: Option<CardType>,
    /// The scheme the card runs on.
    pub association: Option<Association>,
    /// The issuer's own name for the product.
    pub family: Option<Box<str>>,
    /// The issuing bank.
    pub bank_name: Option<Box<str>>,
}

impl From<wire::StoredCardItem> for StoredCard {
    fn from(item: wire::StoredCardItem) -> Self {
        Self {
            token: InstrumentId::issued(item.card_token.unwrap_or_default()),
            alias: item.card_alias.map(String::into_boxed_str),
            bin: item.bin_number.map(String::into_boxed_str),
            last_four: item.last_four_digits.map(String::into_boxed_str),
            card_type: item.card_type.as_deref().map(CardType::from),
            association: item.card_association.as_deref().map(Association::from),
            family: item.card_family.map(String::into_boxed_str),
            bank_name: item.card_bank_name.map(String::into_boxed_str),
        }
    }
}

/// What iyzico knows about a BIN.
#[derive(Debug, Clone)]
pub struct BinDetails {
    /// The BIN as iyzico echoed it.
    pub bin: Box<str>,
    /// Credit, debit or prepaid.
    pub card_type: Option<CardType>,
    /// The scheme the card runs on.
    pub association: Option<Association>,
    /// The issuer's own name for the product — `Bonus`, `Axess`, `World`.
    pub family: Option<Box<str>>,
    /// The issuing bank.
    pub bank_name: Option<Box<str>>,
    /// The issuing bank's code.
    pub bank_code: Option<i64>,
    /// Whether the card is a commercial one.
    pub commercial: bool,
    /// iyzico's own response, untouched.
    pub raw: Raw,
}

/// What to show somebody choosing between saved cards: the alias they gave
/// one, or its last four digits, in that order. `None` where iyzico sent
/// neither.
fn instrument_label(alias: Option<&str>, last_four: Option<&str>) -> Option<Box<str>> {
    alias
        .filter(|value| !value.is_empty())
        .map(Into::into)
        .or_else(|| last_four.map(|value| format!("•••• {value}").into()))
}

/// Turns a `status: "failure"` envelope into an error, and anything else into `None`.
///
/// The classic API answers 200 for a refusal and puts the verdict in the body,
/// so the HTTP status is not the thing to read.
pub(crate) fn refused(
    status: Option<&str>,
    message: Option<String>,
    code: Option<String>,
    fallback: &str,
) -> Option<Error> {
    if matches!(status, Some(s) if s.eq_ignore_ascii_case("success")) || status.is_none() {
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

fn into_bin_details(response: wire::BinCheckResponse, raw: Raw) -> Result<BinDetails, Error> {
    if let Some(error) = refused(
        response.status.as_deref(),
        response.error_message,
        response.error_code,
        "iyzico refused the BIN query",
    ) {
        return Err(error);
    }
    Ok(BinDetails {
        bin: response.bin_number.unwrap_or_default().into_boxed_str(),
        card_type: response.card_type.as_deref().map(CardType::from),
        association: response.card_association.as_deref().map(Association::from),
        family: response.card_family.map(String::into_boxed_str),
        bank_name: response.bank_name.map(String::into_boxed_str),
        bank_code: response.bank_code,
        commercial: response.commercial == Some(1),
        raw,
    })
}

/// A value unique to this request, for the signature and the `x-iyzi-rnd` header.
///
/// The clock alone is not enough: two requests in the same nanosecond would
/// share one, so a counter runs alongside it.
fn random_key() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos}{count}")
}

fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PROVIDER, error.to_string())
}

fn http_error(status: reqwest::StatusCode, body: &str) -> Error {
    let kind = match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    };
    Error::new(kind, PROVIDER, format!("HTTP {status}: {body}"))
}

/// The hosted checkout form is not a flow the shared trait can drive.
///
/// Every operation on it answers [`ErrorKind::Unsupported`], for two different
/// reasons. `charge` cannot be honoured because the form needs a buyer's
/// identity number, two addresses and an itemised basket, none of which
/// [`ChargeRequest`] carries and none of which belongs in it. `charge_status`
/// cannot, because what identifies an unfinished form is a [`FormToken`] and
/// this trait names a payment by a [`PaymentId`]. The calls are
/// [`Client::start_checkout_form`] and [`Client::checkout_result`]. What is
/// left of the trait here is [`Provider::id`], [`Provider::instruments`] —
/// the same `/cardstorage/cards` call [`Client::stored_cards`] makes,
/// `customer` being the `cardUserKey` — and [`Provider::capabilities`], all of
/// which a caller can still ask of this client alongside any other.
#[async_trait::async_trait]
impl Provider for Client {
    fn id(&self) -> ProviderId {
        PROVIDER
    }

    async fn charge(&self, _request: &ChargeRequest) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            "the classic API takes a payment through a hosted form, which needs a buyer, \
             two addresses and a basket; call Client::start_checkout_form",
        ))
    }

    /// Reads a payment back by its id, through [`Client::payment`].
    ///
    /// A hosted form the payer has not finished has no payment id yet, and is
    /// read back by the [`FormToken`] it was opened with — that is
    /// [`Client::checkout_result`], and no signature that takes a `PaymentId`
    /// can stand in for it.
    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        self.payment(id).await
    }

    /// Turns a held authorisation into a sale — `POST /payment/postauth`.
    ///
    /// The other half of [`Client::start_checkout_form_preauth`] and
    /// [`Client::preauth_with_saved_card`]. A payment taken by the ordinary
    /// form or by `/payment/auth` has already been captured, and iyzico
    /// answers this call for one with an error rather than taking the money
    /// twice.
    ///
    /// # `amount: None` costs a second request
    ///
    /// iyzico's `paidPrice` is required and it has no request that means "the
    /// lot", so a capture with no amount reads the payment back through
    /// [`Client::payment`] and captures what it says was authorised. Naming
    /// the amount is one call rather than two.
    ///
    /// # A smaller amount is what iyzico's own field is for
    ///
    /// `paidPrice` is documented as *"the final amount to be collected from
    /// the card"* rather than as the authorised one, which is what
    /// [`Capabilities::partial_capture`] rests on here. iyzico does not say in
    /// as many words that a smaller figure is allowed, and no live account has
    /// been asked — see the crate's own note on what is unverified.
    ///
    /// # The currency the answer comes back in
    ///
    /// Read as sent. iyzico's authorisation documents six currencies and this
    /// response's own schema names three, so a capture of a payment authorised
    /// in sterling answers a currency its schema forbids — #88. Refusing it
    /// would lose a capture that has already happened; the amount is real
    /// money either way.
    ///
    /// `idempotency` is [`ErrorKind::Unsupported`] rather than dropped: iyzico
    /// accepts no idempotency mechanism, and a capture sent without the
    /// guarantee the caller asked for can take the money twice.
    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        if idempotency.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PROVIDER,
                "iyzico's classic API accepts no idempotency key; read the payment back \
                 with Provider::charge_status before capturing again",
            ));
        }
        let amount = match amount {
            Some(amount) => amount.require_positive().map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    PROVIDER,
                    "a capture takes an amount above zero, or None for the lot",
                )
                .with_source(e)
            })?,
            None => self.payment(id).await?.amount,
        };
        let body = wire::PostAuthRequest {
            locale: "tr",
            conversation_id: id.as_str(),
            payment_id: id.as_str(),
            paid_price: amount.to_decimal_string(),
            currency: amount.currency().code(),
        };
        let (response, raw) = self
            .post::<_, wire::PaymentResultResponse>(PAYMENT_POSTAUTH, &body)
            .await?;
        self.read_payment_answer(response, raw, false)
    }

    /// Always [`ErrorKind::Unsupported`]: a void answers a `Reversal`, not a charge.
    ///
    /// [`Client::cancel`] is the call, and what it returns is what iyzico
    /// signs — or does not sign, which is the reason it cannot be flattened
    /// into a [`Charge`] here.
    async fn cancel(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            "voiding a classic payment answers a Reversal rather than a charge; \
             call Client::cancel",
        ))
    }

    /// Gives money back off a payment, through [`Client::refund`].
    ///
    /// `/v2/payment/refund`, which refunds against the payment as a whole.
    /// **Not for a basket with more than one line** — iyzico picks which line
    /// the money comes off, and [`Client::refund_transaction`] is the call
    /// that names it. That call takes a `paymentTransactionId`, which is not a
    /// [`PaymentId`] and has no place in this signature, so per-line refunds
    /// stay this crate's own.
    ///
    /// # `amount: None` is refused
    ///
    /// iyzico's refund takes `price` and has no form that means "the rest".
    /// Reading the payment back to work the figure out would answer what was
    /// *paid* rather than what is still refundable — every earlier refund is
    /// missing from it — so a full refund after a partial one would ask for
    /// money that has already gone. [`ErrorKind::InvalidRequest`], and the
    /// caller names the amount.
    ///
    /// # The reason reaches iyzico
    ///
    /// All four of [`RefundReason`]'s arms map onto
    /// [`ReasonCode`]: `Duplicate` is `DOUBLE_PAYMENT`, `Fraudulent` is
    /// `FRAUD`, `RequestedByCustomer` is `BUYER_REQUEST`, and `Other` is
    /// `OTHER` with the caller's own words in the `description` iyzico keeps
    /// beside it. This is the acquirer-facing field, not a note to the payer
    /// — see [`ReasonCode`] for why sending it is worth doing.
    ///
    /// # An idempotency key is refused
    ///
    /// iyzico accepts no idempotency mechanism on any classic call, so a
    /// refund carrying [`RefundRequest::idempotency_key`] is
    /// [`ErrorKind::Unsupported`] rather than the same request sent without
    /// the guarantee that was asked for. Read the payment back before sending
    /// another; #54 is the open question about what a replay actually does.
    ///
    /// # The answer is a refund iyzico has already made
    ///
    /// [`RefundStatus::Succeeded`] always. `/v2/payment/refund` answers a
    /// refund it accepted — there is no queued state and nothing to poll —
    /// and a refusal is an [`Error`] rather than a failed [`Refund`].
    /// [`Refund::id`] is `None`: iyzico issues no identifier for one, and the
    /// bank reference it does answer is on
    /// [`Reversal::host_reference`], readable from [`Refund::raw`].
    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error> {
        if request.idempotency_key.is_some() {
            return Err(Error::new(
                ErrorKind::Unsupported,
                PROVIDER,
                "iyzico's classic API accepts no idempotency key; read the payment back \
                 with Provider::charge_status before sending a refund again",
            ));
        }
        let amount = request.amount.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "iyzico's refund takes the amount to give back and has no form that means \
                 the rest of it; name the amount",
            )
        })?;
        let reason = request.reason.as_ref().map(refund_reason);
        let reversal = Client::refund(self, &request.payment, amount, reason.as_ref()).await?;
        Ok(Refund {
            id: None,
            payment: reversal.payment.unwrap_or_else(|| request.payment.clone()),
            amount: reversal.amount,
            status: RefundStatus::Succeeded,
            next_action: None,
            provider: PROVIDER,
            raw: reversal.raw,
        })
    }

    /// Asks iyzico what became of a payment made under this `conversationId`.
    ///
    /// `GET /v2/reporting/payment/details` with `paymentConversationId`, which
    /// iyzico documents as the alternative to a `paymentId` — and the
    /// `conversationId` a checkout form is opened with is
    /// [`CheckoutForm::order`], the caller's own reference. So this answers
    /// the one question a caller whose request timed out can still ask.
    ///
    /// Reporting rather than `/payment/detail`, which takes the same field,
    /// for one reason: it answers a **list**, and a list with nothing in it is
    /// an answer. `/payment/detail` answers a payment or a refusal, and
    /// iyzico documents no error code for a `conversationId` it has never
    /// seen — so "no record" and "something else went wrong" would arrive
    /// identically, and reading the second as the first is how a caller
    /// charges twice.
    ///
    /// # A refused payment and one still at 3-D Secure read the same
    ///
    /// iyzico's `paymentStatus` is `2` for both — its own documentation does
    /// not separate them, which is what
    /// [`reporting::PaymentStatus::FailureOrInitThreeDs`] is named for. That
    /// arrives here as [`Status::Pending`] rather than
    /// [`Status::Failed`]: a payment mid-3-D-Secure read as failed is one a
    /// caller sends again while the payer is still on the bank's page, and
    /// two payments is worse than one poll too many.
    ///
    /// # Unverified against a live account
    ///
    /// That an unknown `conversationId` answers an empty list rather than a
    /// refusal is iyzico's documented shape rather than something seen. See
    /// #102 — it is one call from a sandbox account.
    async fn lookup(&self, order: &OrderRef) -> Result<Option<Charge>, Error> {
        let found = reporting::Client::new(self.clone())
            .payment_details(&reporting::PaymentQuery::Conversation(
                order.as_str().into(),
            ))
            .await?;
        let Some(detail) = found.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(detail_into_charge(detail, order)))
    }

    /// Lists the cards stored under a `cardUserKey`.
    ///
    /// `customer` is iyzico's `cardUserKey` — the vault, not a payer as such —
    /// same as it is everywhere else a saved card is named here. Written
    /// separately from [`Client::stored_cards`] rather than through it: this
    /// keeps the per-card JSON `cardDetails` carries so
    /// [`Instrument::raw`](kasapay_core::Instrument::raw) is that card's own
    /// object rather than nothing, which the typed [`StoredCard`] does not
    /// keep.
    async fn instruments(&self, customer: &str) -> Result<Vec<Instrument>, Error> {
        let body = wire::CardListRequest {
            locale: "tr",
            card_user_key: customer,
        };
        let (response, raw) = self
            .post::<_, wire::CardListResponse>("/cardstorage/cards", &body)
            .await?;
        if let Some(error) = refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the card list",
        ) {
            return Err(error);
        }
        let items: Vec<serde_json::Value> = raw
            .json()
            .and_then(|value| value.get("cardDetails").and_then(|v| v.as_array().cloned()))
            .unwrap_or_default();
        Ok(response
            .card_details
            .unwrap_or_default()
            .into_iter()
            .zip(items)
            .map(|(item, item_raw)| Instrument {
                id: InstrumentId::issued(item.card_token.unwrap_or_default()),
                label: instrument_label(
                    item.card_alias.as_deref(),
                    item.last_four_digits.as_deref(),
                ),
                raw: Raw::from_json(&item_raw),
            })
            .collect())
    }

    /// Holds funds when asked to, refunds the way iyzico documents them, and
    /// charges a card iyzico holds.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // About the API rather than about a particular payment: iyzico
            // will hold funds, and Client::start_checkout_form_preauth and
            // Client::preauth_with_saved_card are what ask it to. A payment
            // opened by the ordinary form is captured as it goes, and
            // capturing it afterwards fails.
            separate_capture: true,
            // On `paidPrice` being documented as the final amount to collect
            // rather than the authorised one. See Provider::capture.
            partial_capture: true,
            partial_refund: true,
            // Documented rather than assumed: a refund must not exceed the
            // amount still refundable, and "as long as that rule is followed,
            // more than one refund may be made in succession".
            repeated_refund: true,
            // Reporting reads a payment back by the conversationId it was
            // made with, which is the caller's own order reference.
            lookup_by_order: true,
            // Client::stored_cards lists them and Client::pay_with_saved_card
            // charges one.
            saved_instruments: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Association, CardType, random_key};

    #[test]
    fn card_types_iyzico_documents_are_named_and_the_rest_are_kept() {
        assert_eq!(CardType::from("CREDIT_CARD"), CardType::Credit);
        assert_eq!(CardType::from("DEBIT_CARD"), CardType::Debit);
        assert_eq!(CardType::from("PREPAID_CARD"), CardType::Prepaid);
        let unknown = CardType::from("VIRTUAL_CARD");
        assert_eq!(unknown, CardType::Other("VIRTUAL_CARD".into()));
        assert_eq!(unknown.to_string(), "VIRTUAL_CARD");
    }

    #[test]
    fn every_named_card_type_renders_back_to_what_iyzico_sent() {
        for name in ["CREDIT_CARD", "DEBIT_CARD", "PREPAID_CARD"] {
            assert_eq!(CardType::from(name).to_string(), name);
        }
        for name in ["VISA", "MASTER_CARD", "AMERICAN_EXPRESS", "TROY"] {
            assert_eq!(Association::from(name).to_string(), name);
        }
    }

    #[test]
    fn two_random_keys_in_a_row_differ() {
        // Two requests in the same nanosecond must not share a key.
        assert_ne!(random_key(), random_key());
    }
}
