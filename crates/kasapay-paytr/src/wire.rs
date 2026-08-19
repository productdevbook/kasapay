//! PayTR's response bodies.
//!
//! The payment calls answer the same envelope: a `status`, and on a refusal an
//! `err_no` and a `reason`. The BIN service is the exception — it puts its
//! refusal in `err_msg` — and that is why it has its own type.

use serde::Deserialize;

/// The answer to `/odeme/api/get-token`.
#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) status: Option<String>,
    pub(crate) token: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) err_no: Option<String>,
}

/// The answer to `/odeme/durum-sorgu`.
#[derive(Debug, Deserialize)]
pub(crate) struct StatusResponse {
    pub(crate) status: Option<String>,
    /// The order's amount.
    pub(crate) payment_amount: Option<String>,
    /// What the payer was actually charged, which is larger under an
    /// instalment surcharge.
    pub(crate) payment_total: Option<String>,
    pub(crate) currency: Option<String>,
    /// Every refund taken off this payment so far.
    #[serde(default)]
    pub(crate) returns: Vec<ReturnItem>,
    pub(crate) reason: Option<String>,
    pub(crate) err_no: Option<String>,
}

/// One refund, as the status query reports it.
#[derive(Debug, Deserialize)]
pub(crate) struct ReturnItem {
    pub(crate) return_amount: Option<String>,
    pub(crate) return_date: Option<String>,
    pub(crate) date_completed: Option<String>,
    pub(crate) return_ref_num: Option<String>,
}

/// The answer to `/odeme/iade`, and anything else with nothing to report.
#[derive(Debug, Deserialize)]
pub(crate) struct PlainResponse {
    pub(crate) status: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) err_no: Option<String>,
}

/// The answer to `/odeme/api/bin-detail`.
///
/// This one names its refusal `err_msg` rather than `reason`, and documents no
/// `err_no` at all.
#[derive(Debug, Deserialize)]
pub(crate) struct BinResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "cardType")]
    pub(crate) card_type: Option<String>,
    #[serde(rename = "businessCard")]
    pub(crate) business_card: Option<String>,
    pub(crate) bank: Option<String>,
    pub(crate) brand: Option<String>,
    pub(crate) schema: Option<String>,
    #[serde(rename = "bankCode")]
    pub(crate) bank_code: Option<NumberOrText>,
    pub(crate) allow_non3d: Option<String>,
    pub(crate) err_msg: Option<String>,
}

/// The answer to `/odeme/taksit-oranlari`, as far as PayTR documents it.
///
/// `oranlar` is deliberately absent from this struct. PayTR describes it as
/// "the rates of the instalment counts defined for your store, by card type …
/// returned in array format" and never says what one entry holds — not in
/// either language's field table, not in the sample programs, not in their
/// Postman collection. A shape invented here would be a fixture standing in
/// for a body nobody has seen, so it is read off `Raw` instead. See #73.
#[derive(Debug, Deserialize)]
pub(crate) struct InstalmentRatesResponse {
    pub(crate) status: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) err_msg: Option<String>,
    /// The largest number of instalments the store is set up for.
    pub(crate) max_inst_non_bus: Option<NumberOrText>,
}

/// PayTR documents `bankCode` as an int and gives `0010` as the example, which
/// no int keeps. Both readings are accepted rather than one guessed at.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum NumberOrText {
    Text(String),
    Number(i64),
}

impl NumberOrText {
    pub(crate) fn into_string(self) -> String {
        match self {
            Self::Text(text) => text,
            Self::Number(number) => number.to_string(),
        }
    }
}
