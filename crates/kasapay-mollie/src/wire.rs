//! Mollie's JSON, exactly as their OpenAPI document describes it.
//!
//! Every field is optional on the way in. Mollie's document marks several of
//! them required on a response, but a payments library that stops parsing
//! because one is missing loses the rest of the answer too; the fields this
//! crate cannot do without are checked where they are read, with a message
//! saying which one was absent.

use std::collections::BTreeMap;

/// `{"currency": "EUR", "value": "10.00"}` — a decimal string, never a number.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Amount {
    pub currency: Option<String>,
    pub value: Option<String>,
}

/// The same object on the way out, written from an integer count of minor units.
#[derive(Debug, serde::Serialize)]
pub(crate) struct AmountOut {
    pub currency: &'static str,
    pub value: String,
}

/// One of the `_links` entries: `{"href": …, "type": …}`, or `null`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct Link {
    pub href: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct Links {
    pub checkout: Option<Link>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Payment {
    pub id: Option<String>,
    pub amount: Option<Amount>,
    /// What is left to refund. Mollie sends it on a payment that has been
    /// paid, and it is what `Provider::refund` asks for when the caller names
    /// no amount.
    pub amount_remaining: Option<Amount>,
    pub status: Option<String>,
    pub metadata: Option<serde_json::Value>,
    #[serde(rename = "_links")]
    pub links: Option<Links>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Capture {
    pub id: Option<String>,
    pub payment_id: Option<String>,
    pub amount: Option<Amount>,
    pub status: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Refund {
    pub id: Option<String>,
    pub payment_id: Option<String>,
    pub amount: Option<Amount>,
    pub status: Option<String>,
}

/// Mollie's error object. No machine-readable code anywhere in it.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ErrorBody {
    pub detail: Option<String>,
    pub title: Option<String>,
    pub field: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatePayment<'a> {
    pub amount: AmountOut,
    pub description: &'a str,
    pub redirect_url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<&'a str>,
    /// `first` establishes a mandate, `recurring` charges one. `None` sends
    /// nothing, which is Mollie's own `oneoff`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_type: Option<&'static str>,
    /// Only meaningful with `sequence_type: recurring` — which of the
    /// customer's mandates to charge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mandate_id: Option<&'a str>,
    pub metadata: BTreeMap<&'a str, &'a str>,
}

/// `POST /v2/customers`'s body. Both fields are optional per Mollie's schema.
#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateCustomer<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<&'a str>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct Customer {
    pub id: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Mandate {
    pub id: Option<String>,
    pub status: Option<String>,
    pub method: Option<String>,
    pub customer_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateCapture {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<AmountOut>,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct CreateRefund<'a> {
    pub amount: AmountOut,
    /// Shown to the payer on their statement, so it carries the caller's own
    /// words and never one of the named reasons.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<&'a str, &'a str>,
}
