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
    /// The partial amount, under the name the prose gives it.
    ///
    /// iyzico contradicts itself about what this field is called, so it goes
    /// out under both names — see [`refund_price`](Self::refund_price).
    #[serde(rename = "refundAmount", skip_serializing_if = "Option::is_none")]
    pub(crate) refund_amount: Option<Box<serde_json::value::RawValue>>,
    /// The same amount, under the name the schema gives it.
    ///
    /// Two of iyzico's own sources disagree, and they disagree **on the same
    /// page**: the prose on the cancel-and-refund page says `refundAmount`,
    /// and the OpenAPI fragment embedded in that page says `refundPrice`. The
    /// In-Store overview page's fragment says `refundAmount` as well.
    ///
    /// Sending only the wrong one is not a validation error: the field is
    /// optional, so an unrecognised name is ignored and iyzico refunds **the
    /// whole payment** where a part was asked for. Sending both means whichever
    /// name is real carries the amount, and a server strict enough to reject
    /// the other answers with an error rather than over-refunding.
    #[serde(rename = "refundPrice", skip_serializing_if = "Option::is_none")]
    pub(crate) refund_price: Option<Box<serde_json::value::RawValue>>,
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

/// `POST /crypt/decrypt`.
#[derive(Debug, Serialize)]
pub(crate) struct DecryptRequest<'a> {
    pub(crate) data: &'a str,
    #[serde(rename = "paymentSessionToken")]
    pub(crate) payment_session_token: &'a str,
}

/// The answer to `POST /crypt/decrypt`.
#[derive(Debug, Deserialize)]
pub(crate) struct DecryptResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "inStoreCompleteOperation")]
    pub(crate) operation: Option<CompleteOperation>,
}

/// The settled operation hanging off a decrypted callback.
#[derive(Debug, Deserialize)]
pub(crate) struct CompleteOperation {
    pub(crate) transaction: Option<SettledTransaction>,
}

/// What the bank did, as the callback reports it.
#[derive(Debug, Deserialize)]
pub(crate) struct SettledTransaction {
    /// The amount, typed `BigDecimal` by the spec and sent as a JSON number.
    pub(crate) amount: Option<serde_json::Number>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    pub(crate) receipt: Option<SettledReceipt>,
}

/// The receipt, whose three approval flags are the only thing that says which
/// operation the callback is reporting on.
#[derive(Debug, Deserialize)]
pub(crate) struct SettledReceipt {
    pub(crate) approved: Option<bool>,
    #[serde(rename = "refundApproved")]
    pub(crate) refund_approved: Option<bool>,
    #[serde(rename = "voidApproved")]
    pub(crate) void_approved: Option<bool>,
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

/// `POST /user` and `DELETE /user`.
#[derive(Debug, Serialize)]
pub(crate) struct UserRequest<'a> {
    #[serde(rename = "userId")]
    pub(crate) user_id: &'a str,
}

/// The answer to `POST /user`, and one entry of the list.
#[derive(Debug, Deserialize)]
pub(crate) struct UserDetail {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "userId")]
    pub(crate) user_id: Option<String>,
    #[serde(default)]
    pub(crate) enrollments: Vec<EnrollmentItem>,
}

/// The answer to `GET /user/list`.
#[derive(Debug, Deserialize)]
pub(crate) struct UserListResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "userList", default)]
    pub(crate) user_list: Vec<UserDetail>,
}

/// A bank a till user is registered with.
#[derive(Debug, Deserialize)]
pub(crate) struct EnrollmentItem {
    #[serde(rename = "enrolledBank")]
    pub(crate) enrolled_bank: Option<String>,
    #[serde(rename = "enrolledTerminalId")]
    pub(crate) enrolled_terminal_id: Option<String>,
    #[serde(rename = "enrollmentStatus")]
    pub(crate) enrollment_status: Option<String>,
}

/// The answer to `DELETE /user`.
#[derive(Debug, Deserialize)]
pub(crate) struct DeleteUserResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "userId")]
    pub(crate) user_id: Option<String>,
}
