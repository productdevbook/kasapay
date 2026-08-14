//! The classic API client.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kasapay_core::{
    Charge, Currency, Error, ErrorKind, Money, NextAction, PaymentId, ProviderId, Raw, Status,
};
use url::Url;

use crate::classic::checkout::CheckoutForm;
use crate::classic::wire;
use crate::signing::Credentials;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where the classic client points and what it signs with.
#[derive(Debug, Clone)]
pub struct Config {
    /// Held as text, not as a `Url`: `Url`'s own rendering puts a trailing
    /// slash back on a bare authority, and `{base}{path}` then signs
    /// `//payment/bin/check` while the server sees `/payment/bin/check`.
    base_url: Box<str>,
    credentials: Credentials,
    timeout: Duration,
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
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
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
    pub async fn start_checkout_form(&self, form: &CheckoutForm) -> Result<Charge, Error> {
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
            buyer: wire::BuyerBody {
                id: &form.buyer.id,
                name: &form.buyer.name,
                surname: &form.buyer.surname,
                identity_number: &form.buyer.identity_number,
                email: &form.buyer.email,
                gsm_number: &form.buyer.phone,
                registration_address: &form.buyer.registration_address,
                city: &form.buyer.city,
                country: &form.buyer.country,
                zip_code: form.buyer.zip_code.as_deref(),
                ip: form.buyer.ip.as_deref(),
            },
            billing_address: address_body(&form.billing_address),
            shipping_address: address_body(&form.shipping_address),
            basket_items: form
                .basket
                .iter()
                .map(|item| wire::BasketItemBody {
                    id: &item.id,
                    name: &item.name,
                    category1: &item.category,
                    item_type: item.kind.as_str(),
                    price: item.price.to_decimal_string(),
                })
                .collect(),
        };

        let (response, raw) = self
            .post::<_, wire::CheckoutFormResponse>(
                "/payment/iyzipos/checkoutform/initialize/auth/ecom",
                &body,
            )
            .await?;
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

        Ok(Charge {
            // iyzico has no payment id until the payer is done; the form's
            // token is what identifies it until then.
            id: PaymentId::new(token.as_str()),
            order: Some(form.order.clone()),
            amount: form.paid_price,
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
    pub async fn checkout_result(&self, token: &str) -> Result<Charge, Error> {
        let body = wire::CheckoutResultRequest {
            locale: "tr",
            token,
        };
        let (response, raw) = self
            .post::<_, wire::CheckoutResultResponse>(
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
        into_checkout_charge(response, raw)
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
    pub async fn forget_card(&self, card_user_key: &str, card_token: &str) -> Result<(), Error> {
        let body = wire::CardDeleteRequest {
            locale: "tr",
            card_user_key,
            card_token,
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
        let payload = serde_json::to_string(body).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PROVIDER, "request is not JSON").with_source(e)
        })?;
        let random_key = random_key();
        let authorization =
            self.inner
                .config
                .credentials
                .authorization(&random_key, path, &payload);

        let url = format!("{}{path}", self.inner.config.base_url);
        let response = self
            .inner
            .http
            .request(method, &url)
            .header("Authorization", authorization)
            .header("x-iyzi-rnd", &random_key)
            .header("Content-Type", "application/json")
            .body(payload)
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

/// Reads a finished checkout form as a [`Charge`].
///
/// `paymentStatus` is the field that matters, and `status: "success"` only
/// means the query worked — a refused card comes back as a successful query
/// reporting a failure.
fn into_checkout_charge(response: wire::CheckoutResultResponse, raw: Raw) -> Result<Charge, Error> {
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

    let status = match response.payment_status.as_deref() {
        Some("SUCCESS") => Status::Captured,
        Some("FAILURE") => Status::Failed,
        Some("INIT_THREEDS" | "CALLBACK_THREEDS" | "BKM_POS_SELECTED") => Status::RequiresAction,
        // No paymentStatus at all means the payer has not finished.
        _ => Status::Pending,
    };

    Ok(Charge {
        id: PaymentId::new(response.payment_id.unwrap_or_default()),
        order: response.basket_id.map(kasapay_core::OrderRef::new),
        amount,
        status,
        next_action: None,
        provider: PROVIDER,
        raw,
    })
}

/// A card iyzico holds for a user, named by a token rather than a number.
#[derive(Debug, Clone)]
pub struct StoredCard {
    /// What a payment names this card by.
    pub token: Box<str>,
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
            token: item.card_token.unwrap_or_default().into_boxed_str(),
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

/// Turns a `status: "failure"` envelope into an error, and anything else into `None`.
///
/// The classic API answers 200 for a refusal and puts the verdict in the body,
/// so the HTTP status is not the thing to read.
fn refused(
    status: Option<&str>,
    message: Option<String>,
    code: Option<String>,
    fallback: &str,
) -> Option<Error> {
    if matches!(status, Some(s) if s.eq_ignore_ascii_case("success")) || status.is_none() {
        return None;
    }
    let error = Error::new(
        ErrorKind::InvalidRequest,
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
