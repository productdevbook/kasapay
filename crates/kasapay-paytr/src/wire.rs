//! PayTR's response bodies.
//!
//! All three answer the same envelope: a `status`, and on a refusal an
//! `err_no` and a `reason`.

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
