//! The request and response bodies of the classic API, as it sends them.

use serde::{Deserialize, Serialize};

/// `POST /payment/bin/check`.
#[derive(Debug, Serialize)]
pub(crate) struct BinCheckRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "binNumber")]
    pub(crate) bin_number: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

/// The answer to `POST /payment/bin/check`.
#[derive(Debug, Deserialize)]
pub(crate) struct BinCheckResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "binNumber")]
    pub(crate) bin_number: Option<String>,
    #[serde(rename = "cardType")]
    pub(crate) card_type: Option<String>,
    #[serde(rename = "cardAssociation")]
    pub(crate) card_association: Option<String>,
    #[serde(rename = "cardFamily")]
    pub(crate) card_family: Option<String>,
    #[serde(rename = "bankName")]
    pub(crate) bank_name: Option<String>,
    #[serde(rename = "bankCode")]
    pub(crate) bank_code: Option<i64>,
    /// 1 for a commercial card, 0 otherwise. Typed as a number, not a boolean.
    pub(crate) commercial: Option<i64>,
}
