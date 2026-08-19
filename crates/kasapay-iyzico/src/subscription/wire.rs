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

/// The subscriber, as every subscription request wants them.
#[derive(Debug, Serialize)]
pub(crate) struct SubscriberBody<'a> {
    pub(crate) name: &'a str,
    pub(crate) surname: &'a str,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: &'a str,
    #[serde(rename = "billingAddress")]
    pub(crate) billing_address: AddressBody<'a>,
    #[serde(rename = "shippingAddress", skip_serializing_if = "Option::is_none")]
    pub(crate) shipping_address: Option<AddressBody<'a>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct AddressBody<'a> {
    #[serde(rename = "contactName")]
    pub(crate) contact_name: &'a str,
    pub(crate) address: &'a str,
    pub(crate) city: &'a str,
    pub(crate) country: &'a str,
    #[serde(rename = "zipCode", skip_serializing_if = "Option::is_none")]
    pub(crate) zip_code: Option<&'a str>,
}

/// `POST /v2/subscription/checkoutform/initialize` — the hosted way in.
#[derive(Debug, Serialize)]
pub(crate) struct SubscriptionFormRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "callbackUrl")]
    pub(crate) callback_url: &'a str,
    #[serde(rename = "pricingPlanReferenceCode")]
    pub(crate) pricing_plan_reference_code: &'a str,
    #[serde(rename = "subscriptionInitialStatus")]
    pub(crate) subscription_initial_status: &'a str,
    pub(crate) customer: SubscriberBody<'a>,
}

/// What that answers: a token and the form itself.
#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionFormResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) token: Option<String>,
    #[serde(rename = "checkoutFormContent")]
    pub(crate) checkout_form_content: Option<String>,
    #[serde(rename = "tokenExpireTime")]
    pub(crate) token_expire_time: Option<i64>,
}

/// `POST /v2/subscription/initialize/with-customer` — subscribing somebody
/// iyzico already holds a card for.
#[derive(Debug, Serialize)]
pub(crate) struct SubscribeExistingRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "customerReferenceCode")]
    pub(crate) customer_reference_code: &'a str,
    #[serde(rename = "pricingPlanReferenceCode")]
    pub(crate) pricing_plan_reference_code: &'a str,
    #[serde(rename = "subscriptionInitialStatus")]
    pub(crate) subscription_initial_status: &'a str,
}

/// `POST /v2/subscription/card-update/checkoutform/initialize`.
#[derive(Debug, Serialize)]
pub(crate) struct CardUpdateFormRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "callbackUrl")]
    pub(crate) callback_url: &'a str,
    #[serde(
        rename = "customerReferenceCode",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) customer_reference_code: Option<&'a str>,
    #[serde(
        rename = "subscriptionReferenceCode",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) subscription_reference_code: Option<&'a str>,
}

/// `POST /v2/subscription/subscriptions/{ref}/upgrade`.
#[derive(Debug, Serialize)]
pub(crate) struct UpgradeRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "newPricingPlanReferenceCode")]
    pub(crate) new_pricing_plan_reference_code: &'a str,
    #[serde(rename = "upgradePeriod")]
    pub(crate) upgrade_period: &'a str,
    #[serde(rename = "useTrial")]
    pub(crate) use_trial: bool,
    #[serde(rename = "resetRecurrenceCount")]
    pub(crate) reset_recurrence_count: bool,
}

/// `POST /v2/subscription/operation/retry`.
#[derive(Debug, Serialize)]
pub(crate) struct RetryRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: &'a str,
}

/// One subscription, out of a read or a listing.
#[derive(Debug, Deserialize)]
pub(crate) struct SubscriptionItem {
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: Option<String>,
    #[serde(rename = "subscriptionStatus")]
    pub(crate) subscription_status: Option<String>,
    #[serde(rename = "pricingPlanReferenceCode")]
    pub(crate) pricing_plan_reference_code: Option<String>,
    #[serde(rename = "pricingPlanName")]
    pub(crate) pricing_plan_name: Option<String>,
    #[serde(rename = "productReferenceCode")]
    pub(crate) product_reference_code: Option<String>,
    #[serde(rename = "productName")]
    pub(crate) product_name: Option<String>,
    #[serde(rename = "customerReferenceCode")]
    pub(crate) customer_reference_code: Option<String>,
    #[serde(rename = "customerEmail")]
    pub(crate) customer_email: Option<String>,
    #[serde(rename = "trialDays")]
    pub(crate) trial_days: Option<i64>,
    /// Epoch milliseconds in one place and `YYYY-MM-DD hh:mm:ss` in another,
    /// so kept as iyzico's own bytes — the same as a plan's `createdDate`.
    #[serde(rename = "startDate")]
    pub(crate) start_date: Option<Box<RawValue>>,
    #[serde(rename = "endDate")]
    pub(crate) end_date: Option<Box<RawValue>>,
}

/// One subscriber, out of a read or a listing.
#[derive(Debug, Deserialize)]
pub(crate) struct SubscriberItem {
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) surname: Option<String>,
    pub(crate) email: Option<String>,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: Option<String>,
}
