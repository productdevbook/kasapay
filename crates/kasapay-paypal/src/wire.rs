//! PayPal's JSON, exactly as their OpenAPI document describes it.
//!
//! Every field is optional on the way in, the same rule `kasapay-mollie`
//! follows: a payments library that stops parsing because one documented
//! field is missing loses the rest of the answer too. What this crate cannot
//! do without is checked where it is read, in `client.rs`.

/// `{"currency_code": "EUR", "value": "10.00"}` on the way out.
#[derive(Debug, serde::Serialize)]
pub(crate) struct AmountOut {
    pub currency_code: &'static str,
    pub value: String,
}

/// The same object on the way in.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AmountIn {
    pub currency_code: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct PurchaseUnitOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    pub amount: AmountOut,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ExperienceContext<'a> {
    pub return_url: &'a str,
    pub cancel_url: &'a str,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct PayPalWalletOut<'a> {
    pub experience_context: ExperienceContext<'a>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct PaymentSourceOut<'a> {
    pub paypal: PayPalWalletOut<'a>,
}

/// `POST /v2/checkout/orders`'s body.
///
/// `intent` is required by PayPal's `order_request` schema, even though the
/// "Create Order - Minimal Request and Response" example in their own
/// document omits it — see the crate docs for where else that document
/// disagrees with itself.
#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateOrder<'a> {
    pub intent: &'static str,
    pub purchase_units: [PurchaseUnitOut<'a>; 1],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_source: Option<PaymentSourceOut<'a>>,
}

/// The body `POST .../capture` takes when nothing more than a capture is
/// asked for — no `payment_source`, no amount, nothing. PayPal's own
/// documented example sends `{}`.
#[derive(Debug, serde::Serialize, Default)]
pub(crate) struct CaptureOrder {}

/// One of the `_links` entries: `{"href": …, "rel": …, "method": …}`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Link {
    pub href: Option<String>,
    pub rel: Option<String>,
}

// Each carries its own `id` too — `3C679366HH908993F` for a capture, a
// different alphabet for an authorization — which this crate does not model
// as its own kind of [`Id`](kasapay_core::Id) because nothing here reads one
// back by it: unlike Mollie's capture, PayPal's is not a resource this crate
// calls anything else with. It is still on [`Charge::raw`](kasapay_core::Charge::raw)
// for a caller who wants it.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Capture {
    pub status: Option<String>,
    pub amount: Option<AmountIn>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct Authorization {
    pub status: Option<String>,
    pub amount: Option<AmountIn>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub(crate) struct Payments {
    #[serde(default)]
    pub captures: Vec<Capture>,
    #[serde(default)]
    pub authorizations: Vec<Authorization>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct PurchaseUnit {
    pub custom_id: Option<String>,
    pub amount: Option<AmountIn>,
    pub payments: Option<Payments>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct Order {
    pub id: Option<String>,
    /// Absent from every example in PayPal's own OpenAPI document, on every
    /// operation this crate calls — see the crate docs.
    pub status: Option<String>,
    #[serde(default)]
    pub purchase_units: Vec<PurchaseUnit>,
    #[serde(default)]
    pub links: Vec<Link>,
}

/// `POST /v1/oauth2/token`'s form body.
#[derive(Debug, serde::Serialize)]
pub(crate) struct ClientCredentialsGrant {
    pub grant_type: &'static str,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: Option<String>,
    pub expires_in: Option<u64>,
}

/// The plain OAuth2 error shape `/v1/oauth2/token` answers — `error` and
/// `error_description`, nothing else — which is not PayPal's own `error`
/// object every other endpoint in this crate answers with.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct OAuthError {
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ErrorDetail {
    pub description: Option<String>,
    pub field: Option<String>,
}

/// PayPal's standard error object, on every REST endpoint but the OAuth2 one.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ErrorBody {
    pub name: Option<String>,
    pub message: Option<String>,
    pub debug_id: Option<String>,
    #[serde(default)]
    pub details: Vec<ErrorDetail>,
}
