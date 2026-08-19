//! The request and response bodies of the Terminal API, as iyzico documents them.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// `POST /in-store/oauth2/authorize`, form-urlencoded.
///
/// The only call in this crate that puts a secret in a request body: the
/// client secret goes here and again in the Basic header of every later token
/// call.
#[derive(Debug, Serialize)]
pub(crate) struct AuthorizeRequest<'a> {
    pub(crate) scope: &'a str,
    pub(crate) client_id: &'a str,
    pub(crate) client_secret: &'a str,
    pub(crate) response_type: &'a str,
    pub(crate) username: &'a str,
    pub(crate) password: &'a str,
    pub(crate) request_timestamp: String,
}

/// The auth code, and how long it lives.
#[derive(Debug, Deserialize)]
pub(crate) struct AuthorizeResponse {
    pub(crate) code: Option<String>,
    #[serde(rename = "issuedAt")]
    pub(crate) issued_at: Option<String>,
    #[serde(rename = "expiredAt")]
    pub(crate) expired_at: Option<String>,
}

/// What `/authorize` answers when it refuses.
///
/// It also documents a `uri`, "returned in some cases", with no word on what it
/// points at. Nothing here reads it.
#[derive(Debug, Deserialize)]
pub(crate) struct AuthorizeError {
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    pub(crate) description: Option<String>,
}

/// `POST /in-store/oauth2/token` with `grant_type=authorization_code`.
#[derive(Debug, Serialize)]
pub(crate) struct TokenByCode<'a> {
    pub(crate) grant_type: &'a str,
    pub(crate) code: &'a str,
}

/// `POST /in-store/oauth2/token/refresh` with `grant_type=refresh_token`.
#[derive(Debug, Serialize)]
pub(crate) struct TokenByRefresh<'a> {
    pub(crate) grant_type: &'a str,
    pub(crate) refresh_token: &'a str,
}

/// What both token services answer.
#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) scope: Option<String>,
    pub(crate) token_type: Option<String>,
    pub(crate) expires_in: Option<u64>,
}

/// What both token services answer when they refuse: one field, no code.
#[derive(Debug, Deserialize)]
pub(crate) struct OAuthError {
    pub(crate) error: Option<String>,
}

/// `POST /v2/terminal-host/payment`.
///
/// `salesType` and not `saleType`: the OpenAPI fragment spells it with the `s`
/// in both documentation languages, and only the hand-written sample on the
/// overview page spells it without. See the module docs.
#[derive(Debug, Serialize)]
pub(crate) struct PaymentRequest<'a> {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) locale: &'a str,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    /// Typed `double`. Written as the decimal it is, never through a float.
    pub(crate) price: Box<RawValue>,
    pub(crate) currency: &'a str,
    #[serde(rename = "salesType")]
    pub(crate) sales_type: &'a str,
    #[serde(rename = "paymentId", skip_serializing_if = "Option::is_none")]
    pub(crate) payment_id: Option<&'a str>,
    pub(crate) installment: u8,
}

/// `POST /v2/terminal-host/payment/query-transaction-status`.
///
/// Three of the five fields are optional here although the schema marks them
/// required; iyzico's own note beside it says which combinations work.
#[derive(Debug, Serialize)]
pub(crate) struct QueryRequest<'a> {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) locale: &'a str,
    #[serde(rename = "paymentId", skip_serializing_if = "Option::is_none")]
    pub(crate) payment_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId", skip_serializing_if = "Option::is_none")]
    pub(crate) device_unique_id: Option<&'a str>,
    #[serde(
        rename = "transactionReferenceId",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) transaction_reference_id: Option<&'a str>,
}

/// `POST /v2/terminal-host/payment/refund`.
#[derive(Debug, Serialize)]
pub(crate) struct RefundRequest<'a> {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) locale: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    pub(crate) price: Box<RawValue>,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// `POST /v2/terminal-host/payment/void`.
#[derive(Debug, Serialize)]
pub(crate) struct VoidRequest<'a> {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) locale: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: &'a str,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// What all four Terminal Host operations answer, refused or not.
///
/// One struct for both shapes iyzico names. `TerminalPaymentSuccessResponse`
/// and `TerminalFailureResponse` share five fields — the failure adds
/// `consumerErrorMessage` and drops the rest — so a body that is either reads
/// here, and a refusal arriving with HTTP 200 is not mistaken for a payment.
///
/// One documented field is missing: `locale`, which echoes the language the
/// request asked for. Nothing here reads it, and a caller who wants it has it
/// in [`Payment::raw`](crate::terminal::Payment::raw).
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentResponse {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: Option<String>,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: Option<String>,
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "errorGroup")]
    pub(crate) error_group: Option<String>,
    #[serde(rename = "consumerErrorMessage")]
    pub(crate) consumer_error_message: Option<String>,
    #[serde(rename = "systemTime")]
    pub(crate) system_time: Option<i64>,
    #[serde(rename = "transactionDateTime")]
    pub(crate) transaction_date_time: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: Option<String>,
    /// Typed `double`, and kept as iyzico's own bytes: a `double` read into an
    /// `f64` and printed back is not reliably the amount that was sent.
    pub(crate) price: Option<Box<RawValue>>,
    pub(crate) installment: Option<i32>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "binNumber")]
    pub(crate) bin_number: Option<String>,
    #[serde(rename = "lastFourDigits")]
    pub(crate) last_four_digits: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
    #[serde(rename = "cardType")]
    pub(crate) card_type: Option<String>,
    #[serde(rename = "acquirerId")]
    pub(crate) acquirer_id: Option<String>,
    #[serde(rename = "issuerId")]
    pub(crate) issuer_id: Option<String>,
    #[serde(rename = "bankMerchantId")]
    pub(crate) bank_merchant_id: Option<String>,
    #[serde(rename = "bankTerminalId")]
    pub(crate) bank_terminal_id: Option<String>,
    #[serde(rename = "batchNo")]
    pub(crate) batch_no: Option<String>,
    #[serde(rename = "stanNo")]
    pub(crate) stan_no: Option<String>,
    #[serde(rename = "posEntryModeCode")]
    pub(crate) pos_entry_mode_code: Option<String>,
    #[serde(rename = "cancelHostReference")]
    pub(crate) cancel_host_reference: Option<String>,
    #[serde(rename = "refundHostReference")]
    pub(crate) refund_host_reference: Option<String>,
}

/// A value as iyzico wrote it, whether that was `100.0` or `"100.0"`.
pub(crate) fn text(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}

/// `POST /v2/terminal-host/eod` — closing the day's batch on one device.
#[derive(Debug, Serialize)]
pub(crate) struct EndOfDayRequest<'a> {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) locale: &'a str,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "useSummary")]
    pub(crate) use_summary: bool,
}

/// What that answers. The error fields are read through
/// [`PaymentResponse`] like every other operation's; these are the ones only
/// an end of day carries.
#[derive(Debug, Deserialize)]
pub(crate) struct EndOfDayResponse {
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(rename = "batchNo")]
    pub(crate) batch_no: Option<String>,
    #[serde(rename = "resultMessage")]
    pub(crate) result_message: Option<String>,
    pub(crate) totals: Option<Vec<EndOfDayTotal>>,
}

/// One acquiring bank's line on the batch. Every figure is a string, which is
/// how iyzico types them.
#[derive(Debug, Deserialize)]
pub(crate) struct EndOfDayTotal {
    #[serde(rename = "acquirerId")]
    pub(crate) acquirer_id: Option<String>,
    #[serde(rename = "acquirerName")]
    pub(crate) acquirer_name: Option<String>,
    #[serde(rename = "terminalId")]
    pub(crate) terminal_id: Option<String>,
    #[serde(rename = "bankMerchantId")]
    pub(crate) bank_merchant_id: Option<String>,
    #[serde(rename = "batchNo")]
    pub(crate) batch_no: Option<String>,
    #[serde(rename = "totalTransactionAmount")]
    pub(crate) total_transaction_amount: Option<String>,
    #[serde(rename = "totalTransactionCount")]
    pub(crate) total_transaction_count: Option<String>,
    #[serde(rename = "responseCode")]
    pub(crate) response_code: Option<String>,
}

/// `POST /v2/terminal-host/gmu/payment` and the partial payment's own start.
#[derive(Debug, Serialize)]
pub(crate) struct GmuPaymentRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    /// A decimal string here, where VUK 509 types the same figure as a number.
    pub(crate) price: String,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: String,
    #[serde(rename = "paymentType")]
    pub(crate) payment_type: &'a str,
    pub(crate) currency: &'a str,
    pub(crate) installment: u8,
    #[serde(rename = "saleAppName")]
    pub(crate) sale_app_name: &'a str,
    #[serde(rename = "saleAppVersion")]
    pub(crate) sale_app_version: &'a str,
    #[serde(rename = "saleDocumentType")]
    pub(crate) sale_document_type: u8,
    #[serde(rename = "notificationPhone", skip_serializing_if = "Option::is_none")]
    pub(crate) notification_phone: Option<&'a str>,
    #[serde(rename = "notificationEmail", skip_serializing_if = "Option::is_none")]
    pub(crate) notification_email: Option<&'a str>,
    #[serde(rename = "saleItems")]
    pub(crate) sale_items: Vec<GmuSaleItem<'a>>,
    #[serde(rename = "buyerInfo", skip_serializing_if = "Option::is_none")]
    pub(crate) buyer_info: Option<GmuBuyer<'a>>,
}

/// One line of a VUK 507 document.
#[derive(Debug, Serialize)]
pub(crate) struct GmuSaleItem<'a> {
    pub(crate) name: &'a str,
    pub(crate) generic: bool,
    #[serde(rename = "unitCode")]
    pub(crate) unit_code: &'a str,
    #[serde(rename = "taxGroupCode")]
    pub(crate) tax_group_code: &'a str,
    #[serde(rename = "itemQuantity")]
    pub(crate) item_quantity: i64,
    #[serde(rename = "unitPriceAmount")]
    pub(crate) unit_price_amount: String,
    #[serde(rename = "grossPriceAmount")]
    pub(crate) gross_price_amount: String,
    #[serde(rename = "totalPriceAmount")]
    pub(crate) total_price_amount: String,
    #[serde(rename = "relatedSaleItemId", skip_serializing_if = "Option::is_none")]
    pub(crate) related_sale_item_id: Option<&'a str>,
    #[serde(rename = "returnAmount", skip_serializing_if = "Option::is_none")]
    pub(crate) return_amount: Option<String>,
}

/// The buyer, where the document has to name one.
#[derive(Debug, Serialize)]
pub(crate) struct GmuBuyer<'a> {
    #[serde(rename = "customerType")]
    pub(crate) customer_type: u8,
    #[serde(rename = "firstName", skip_serializing_if = "Option::is_none")]
    pub(crate) first_name: Option<&'a str>,
    #[serde(rename = "middleName", skip_serializing_if = "Option::is_none")]
    pub(crate) middle_name: Option<&'a str>,
    #[serde(rename = "familyName", skip_serializing_if = "Option::is_none")]
    pub(crate) family_name: Option<&'a str>,
    #[serde(rename = "companyName", skip_serializing_if = "Option::is_none")]
    pub(crate) company_name: Option<&'a str>,
    #[serde(rename = "taxOfficeCode", skip_serializing_if = "Option::is_none")]
    pub(crate) tax_office_code: Option<&'a str>,
    #[serde(rename = "taxNumber", skip_serializing_if = "Option::is_none")]
    pub(crate) tax_number: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) country: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) city: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) district: Option<&'a str>,
}

/// `POST /v2/terminal-host/gmu/payment/refund`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuRefundRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: &'a str,
    #[serde(rename = "saleAppName")]
    pub(crate) sale_app_name: &'a str,
    #[serde(rename = "saleAppVersion")]
    pub(crate) sale_app_version: &'a str,
    #[serde(rename = "notificationPhone", skip_serializing_if = "Option::is_none")]
    pub(crate) notification_phone: Option<&'a str>,
    #[serde(rename = "notificationEmail", skip_serializing_if = "Option::is_none")]
    pub(crate) notification_email: Option<&'a str>,
    #[serde(rename = "saleItems")]
    pub(crate) sale_items: Vec<GmuSaleItem<'a>>,
}

/// `POST /v2/terminal-host/gmu/payment/void`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuVoidRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// `POST /v2/terminal-host/gmu/payment/query-transaction-status`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuQueryRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "paymentId", skip_serializing_if = "Option::is_none")]
    pub(crate) payment_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId", skip_serializing_if = "Option::is_none")]
    pub(crate) device_unique_id: Option<&'a str>,
    #[serde(
        rename = "transactionReferenceId",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) transaction_reference_id: Option<&'a str>,
}

/// `POST /v2/terminal-host/gmu/payment/refundable-sale-info`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuRefundableSaleRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId", skip_serializing_if = "Option::is_none")]
    pub(crate) device_unique_id: Option<&'a str>,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
}

/// `POST /v2/terminal-host/gmu/partial-payment/add-payment`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuPartialAddRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId", skip_serializing_if = "Option::is_none")]
    pub(crate) device_unique_id: Option<&'a str>,
    #[serde(
        rename = "transactionReferenceId",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) transaction_reference_id: Option<&'a str>,
    #[serde(rename = "saleNumber")]
    pub(crate) sale_number: &'a str,
    pub(crate) price: String,
    pub(crate) installment: u8,
    pub(crate) currency: &'a str,
}

/// `POST /v2/terminal-host/gmu/partial-payment/complete`.
#[derive(Debug, Serialize)]
pub(crate) struct GmuPartialCompleteRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "deviceUniqueId")]
    pub(crate) device_unique_id: &'a str,
    #[serde(rename = "transactionReferenceId")]
    pub(crate) transaction_reference_id: &'a str,
    #[serde(rename = "saleNumber")]
    pub(crate) sale_number: &'a str,
}

/// What every VUK 507 payment operation answers.
#[derive(Debug, Deserialize)]
pub(crate) struct GmuPaymentResponse {
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    /// `YYYYMMDD` as an integer, which is how iyzico types it here.
    #[serde(rename = "paymentDate")]
    pub(crate) payment_date: Option<i64>,
    pub(crate) price: Option<String>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "saleNumber")]
    pub(crate) sale_number: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "batchNo")]
    pub(crate) batch_no: Option<String>,
    #[serde(rename = "lastFourDigits")]
    pub(crate) last_four_digits: Option<String>,
}

/// What the partial payment's start and add steps answer.
#[derive(Debug, Deserialize)]
pub(crate) struct GmuPartialPaymentResponse {
    #[serde(rename = "saleNumber")]
    pub(crate) sale_number: Option<String>,
    #[serde(rename = "remainingPaymentAmount")]
    pub(crate) remaining_payment_amount: Option<String>,
}

/// What `refundable-sale-info` answers.
#[derive(Debug, Deserialize)]
pub(crate) struct GmuRefundableSaleResponse {
    #[serde(rename = "saleNumber")]
    pub(crate) sale_number: Option<String>,
    #[serde(rename = "saleUid")]
    pub(crate) sale_uid: Option<String>,
    #[serde(rename = "totalReturnableAmount")]
    pub(crate) total_returnable_amount: Option<String>,
    pub(crate) currency: Option<String>,
}
