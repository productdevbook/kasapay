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

/// The body `POST .../authorize` takes. Every one of PayPal's own documented
/// examples for this operation sends `{}` — `payment_source` only matters
/// when the order was not already approved through PayPal's own checkout,
/// which is the one flow this crate opens.
#[derive(Debug, serde::Serialize, Default)]
pub(crate) struct AuthorizeOrder {}

/// `POST /v2/payments/authorizations/{id}/capture`'s body. `amount` absent
/// takes the whole authorization; PayPal's own `authorizations_capture_empty_request`
/// example sends `{}` for that.
#[derive(Debug, serde::Serialize, Default)]
pub(crate) struct CreateAuthorizationCapture {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<AmountOut>,
}

/// What `POST /v2/payments/authorizations/{id}/capture` answers: a capture
/// on its own, not nested in an order the way [`Capture`] is. PayPal's own
/// documented examples for this call carry no order id and no authorization
/// id in the body at all — only in `links[].href`, under `rel: "up"`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct AuthorizationCapture {
    pub id: Option<String>,
    pub status: Option<String>,
    pub amount: Option<AmountIn>,
}

/// `POST /v2/payments/captures/{id}/refund`'s body. `amount` absent refunds
/// the capture in full; PayPal's own `captures_refund_empty_request` example
/// sends `{}` for that.
#[derive(Debug, serde::Serialize, Default)]
pub(crate) struct CreateRefund {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<AmountOut>,
}

/// What `POST /v2/payments/captures/{id}/refund` answers. Carries no capture
/// id of its own in the body either — the same gap [`AuthorizationCapture`]
/// has, and for the same reason: PayPal's `up` link names it, not a field.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Refund {
    pub id: Option<String>,
    pub status: Option<String>,
    pub amount: Option<AmountIn>,
}

/// One of the `_links` entries: `{"href": …, "rel": …, "method": …}`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Link {
    pub href: Option<String>,
    pub rel: Option<String>,
}

// Each carries its own `id` too — `3C679366HH908993F` for a capture, a
// different alphabet for an authorization — which is not a field on either
// struct below. [`crate::CaptureId`] and [`crate::AuthorizationId`] are what
// name the two now that something here calls another endpoint with each —
// [`crate::PayPal::refund`] against the capture,
// [`crate::PayPal::capture_authorization`] against the authorization — and
// [`crate::capture_id`] and [`crate::authorization_id`] read one out of
// [`Charge::raw`](kasapay_core::Charge::raw) by its JSON pointer directly,
// the same way this crate already reads an order's amount off the raw shape
// without a struct field for the id.
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
