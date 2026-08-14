//! The request and response bodies of the subscription API, as iyzico documents them.

use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// `POST /v2/subscription/products` and `POST /v2/subscription/products/{ref}`.
///
/// One struct for both: iyzico documents the create and the update with the
/// same four fields, and the update is a replacement rather than a patch.
#[derive(Debug, Serialize)]
pub(crate) struct ProductRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    pub(crate) name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// `POST /v2/subscription/products/{productReferenceCode}/pricing-plans`.
#[derive(Debug, Serialize)]
pub(crate) struct CreatePlanRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    /// Also in the path. iyzico's documentation leaves it out of the body and
    /// their PHP SDK puts it in, so it goes in both places rather than one.
    #[serde(rename = "productReferenceCode")]
    pub(crate) product_reference_code: &'a str,
    pub(crate) name: &'a str,
    /// A decimal string. iyzico types this `decimal`; a float would round.
    pub(crate) price: String,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: &'a str,
    #[serde(rename = "paymentInterval")]
    pub(crate) payment_interval: &'a str,
    #[serde(
        rename = "paymentIntervalCount",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) payment_interval_count: Option<u32>,
    #[serde(rename = "planPaymentType")]
    pub(crate) plan_payment_type: &'a str,
    #[serde(rename = "recurrenceCount", skip_serializing_if = "Option::is_none")]
    pub(crate) recurrence_count: Option<u32>,
    #[serde(rename = "trialPeriodDays", skip_serializing_if = "Option::is_none")]
    pub(crate) trial_period_days: Option<u32>,
}

/// `POST /v2/subscription/pricing-plans/{pricingPlanReferenceCode}`.
///
/// Two fields and no more: iyzico documents this as updating the name and the
/// trial period, and nothing else about a plan.
#[derive(Debug, Serialize)]
pub(crate) struct UpdatePlanRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    /// Also in the path, for the same reason as the create's product code.
    #[serde(rename = "pricingPlanReferenceCode")]
    pub(crate) pricing_plan_reference_code: &'a str,
    pub(crate) name: &'a str,
    #[serde(rename = "trialPeriodDays", skip_serializing_if = "Option::is_none")]
    pub(crate) trial_period_days: Option<u32>,
}

/// The answer to anything that returns one product or one plan.
///
/// `data` is held as the bytes iyzico sent so that a product read on its own
/// carries the same untouched body as one read out of a listing.
#[derive(Debug, Deserialize)]
pub(crate) struct Envelope {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) data: Option<Box<RawValue>>,
}

/// The answer to a listing.
#[derive(Debug, Deserialize)]
pub(crate) struct ListEnvelope {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) data: Option<ListData>,
}

/// One page of products or of plans.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListData {
    /// Documented as an integer for a page of plans and as a string for a page
    /// of products. Kept as iyzico's own bytes so either reads.
    #[serde(rename = "totalCount")]
    pub(crate) total_count: Option<Box<RawValue>>,
    #[serde(rename = "currentPage")]
    pub(crate) current_page: Option<i64>,
    #[serde(rename = "pageCount")]
    pub(crate) page_count: Option<i64>,
    pub(crate) items: Option<Vec<Box<RawValue>>>,
}

/// The answer to a delete: the envelope and nothing else.
#[derive(Debug, Deserialize)]
pub(crate) struct Ack {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// One product, as a create, an update, a read and a listing all describe it.
#[derive(Debug, Deserialize)]
pub(crate) struct ProductItem {
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) status: Option<String>,
    /// Documented as `YYYY-MM-DD hh:mm:ss`, and kept as iyzico's own bytes
    /// because the same field on a plan is documented as epoch milliseconds.
    #[serde(rename = "createdDate")]
    pub(crate) created_date: Option<Box<RawValue>>,
    #[serde(rename = "pricingPlans")]
    pub(crate) pricing_plans: Option<Vec<Box<RawValue>>>,
}

/// One pricing plan, on its own or summarised inside a product.
#[derive(Debug, Deserialize)]
pub(crate) struct PlanItem {
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: Option<String>,
    pub(crate) name: Option<String>,
    #[serde(rename = "productReferenceCode")]
    pub(crate) product_reference_code: Option<String>,
    /// Documented as a decimal, and as a JSON number in one of the two
    /// languages. Kept as iyzico's own bytes so neither reading loses a digit.
    pub(crate) price: Option<Box<RawValue>>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    #[serde(rename = "paymentInterval")]
    pub(crate) payment_interval: Option<String>,
    #[serde(rename = "paymentIntervalCount")]
    pub(crate) payment_interval_count: Option<i64>,
    #[serde(rename = "planPaymentType")]
    pub(crate) plan_payment_type: Option<String>,
    #[serde(rename = "recurrenceCount")]
    pub(crate) recurrence_count: Option<i64>,
    #[serde(rename = "trialPeriodDays")]
    pub(crate) trial_period_days: Option<i64>,
    pub(crate) status: Option<String>,
    /// Epoch milliseconds on a plan of its own, `YYYY-MM-DD hh:mm:ss` on one
    /// summarised inside a product. See [`ProductItem::created_date`].
    #[serde(rename = "createdDate")]
    pub(crate) created_date: Option<Box<RawValue>>,
}

/// A value as iyzico wrote it, whether that was `10.5` or `"10.5"`.
///
/// The quotes are the only difference, and stripping them keeps every digit —
/// reading a JSON number into a float and printing it back does not.
pub(crate) fn text(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}

/// A count iyzico wrote as either a number or a string.
pub(crate) fn integer(value: &RawValue) -> Option<i64> {
    text(value).parse().ok()
}
