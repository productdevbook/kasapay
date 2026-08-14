//! The request and response bodies of the mass payout API, as iyzico documents them.

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// `POST /v1/mass/payout/init`.
#[derive(Debug, Serialize)]
pub(crate) struct InitRequest<'a> {
    #[serde(rename = "externalId")]
    pub(crate) external_id: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) purpose: Option<&'a str>,
    pub(crate) items: Vec<Item<'a>>,
}

/// One line of a mass payout, on the way out.
#[derive(Debug, Serialize)]
pub(crate) struct Item<'a> {
    #[serde(rename = "itemExternalId")]
    pub(crate) external_id: &'a str,
    #[serde(rename = "recipientType")]
    pub(crate) recipient_type: &'a str,
    #[serde(rename = "recipientInfo")]
    pub(crate) recipient_info: &'a str,
    pub(crate) amount: Amount<'a>,
    pub(crate) description: &'a str,
    #[serde(rename = "recipientName", skip_serializing_if = "Option::is_none")]
    pub(crate) recipient_name: Option<&'a str>,
    #[serde(rename = "nonWithdrawable", skip_serializing_if = "Option::is_none")]
    pub(crate) non_withdrawable: Option<bool>,
}

/// What is being sent, and in what.
#[derive(Debug, Serialize)]
pub(crate) struct Amount<'a> {
    /// A bare JSON number, which is how iyzico's own example writes it.
    pub(crate) value: Box<RawValue>,
    pub(crate) currency: &'a str,
}

/// `POST /v1/mass/payout/auth`.
#[derive(Debug, Serialize)]
pub(crate) struct AuthRequest<'a> {
    #[serde(rename = "requestId")]
    pub(crate) request_id: &'a str,
}

/// `POST /v1/mass/payout/retrieve`.
#[derive(Debug, Serialize)]
pub(crate) struct RetrieveRequest<'a> {
    #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
    pub(crate) request_id: Option<&'a str>,
    #[serde(
        rename = "externalMassPayoutId",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) external_mass_payout_id: Option<&'a str>,
    /// Required in Turkish and absent from the English page entirely.
    #[serde(rename = "itemType")]
    pub(crate) item_type: &'a str,
    pub(crate) page: u32,
    pub(crate) size: u32,
}

/// The answer to `init`.
#[derive(Debug, Deserialize)]
pub(crate) struct InitResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "requestId")]
    pub(crate) request_id: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(rename = "invalidItems")]
    pub(crate) invalid_items: Option<Vec<InvalidItem>>,
}

/// One item iyzico would not take, reported alongside a `success`.
#[derive(Debug, Deserialize)]
pub(crate) struct InvalidItem {
    /// Named `externalId` here and `itemExternalId` on the way in.
    #[serde(rename = "externalId")]
    pub(crate) external_id: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// The answer to anything that reports only whether it worked.
#[derive(Debug, Deserialize)]
pub(crate) struct Ack {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// The answer to `balance`.
#[derive(Debug, Deserialize)]
pub(crate) struct BalanceResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    /// Kept as iyzico's own bytes: no currency comes with it, so there is
    /// nothing to read it into.
    pub(crate) balance: Option<Box<RawValue>>,
}

/// The answer to `retrieve`.
#[derive(Debug, Deserialize)]
pub(crate) struct RetrieveResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "massPayout")]
    pub(crate) mass_payout: Option<Box<RawValue>>,
    #[serde(rename = "massPayoutItems")]
    pub(crate) mass_payout_items: Option<ItemsPage>,
    pub(crate) page: Option<i64>,
    pub(crate) size: Option<i64>,
    pub(crate) total: Option<i64>,
}

/// The page of items, which carries the paging in iyzico's example and not in
/// their schema.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ItemsPage {
    pub(crate) items: Option<Vec<Box<RawValue>>>,
    pub(crate) page: Option<i64>,
    pub(crate) size: Option<i64>,
    pub(crate) total: Option<i64>,
}

/// The answer to `item`.
#[derive(Debug, Deserialize)]
pub(crate) struct ItemResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) item: Option<Box<RawValue>>,
}

/// The mass payout itself, summarised.
#[derive(Debug, Deserialize)]
pub(crate) struct SummaryItem {
    #[serde(rename = "externalId")]
    pub(crate) external_id: Option<String>,
    /// Kept as iyzico's own bytes: their example writes it `000000`.
    #[serde(rename = "merchantId")]
    pub(crate) merchant_id: Option<Box<RawValue>>,
    #[serde(rename = "totalAmount")]
    pub(crate) total_amount: Option<Box<RawValue>>,
    #[serde(rename = "totalSuccessfulAmount")]
    pub(crate) total_successful_amount: Option<Box<RawValue>>,
    #[serde(rename = "totalCommissionAmount")]
    pub(crate) total_commission_amount: Option<Box<RawValue>>,
    #[serde(rename = "massPayoutStatus")]
    pub(crate) mass_payout_status: Option<String>,
    pub(crate) currency: Option<String>,
}

/// One line of a mass payout, on the way back.
#[derive(Debug, Deserialize)]
pub(crate) struct ItemDetail {
    #[serde(rename = "itemExternalId")]
    pub(crate) item_external_id: Option<String>,
    #[serde(rename = "referenceCode")]
    pub(crate) reference_code: Option<String>,
    #[serde(rename = "recipientType")]
    pub(crate) recipient_type: Option<String>,
    #[serde(rename = "recipientInfo")]
    pub(crate) recipient_info: Option<String>,
    #[serde(rename = "recipientName")]
    pub(crate) recipient_name: Option<String>,
    pub(crate) description: Option<String>,
    #[serde(rename = "itemStatus")]
    pub(crate) item_status: Option<String>,
    #[serde(rename = "errorMessages")]
    pub(crate) error_messages: Option<Vec<String>>,
    #[serde(rename = "totalAmount")]
    pub(crate) total_amount: Option<Box<RawValue>>,
    #[serde(rename = "commissionAmount")]
    pub(crate) commission_amount: Option<Box<RawValue>>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
}

/// A value as iyzico wrote it, whether that was `10.5` or `"10.5"`.
pub(crate) fn text(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}

/// A count iyzico wrote as either a number or a string.
pub(crate) fn integer(value: &RawValue) -> Option<i64> {
    text(value).parse().ok()
}

/// An amount iyzico wrote with more decimal places than the currency has.
///
/// Their own example reports a commission as `20.00000000`, which is exactly
/// twenty lira written with eight decimal places rather than two. Those zeros
/// carry no value, so they are dropped and the rest goes through
/// [`Money::parse`] unchanged. A digit that is *not* a zero out there is a
/// different number from any amount in the currency, and answers `None` rather
/// than being rounded into one.
pub(crate) fn decimal(value: &RawValue, currency: Currency) -> Option<Money> {
    let written = text(value).trim();
    let exponent = usize::try_from(currency.exponent()).ok()?;
    let Some((whole, frac)) = written.split_once('.') else {
        return Money::parse(written, currency).ok();
    };
    if frac.len() <= exponent {
        return Money::parse(written, currency).ok();
    }
    let kept = frac.get(..exponent)?;
    let dropped = frac.get(exponent..)?;
    if !dropped.bytes().all(|byte| byte == b'0') {
        return None;
    }
    let trimmed = if kept.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{kept}")
    };
    Money::parse(&trimmed, currency).ok()
}

/// An amount on the way out, as the bare JSON number iyzico's example writes.
///
/// Never a float: the digits [`Money`] holds are written straight into the
/// body, so nothing is rounded on the way to the wire.
pub(crate) fn amount(money: Money) -> Result<Box<RawValue>, Error> {
    RawValue::from_string(money.to_decimal_string()).map_err(|e| {
        Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "an amount could not be written as JSON",
        )
        .with_source(e)
    })
}
