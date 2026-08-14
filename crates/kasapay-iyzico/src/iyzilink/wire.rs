//! The request and response bodies of the iyzilink API, as iyzico documents them.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// `POST /v2/iyzilink/products`.
#[derive(Debug, Serialize)]
pub(crate) struct CreateRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    /// A decimal string. iyzico types these `BigDecimal`; a float would round.
    pub(crate) price: String,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: &'a str,
    #[serde(rename = "encodedImageFile")]
    pub(crate) encoded_image_file: &'a str,
    #[serde(rename = "addressIgnorable", skip_serializing_if = "Option::is_none")]
    pub(crate) address_ignorable: Option<bool>,
    #[serde(
        rename = "installmentRequested",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) installment_requested: Option<bool>,
    #[serde(rename = "stockEnabled", skip_serializing_if = "Option::is_none")]
    pub(crate) stock_enabled: Option<bool>,
    #[serde(rename = "stockCount", skip_serializing_if = "Option::is_none")]
    pub(crate) stock_count: Option<u32>,
    #[serde(rename = "flexibleLink", skip_serializing_if = "Option::is_none")]
    pub(crate) flexible_link: Option<bool>,
    #[serde(rename = "categoryType", skip_serializing_if = "Option::is_none")]
    pub(crate) category_type: Option<&'a str>,
}

/// `PUT /v2/iyzilink/products/{token}`.
///
/// The same fields as a create bar two: iyzico documents no `flexibleLink` and
/// no `presetPriceValues` here, so a link cannot stop being a flexible one
/// through this API.
#[derive(Debug, Serialize)]
pub(crate) struct UpdateRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    pub(crate) price: String,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: &'a str,
    #[serde(rename = "encodedImageFile", skip_serializing_if = "Option::is_none")]
    pub(crate) encoded_image_file: Option<&'a str>,
    #[serde(rename = "addressIgnorable", skip_serializing_if = "Option::is_none")]
    pub(crate) address_ignorable: Option<bool>,
    #[serde(
        rename = "installmentRequested",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) installment_requested: Option<bool>,
    #[serde(rename = "stockEnabled", skip_serializing_if = "Option::is_none")]
    pub(crate) stock_enabled: Option<bool>,
    #[serde(rename = "stockCount", skip_serializing_if = "Option::is_none")]
    pub(crate) stock_count: Option<u32>,
    #[serde(rename = "categoryType", skip_serializing_if = "Option::is_none")]
    pub(crate) category_type: Option<&'a str>,
}

/// `POST /v2/iyzilink/fast-link/products`.
#[derive(Debug, Serialize)]
pub(crate) struct FastLinkRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
    pub(crate) price: String,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: &'a str,
}

/// The answer to a create, a fast-link create or an update.
#[derive(Debug, Deserialize)]
pub(crate) struct SaveResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) data: Option<SavedLink>,
}

/// The `data` of a create or an update.
#[derive(Debug, Deserialize)]
pub(crate) struct SavedLink {
    pub(crate) token: Option<String>,
    pub(crate) url: Option<String>,
    #[serde(rename = "imageUrl")]
    pub(crate) image_url: Option<String>,
}

/// The answer to a delete and to a status change: the envelope and nothing else.
#[derive(Debug, Deserialize)]
pub(crate) struct Ack {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// The answer to `GET /v2/iyzilink/products/{token}`.
///
/// `data` is held as the bytes iyzico sent so that the link can carry its own
/// untouched body, exactly as one read out of a listing does.
#[derive(Debug, Deserialize)]
pub(crate) struct DetailResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) data: Option<Box<RawValue>>,
}

/// The answer to `GET /v2/iyzilink/products`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) data: Option<ListData>,
}

/// One page of links.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListData {
    #[serde(rename = "listingReviewed")]
    pub(crate) listing_reviewed: Option<bool>,
    #[serde(rename = "totalCount")]
    pub(crate) total_count: Option<i64>,
    #[serde(rename = "currentPage")]
    pub(crate) current_page: Option<i64>,
    #[serde(rename = "pageCount")]
    pub(crate) page_count: Option<i64>,
    pub(crate) items: Option<Vec<Box<RawValue>>>,
}

/// One link, as a detail query and a listing both describe it.
#[derive(Debug, Deserialize)]
pub(crate) struct Product {
    pub(crate) token: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    /// Documented as a decimal string, and sent as a JSON number in places.
    /// Kept as iyzico's own bytes so neither reading loses a digit.
    pub(crate) price: Option<Box<RawValue>>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    #[serde(rename = "productType")]
    pub(crate) product_type: Option<String>,
    #[serde(rename = "productStatus")]
    pub(crate) product_status: Option<String>,
    #[serde(rename = "merchantId")]
    pub(crate) merchant_id: Option<i64>,
    pub(crate) url: Option<String>,
    #[serde(rename = "imageUrl")]
    pub(crate) image_url: Option<String>,
    #[serde(rename = "addressIgnorable")]
    pub(crate) address_ignorable: Option<bool>,
    #[serde(rename = "soldCount")]
    pub(crate) sold_count: Option<i64>,
    #[serde(rename = "installmentRequested")]
    pub(crate) installment_requested: Option<bool>,
    #[serde(rename = "stockEnabled")]
    pub(crate) stock_enabled: Option<bool>,
    #[serde(rename = "stockCount")]
    pub(crate) stock_count: Option<i64>,
    #[serde(rename = "presetPriceValues")]
    pub(crate) preset_price_values: Option<Vec<Box<RawValue>>>,
    #[serde(rename = "flexibleLink")]
    pub(crate) flexible_link: Option<bool>,
    #[serde(rename = "categoryType")]
    pub(crate) category_type: Option<String>,
}

/// A decimal as iyzico wrote it, whether that was `10.5` or `"10.5"`.
///
/// The quotes are the only difference, and stripping them keeps every digit —
/// reading a JSON number into a float and printing it back does not.
pub(crate) fn decimal(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}
