//! The request and response bodies of the In-Store API, as it sends them.
//!
//! Field names are iyzico's, so this module is the only one shouting in
//! `camelCase`. Everything it produces is turned into `kasapay-core` types by
//! [`crate::client`] before it leaves the crate.

use serde::{Deserialize, Serialize};

/// `POST /payment/init`.
#[derive(Debug, Serialize)]
pub(crate) struct PaymentInitRequest<'a> {
    #[serde(rename = "userId")]
    pub(crate) user_id: &'a str,
    #[serde(rename = "orderId")]
    pub(crate) order_id: &'a str,
    /// A bare JSON number written from an exact decimal string.
    ///
    /// The spec types this `BigDecimal`; sending it through `f64` would put
    /// 10.10 on the wire as 10.100000000000001.
    pub(crate) amount: Box<serde_json::value::RawValue>,
}

/// `POST /payment/refund`.
#[derive(Debug, Serialize)]
pub(crate) struct RefundRequest<'a> {
    #[serde(rename = "userId")]
    pub(crate) user_id: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: i64,
    #[serde(rename = "refundAmount", skip_serializing_if = "Option::is_none")]
    pub(crate) refund_amount: Option<Box<serde_json::value::RawValue>>,
}

/// The answer to `/payment/init` and `/payment/refund`.
#[derive(Debug, Deserialize)]
pub(crate) struct SessionResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "deepLinkUrl")]
    pub(crate) deep_link_url: Option<String>,
    #[serde(rename = "paymentSessionToken")]
    pub(crate) payment_session_token: Option<String>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<i64>,
}

/// The answer to `GET /payment/query`.
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentQueryResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<i64>,
    #[serde(rename = "orderId")]
    pub(crate) order_id: Option<String>,
    #[serde(rename = "transactionDetail")]
    pub(crate) transaction_detail: Option<TransactionDetail>,
}

/// The bank-side detail hanging off a queried payment.
#[derive(Debug, Deserialize)]
pub(crate) struct TransactionDetail {
    /// The amount, as a JSON number the API types `BigDecimal`.
    pub(crate) amount: Option<serde_json::Number>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    pub(crate) receipt: Option<Receipt>,
    #[serde(rename = "isRefundable")]
    pub(crate) is_refundable: Option<bool>,
}

/// The printable receipt, which is also where the approval flag lives.
#[derive(Debug, Deserialize)]
pub(crate) struct Receipt {
    pub(crate) approved: Option<bool>,
}

/// The failure body, shared by every endpoint.
#[derive(Debug, Deserialize)]
pub(crate) struct ErrorResponse {
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}
