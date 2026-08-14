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
    pub(crate) payment_amount: Option<String>,
    pub(crate) currency: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) err_no: Option<String>,
}

/// The answer to `/odeme/iade`, and anything else with nothing to report.
#[derive(Debug, Deserialize)]
pub(crate) struct PlainResponse {
    pub(crate) status: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) err_no: Option<String>,
}
