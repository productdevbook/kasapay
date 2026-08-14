//! The bytes `softpos::Client` sends and reads.

use kasapay_core::{Error, ErrorKind, Money};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::softpos::PROVIDER;

#[derive(Serialize)]
pub(crate) struct InitSaleRequest<'a> {
    pub(crate) amount: Box<RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) add_commission: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) instalment: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) card_holder_phone: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) card_holder_mail: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reference_no: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) selected_agent_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) callback_url: Option<&'a str>,
}

#[derive(Serialize)]
pub(crate) struct InitReversalRequest<'a> {
    pub(crate) xact_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reference_no: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) callback_url: Option<&'a str>,
}

/// What both `init_sale_transaction` and `init_reversal_transaction` answer.
///
/// iyzico documents `InitSaleResponse` and `InitReversalResponse` as two
/// schemas, but every field, name and type is identical — both name a
/// `payment_session_id`, a `deeplink_url` and an `encryption_key`, and
/// nothing else. One wire type reads both.
#[derive(Deserialize, Default)]
pub(crate) struct FlowResponse {
    pub(crate) payment_session_id: Option<String>,
    pub(crate) deeplink_url: Option<String>,
    pub(crate) encryption_key: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CheckTransactionRequest<'a> {
    pub(crate) payment_session_id: &'a str,
}

#[derive(Deserialize, Default)]
pub(crate) struct CheckTransactionResponse {
    #[serde(rename = "Data")]
    pub(crate) data: Option<Vec<Box<RawValue>>>,
}

#[derive(Deserialize, Default)]
pub(crate) struct TransactionWire {
    pub(crate) xact_id: Option<String>,
    pub(crate) xact_date: Option<String>,
    pub(crate) transaction_type: Option<i32>,
    pub(crate) pos_type: Option<i32>,
    pub(crate) agent_id: Option<String>,
    pub(crate) is_tds: Option<bool>,
    pub(crate) bank_id: Option<String>,
    pub(crate) instalment: Option<i32>,
    pub(crate) card_no: Option<String>,
    pub(crate) card_holder: Option<String>,
    pub(crate) card_type: Option<String>,
    pub(crate) ratio: Option<Box<RawValue>>,
    pub(crate) amount: Option<Box<RawValue>>,
    #[serde(rename = "netAmount")]
    pub(crate) net_amount: Option<Box<RawValue>>,
    pub(crate) comission: Option<Box<RawValue>>,
    pub(crate) comission_tax: Option<Box<RawValue>>,
    pub(crate) currency: Option<String>,
    pub(crate) authorization_code: Option<String>,
    pub(crate) reference_code: Option<String>,
    pub(crate) order_id: Option<String>,
    pub(crate) is_succeed: Option<bool>,
    pub(crate) xact_transaction_id: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) phone: Option<String>,
    pub(crate) note: Option<String>,
    pub(crate) agent_reference: Option<String>,
}

/// Paynet's `{"object_name", "code", "message"}` shape, on a 400.
#[derive(Deserialize, Default)]
pub(crate) struct ErrorResponse {
    pub(crate) code: Option<i64>,
    pub(crate) message: Option<String>,
}

/// A value as `PayPOS` wrote it, whether that was `10.5` or `"10.5"`.
pub(crate) fn text(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}

/// A [`Money`] read off a `PayPOS` `number` field, in the currency the caller
/// already knows the transaction is in.
///
/// `None` for anything [`Money::parse`] will not take — `PayPOS` types these
/// fields `number` with no further shape documented, so a value this cannot
/// read stays in [`crate::softpos::Transaction::raw`] rather than being
/// guessed at.
pub(crate) fn money(value: Option<&RawValue>, currency: kasapay_core::Currency) -> Option<Money> {
    Money::parse(text(value?), currency).ok()
}

/// An amount on the way out, as the bare JSON number `PayPOS`'s schema types it.
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
