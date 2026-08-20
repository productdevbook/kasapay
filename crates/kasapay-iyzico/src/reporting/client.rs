//! The reporting client.

use std::fmt;

use kasapay_core::{Currency, Error, ErrorKind, Money, PaymentId, ProviderId, Raw, Status};
use reqwest::Method;
use serde_json::value::RawValue;

use crate::classic::{self, Association, CardType};
use crate::reporting::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where a payment's own status, fraud result and refunds are read back.
const PAYMENT_DETAILS: &str = "/v2/reporting/payment/details";
/// Where a day's payments, cancels and refunds are read back together.
const PAYMENT_TRANSACTIONS: &str = "/v2/reporting/payment/transactions";
/// What language iyzico answers in. Sent rather than left out — see
/// [`crate::iyzilink`], which does the same for the same reason.
const LOCALE: &str = "tr";

/// Talks to iyzico's reporting API.
///
/// Built over a [`classic::Client`], because that is what reporting is: the
/// same host, the same [`IYZWSv2`](crate::Credentials) signing, the same
/// `status: "failure"` envelope. Cloning shares the one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    classic: classic::Client,
}

impl Client {
    /// Speaks reporting over a classic client.
    #[must_use]
    pub const fn new(classic: classic::Client) -> Self {
        Self { classic }
    }

    /// The classic client underneath, for everything that is not a report.
    #[must_use]
    pub const fn classic(&self) -> &classic::Client {
        &self.classic
    }

    /// Reads back a payment's status, fraud result, cancels and refunds.
    ///
    /// iyzico documents `paymentId` and `paymentConversationId` as
    /// alternatives — *"at least one of these must be provided"* — rather
    /// than as two required fields, which is what a literal reading of their
    /// own OpenAPI fragment would say. [`PaymentQuery`] makes the caller name
    /// one.
    ///
    /// Answers every payment iyzico has stored under the id or conversation
    /// id given — ordinarily one, but iyzico's own field name is plural and
    /// does not document a limit, so this does not assume one either.
    pub async fn payment_details(&self, query: &PaymentQuery) -> Result<Vec<PaymentDetail>, Error> {
        let (param, value) = query.as_query_param()?;
        let query_string = format!("?locale={LOCALE}&{param}={value}");
        let (response, _) = self
            .classic
            .request::<(), wire::PaymentDetailsResponse>(
                Method::GET,
                PAYMENT_DETAILS,
                &query_string,
                None,
            )
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the payment detail report",
        ) {
            return Err(error);
        }
        let items = response.payments.unwrap_or_default();
        let mut payments = Vec::with_capacity(items.len());
        for item in &items {
            payments.push(PaymentDetail::read(item)?);
        }
        Ok(payments)
    }

    /// Reads back a day's worth of payments, cancels and refunds, a page at a
    /// time.
    ///
    /// `page` counts from one, which is what iyzico's own parameter
    /// description says (`minimum: 1`) and is checked here rather than sent,
    /// for the same reason [`crate::iyzilink::Client::list`] checks its own
    /// paging: iyzico answers a page out of range with an error code rather
    /// than an empty page.
    ///
    /// `date` is `transactionDate`, documented as `YYYY-MM-DD` — iyzico's own
    /// example is `2025-07-24` — and is sent exactly as given; nothing here
    /// parses or reformats it.
    pub async fn daily_transactions(
        &self,
        date: &str,
        page: u32,
    ) -> Result<DailyTransactions, Error> {
        if page < 1 {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "iyzico pages daily transactions from page 1",
            ));
        }
        let date = query_value("transactionDate", date)?;
        let query_string = format!("?locale={LOCALE}&page={page}&transactionDate={date}");
        let (response, raw) = self
            .classic
            .request::<(), wire::DailyTransactionsResponse>(
                Method::GET,
                PAYMENT_TRANSACTIONS,
                &query_string,
                None,
            )
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the daily transaction report",
        ) {
            return Err(error);
        }
        let items = response.transactions.unwrap_or_default();
        let mut transactions = Vec::with_capacity(items.len());
        for item in &items {
            transactions.push(DailyTransactionItem::read(item)?);
        }
        Ok(DailyTransactions {
            transactions,
            current_page: response.current_page,
            total_page_count: response.total_page_count,
            raw,
        })
    }
}

impl From<classic::Client> for Client {
    fn from(classic: classic::Client) -> Self {
        Self::new(classic)
    }
}

/// Names the payment `payment_details` reads back.
///
/// iyzico takes exactly one of `paymentId` or `paymentConversationId`; both
/// marked `required` on the same operation is the OpenAPI fragment's way of
/// saying "one of these", not "both of these" — the prose beside it says so
/// in as many words.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaymentQuery {
    /// iyzico's own `paymentId`.
    Id(Box<str>),
    /// The `conversationId` the payment was made with.
    Conversation(Box<str>),
}

impl PaymentQuery {
    fn as_query_param(&self) -> Result<(&'static str, &str), Error> {
        match self {
            Self::Id(id) => Ok(("paymentId", query_value("paymentId", id)?)),
            Self::Conversation(id) => Ok((
                "paymentConversationId",
                query_value("paymentConversationId", id)?,
            )),
        }
    }
}

/// Refuses a query value that would change the query string rather than sit
/// in it.
///
/// Nothing about the query string is signed — the signature covers the path
/// alone — but an unescaped `&`, `#` or space in a caller-chosen id would
/// still either break the request or silently ask
/// iyzico something other than what was meant. Only the unreserved characters
/// of RFC 3986 are let through, same as the tokens [`crate::iyzilink`] and
/// [`crate::mass`] put in a signed path.
fn query_value<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    let unreserved = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    if value.is_empty() || !value.chars().all(unreserved) {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            format!(
                "reporting's `{field}` goes into an unsigned query string, so it may \
                 hold only letters, digits and -._~"
            ),
        ));
    }
    Ok(value)
}

/// One payment, as `payment/details` answers it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentDetail {
    /// iyzico's own id for the payment.
    pub payment_id: Option<PaymentId>,
    /// Where the payment stands.
    ///
    /// **Not [`kasapay_core::Status`]** — see [`PaymentStatus`] for why the
    /// two cannot be the same field.
    pub payment_status: Option<PaymentStatus>,
    /// How much of it has been refunded.
    pub refund_status: Option<PaymentRefundStatus>,
    /// The basket total.
    pub price: Option<Money>,
    /// What was actually collected.
    pub paid_price: Option<Money>,
    /// How many instalments it was split over.
    pub installment: Option<i64>,
    /// The merchant's own surcharge rate, informational only.
    ///
    /// A rate rather than an amount — `"0.05"` for five per cent — so it is
    /// kept as iyzico wrote it rather than read as [`Money`].
    pub merchant_commission_rate: Option<Box<str>>,
    /// What that rate came to.
    pub merchant_commission_rate_amount: Option<Money>,
    /// iyzico's own commission on the payment.
    pub iyzi_commission_rate_amount: Option<Money>,
    /// iyzico's own transaction fee on the payment.
    pub iyzi_commission_fee: Option<Money>,
    /// The `conversationId` the payment was made with.
    pub payment_conversation_id: Option<Box<str>>,
    /// What fraud review says about the payment, read the way
    /// [`crate::classic`] reads this field — the same `fraudStatus`
    /// mapping its stored-card charge uses — applied only when iyzico sent a
    /// code at all. Unlike a stored-card charge, which never reaches that
    /// mapping without one, a payment read back here may carry no fraud
    /// result at all, and `None` says so rather than guessing
    /// [`Status::Captured`].
    pub fraud_status: Option<Status>,
    /// What kind of card was used.
    pub card_type: Option<CardType>,
    /// The scheme the card runs on.
    pub card_association: Option<Association>,
    /// The issuer's own name for the product.
    pub card_family: Option<Box<str>>,
    /// The first eight digits of the card.
    pub bin_number: Option<Box<str>>,
    /// The last four digits of the card.
    pub last_four_digits: Option<Box<str>>,
    /// The basket id the payment was made with.
    pub basket_id: Option<Box<str>>,
    /// What the amounts on this payment are counted in.
    pub currency: Option<Currency>,
    /// The connector or POS provider that took the payment.
    pub connector_name: Option<Box<str>>,
    /// The bank's own auth code.
    pub auth_code: Option<Box<str>>,
    /// Whether 3-D Secure was used.
    pub three_ds: Option<bool>,
    /// The payment phase.
    pub phase: Option<Box<str>>,
    /// The acquiring bank or provider's name.
    pub acquirer_bank_name: Option<Box<str>>,
    /// The provider's own `hostReference` for the payment.
    pub host_reference: Option<Box<str>>,
    /// When the payment was created, as iyzico wrote it.
    pub created_date: Option<Box<str>>,
    /// The cancels on this payment.
    pub cancels: Vec<Cancel>,
    /// The line-item transactions on this payment.
    pub item_transactions: Vec<ItemTransaction>,
    /// iyzico's own answer for this payment, untouched.
    pub raw: Raw,
}

impl PaymentDetail {
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::PaymentDetailItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a payment detail was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        let currency = item.currency.as_deref().and_then(|c| c.parse().ok());
        let cancels = item
            .cancels
            .unwrap_or_default()
            .into_iter()
            .map(Cancel::from)
            .collect();
        let item_transactions = item
            .item_transactions
            .unwrap_or_default()
            .into_iter()
            .map(ItemTransaction::from)
            .collect();
        Ok(Self {
            payment_id: item.payment_id.map(PaymentId::issued),
            payment_status: item.payment_status.map(PaymentStatus::from),
            refund_status: item
                .payment_refund_status
                .as_deref()
                .map(PaymentRefundStatus::from),
            price: money(item.price.as_deref(), currency),
            paid_price: money(item.paid_price.as_deref(), currency),
            installment: item.installment,
            merchant_commission_rate: item.merchant_commission_rate.map(String::into_boxed_str),
            merchant_commission_rate_amount: money(
                item.merchant_commission_rate_amount.as_deref(),
                currency,
            ),
            iyzi_commission_rate_amount: money(
                item.iyzi_commission_rate_amount.as_deref(),
                currency,
            ),
            iyzi_commission_fee: money(item.iyzi_commission_fee.as_deref(), currency),
            payment_conversation_id: item.payment_conversation_id.map(String::into_boxed_str),
            fraud_status: item
                .fraud_status
                .map(|code| classic::fraud_status(Some(code))),
            card_type: item.card_type.as_deref().map(CardType::from),
            card_association: item.card_association.as_deref().map(Association::from),
            card_family: item.card_family.map(String::into_boxed_str),
            bin_number: item.bin_number.map(String::into_boxed_str),
            last_four_digits: item.last_four_digits.map(String::into_boxed_str),
            basket_id: item.basket_id.map(String::into_boxed_str),
            currency,
            connector_name: item.connector_name.map(String::into_boxed_str),
            auth_code: item.auth_code.map(String::into_boxed_str),
            three_ds: item.three_ds,
            phase: item.phase.map(String::into_boxed_str),
            acquirer_bank_name: item.acquirer_bank_name.map(String::into_boxed_str),
            host_reference: item.host_reference.map(String::into_boxed_str),
            created_date: item.created_date.map(String::into_boxed_str),
            cancels,
            item_transactions,
            raw: Raw::from_text(value.get()),
        })
    }
}

/// One cancel against a payment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Cancel {
    /// iyzico's own id for the cancel. `refundId` on the wire, and documented
    /// `oneOf` a string or an integer — kept as iyzico wrote it either way.
    pub refund_id: Option<Box<str>>,
    /// The `conversationId` the cancel was made with.
    pub cancel_conversation_id: Option<Box<str>>,
    /// How much was cancelled.
    pub refund_price: Option<Money>,
    /// iyzico's own status code for the cancel.
    ///
    /// A raw code rather than an enum: iyzico documents this field as
    /// `integer` and names no values for it, unlike
    /// [`ItemTransaction::status`], which does.
    pub refund_status: Option<i64>,
    /// When the cancel was made, as iyzico wrote it.
    pub created_date: Option<Box<str>>,
    /// What the cancel is counted in.
    pub currency: Option<Currency>,
    /// The bank's own auth code for the cancel.
    pub auth_code: Option<Box<str>>,
    /// The provider's own `hostReference` for the cancel.
    pub host_reference: Option<Box<str>>,
}

impl From<wire::CancelItem> for Cancel {
    fn from(item: wire::CancelItem) -> Self {
        let currency = item.currency_code.as_deref().and_then(|c| c.parse().ok());
        Self {
            refund_id: item.refund_id.as_deref().map(wire::text).map(Into::into),
            cancel_conversation_id: item.cancel_conversation_id.map(String::into_boxed_str),
            refund_price: money(item.refund_price.as_deref(), currency),
            refund_status: item.refund_status,
            created_date: item.created_date.map(String::into_boxed_str),
            currency,
            auth_code: item.auth_code.map(String::into_boxed_str),
            host_reference: item.host_reference.map(String::into_boxed_str),
        }
    }
}

/// One line-item transaction on a payment — the marketplace/sub-merchant
/// breakdown, when there is one.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ItemTransaction {
    /// iyzico's own id for the line-item transaction.
    pub payment_transaction_id: Option<Box<str>>,
    /// Where the line stands with fraud review.
    pub status: Option<TransactionApprovalStatus>,
    /// The item amount.
    pub price: Option<Money>,
    /// What was collected for this line.
    pub paid_price: Option<Money>,
    /// The merchant's own surcharge rate for this line, kept as text — see
    /// [`PaymentDetail::merchant_commission_rate`].
    pub merchant_commission_rate: Option<Box<str>>,
    /// What that rate came to for this line.
    pub merchant_commission_rate_amount: Option<Money>,
    /// iyzico's own commission on this line.
    pub iyzi_commission_rate_amount: Option<Money>,
    /// iyzico's own transaction fee on this line.
    pub iyzi_commission_fee: Option<Money>,
    /// The merchant blockage rate for this line, kept as text.
    pub blockage_rate: Option<Box<str>>,
    /// The blockage amount reflected to the merchant.
    pub blockage_rate_amount_merchant: Option<Money>,
    /// The blockage amount reflected to the sub-merchant.
    pub blockage_rate_amount_sub_merchant: Option<Money>,
    /// When the blockage resolved, as iyzico wrote it.
    pub blockage_resolved_date: Option<Box<str>>,
    /// The sub-merchant's own item amount.
    pub sub_merchant_price: Option<Money>,
    /// The sub-merchant's payout rate for this line, kept as text.
    pub sub_merchant_payout_rate: Option<Box<str>>,
    /// What is paid out to the sub-merchant for this line.
    pub sub_merchant_payout_amount: Option<Money>,
    /// What is paid out to the merchant for this line.
    pub merchant_payout_amount: Option<Money>,
    /// The same figures after FX conversion, when iyzico converted them.
    pub converted_payout: Option<ConvertedPayout>,
    /// The refunds made against this line.
    pub refunds: Vec<Refund>,
}

impl From<wire::ItemTransactionItem> for ItemTransaction {
    fn from(item: wire::ItemTransactionItem) -> Self {
        // iyzico's own schema names no currency for a line-item transaction;
        // its amounts are read as the payment's own, the same inference
        // `mass::client::Summary` makes for a mass payout's totals.
        let currency = None;
        Self {
            payment_transaction_id: item.payment_transaction_id.map(String::into_boxed_str),
            status: item.transaction_status.map(TransactionApprovalStatus::from),
            price: money(item.price.as_deref(), currency),
            paid_price: money(item.paid_price.as_deref(), currency),
            merchant_commission_rate: item.merchant_commission_rate.map(String::into_boxed_str),
            merchant_commission_rate_amount: money(
                item.merchant_commission_rate_amount.as_deref(),
                currency,
            ),
            iyzi_commission_rate_amount: money(
                item.iyzi_commission_rate_amount.as_deref(),
                currency,
            ),
            iyzi_commission_fee: money(item.iyzi_commission_fee.as_deref(), currency),
            blockage_rate: item.blockage_rate.map(String::into_boxed_str),
            blockage_rate_amount_merchant: money(
                item.blockage_rate_amount_merchant.as_deref(),
                currency,
            ),
            blockage_rate_amount_sub_merchant: money(
                item.blockage_rate_amount_sub_merchant.as_deref(),
                currency,
            ),
            blockage_resolved_date: item.blockage_resolved_date.map(String::into_boxed_str),
            sub_merchant_price: money(item.sub_merchant_price.as_deref(), currency),
            sub_merchant_payout_rate: item.sub_merchant_payout_rate.map(String::into_boxed_str),
            sub_merchant_payout_amount: money(item.sub_merchant_payout_amount.as_deref(), currency),
            merchant_payout_amount: money(item.merchant_payout_amount.as_deref(), currency),
            converted_payout: item.converted_payout.map(ConvertedPayout::from),
            refunds: item
                .refunds
                .unwrap_or_default()
                .into_iter()
                .map(Refund::from)
                .collect(),
        }
    }
}

/// A line-item transaction's amounts, after iyzico converted them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConvertedPayout {
    /// What was collected, after conversion.
    pub paid_price: Option<Money>,
    /// iyzico's own commission, after conversion.
    pub iyzi_commission_rate_amount: Option<Money>,
    /// iyzico's own transaction fee, after conversion.
    pub iyzi_commission_fee: Option<Money>,
    /// The blockage amount reflected to the merchant, after conversion.
    pub blockage_rate_amount_merchant: Option<Money>,
    /// The blockage amount reflected to the sub-merchant, after conversion.
    pub blockage_rate_amount_sub_merchant: Option<Money>,
    /// What is paid out to the sub-merchant, after conversion.
    pub sub_merchant_payout_amount: Option<Money>,
    /// What is paid out to the merchant, after conversion.
    pub merchant_payout_amount: Option<Money>,
    /// The FX rate applied, kept as text — a rate, not an amount.
    pub conversion_rate: Option<Box<str>>,
    /// What the FX rate came to.
    pub conversion_rate_amount: Option<Money>,
    /// What the converted figures are counted in.
    pub currency: Option<Currency>,
}

impl From<wire::ConvertedPayoutItem> for ConvertedPayout {
    fn from(item: wire::ConvertedPayoutItem) -> Self {
        let currency = item.currency.as_deref().and_then(|c| c.parse().ok());
        Self {
            paid_price: money(item.paid_price.as_deref(), currency),
            iyzi_commission_rate_amount: money(
                item.iyzi_commission_rate_amount.as_deref(),
                currency,
            ),
            iyzi_commission_fee: money(item.iyzi_commission_fee.as_deref(), currency),
            blockage_rate_amount_merchant: money(
                item.blockage_rate_amount_merchant.as_deref(),
                currency,
            ),
            blockage_rate_amount_sub_merchant: money(
                item.blockage_rate_amount_sub_merchant.as_deref(),
                currency,
            ),
            sub_merchant_payout_amount: money(item.sub_merchant_payout_amount.as_deref(), currency),
            merchant_payout_amount: money(item.merchant_payout_amount.as_deref(), currency),
            conversion_rate: item.iyzi_conversion_rate.map(String::into_boxed_str),
            conversion_rate_amount: money(item.iyzi_conversion_rate_amount.as_deref(), currency),
            currency,
        }
    }
}

/// One refund against a line-item transaction.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Refund {
    /// iyzico's own id for the refund. `refundTxId` on the wire, documented
    /// `oneOf` a string or an integer.
    pub refund_tx_id: Option<Box<str>>,
    /// The `conversationId` the refund was made with.
    pub refund_conversation_id: Option<Box<str>>,
    /// How much was refunded.
    pub refund_price: Option<Money>,
    /// iyzico's own status code for the refund.
    ///
    /// Raw, for the same reason as [`Cancel::refund_status`]: iyzico names no
    /// values for it here.
    pub refund_status: Option<i64>,
    /// Whether the refund happened after the payment was settled.
    pub is_after_settlement: Option<bool>,
    /// When the refund was made, as iyzico wrote it.
    pub created_date: Option<Box<str>>,
    /// What the refund is counted in.
    pub currency: Option<Currency>,
    /// The bank's own auth code for the refund.
    pub auth_code: Option<Box<str>>,
    /// The provider's own `hostReference` for the refund.
    pub host_reference: Option<Box<str>>,
    /// iyzico's own commission on the refund, when there was one.
    pub iyzi_commission_rate_amount: Option<Money>,
}

impl From<wire::RefundItem> for Refund {
    fn from(item: wire::RefundItem) -> Self {
        let currency = item.currency_code.as_deref().and_then(|c| c.parse().ok());
        Self {
            refund_tx_id: item.refund_tx_id.as_deref().map(wire::text).map(Into::into),
            refund_conversation_id: item.refund_conversation_id.map(String::into_boxed_str),
            refund_price: money(item.refund_price.as_deref(), currency),
            refund_status: item.refund_status,
            is_after_settlement: item.is_after_settlement,
            created_date: item.created_date.map(String::into_boxed_str),
            currency,
            auth_code: item.auth_code.map(String::into_boxed_str),
            host_reference: item.host_reference.map(String::into_boxed_str),
            iyzi_commission_rate_amount: money(
                item.iyzi_commission_rate_amount.as_deref(),
                currency,
            ),
        }
    }
}

/// A page of a day's payments, cancels and refunds.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DailyTransactions {
    /// The transactions on this page.
    pub transactions: Vec<DailyTransactionItem>,
    /// Which page this is.
    pub current_page: Option<i64>,
    /// How many pages there are in total.
    pub total_page_count: Option<i64>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// One transaction, out of a day's report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DailyTransactionItem {
    /// Whether this line is a payment, a cancel or a refund.
    pub transaction_type: Option<TransactionType>,
    /// When it happened, as iyzico wrote it.
    pub transaction_date: Option<Box<str>>,
    /// iyzico's own id for the transaction.
    pub transaction_id: Option<Box<str>>,
    /// Where it stands with fraud review — the same four codes as
    /// [`ItemTransaction::status`].
    pub transaction_status: Option<TransactionApprovalStatus>,
    /// Whether it was processed after payout, for a refund.
    pub after_settlement: Option<bool>,
    /// The line-item payment transaction id it belongs to.
    pub payment_tx_id: Option<Box<str>>,
    /// The payment it belongs to.
    pub payment_id: Option<Box<str>>,
    /// The `conversationId` it was made with.
    pub conversation_id: Option<Box<str>>,
    /// The payment phase.
    pub payment_phase: Option<Box<str>>,
    /// The item or transaction amount.
    pub price: Option<Money>,
    /// What was collected.
    pub paid_price: Option<Money>,
    /// What [`DailyTransactionItem::price`] and
    /// [`DailyTransactionItem::paid_price`] are counted in.
    pub transaction_currency: Option<Currency>,
    /// How many instalments it was split over.
    pub installment: Option<i64>,
    /// Whether 3-D Secure was used.
    pub three_ds: Option<bool>,
    /// What it settles in, when iyzico reports one.
    pub settlement_currency: Option<Currency>,
    /// The connector or POS provider that took it.
    pub connector_type: Option<Box<str>>,
    /// The POS order number.
    pub pos_order_id: Option<Box<str>>,
    /// The bank's own auth code.
    pub auth_code: Option<Box<str>>,
    /// The provider's own `hostReference`.
    pub host_reference: Option<Box<str>>,
    /// The basket id.
    pub basket_id: Option<Box<str>>,
    /// iyzico's own commission, for a payment line.
    pub iyzico_commission: Option<Money>,
    /// iyzico's own transaction fee, for a payment line.
    pub iyzico_fee: Option<Money>,
    /// The FX parity applied, kept as text — a rate, not an amount.
    pub parity: Option<Box<str>>,
    /// The FX conversion amount, for a payment line.
    pub iyzico_conversion_amount: Option<Money>,
    /// What is paid out to the merchant, for a payment line.
    pub merchant_payout_amount: Option<Money>,
    /// What is paid out to the sub-merchant, for a payment line.
    pub sub_merchant_payout_amount: Option<Money>,
}

impl DailyTransactionItem {
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::DailyTransactionItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a daily transaction was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        let currency = item
            .transaction_currency
            .as_deref()
            .and_then(|c| c.parse().ok());
        let settlement_currency = item
            .settlement_currency
            .as_deref()
            .and_then(|c| c.parse().ok());
        Ok(Self {
            transaction_type: item.transaction_type.as_deref().map(TransactionType::from),
            transaction_date: item.transaction_date.map(String::into_boxed_str),
            transaction_id: item.transaction_id.map(String::into_boxed_str),
            transaction_status: item.transaction_status.map(TransactionApprovalStatus::from),
            after_settlement: item.after_settlement.map(|flag| flag != 0),
            payment_tx_id: item.payment_tx_id.map(String::into_boxed_str),
            payment_id: item.payment_id.map(String::into_boxed_str),
            conversation_id: item.conversation_id.map(String::into_boxed_str),
            payment_phase: item.payment_phase.map(String::into_boxed_str),
            price: money(item.price.as_deref(), currency),
            paid_price: money(item.paid_price.as_deref(), currency),
            transaction_currency: currency,
            installment: item.installment,
            three_ds: item.three_ds,
            settlement_currency,
            connector_type: item.connector_type.map(String::into_boxed_str),
            pos_order_id: item.pos_order_id.map(String::into_boxed_str),
            auth_code: item.auth_code.map(String::into_boxed_str),
            host_reference: item.host_reference.map(String::into_boxed_str),
            basket_id: item.basket_id.map(String::into_boxed_str),
            iyzico_commission: money(item.iyzico_commission.as_deref(), currency),
            iyzico_fee: money(item.iyzico_fee.as_deref(), currency),
            parity: item.parity.map(String::into_boxed_str),
            iyzico_conversion_amount: money(item.iyzico_conversion_amount.as_deref(), currency),
            merchant_payout_amount: money(item.merchant_payout_amount.as_deref(), currency),
            sub_merchant_payout_amount: money(item.sub_merchant_payout_amount.as_deref(), currency),
        })
    }
}

/// iyzico's `paymentStatus`, on `payment/details`.
///
/// **Not [`Status`].** iyzico's own three codes fold two different outcomes
/// into one: `2` is documented as `"Failure / INIT_THREEDS"` — a refused
/// payment and one still waiting on the payer to finish 3-D Secure — where
/// [`crate::classic`]'s own mapping of a checkout form's `paymentStatus`
/// sends `FAILURE` to [`Status::Failed`] and `INIT_THREEDS` to
/// [`Status::RequiresAction`] as two different answers. Reporting's own
/// documentation does not say which of the two a `2` was, and forcing one
/// here would be a guess dressed as a mapping — the whole thing reusing
/// `classic`'s `fraudStatus` mapping for [`PaymentDetail::fraud_status`] is
/// built to avoid doing a second time. So this stays iyzico's own three codes, plain,
/// and a caller who needs [`Status`] reads [`PaymentDetail::raw`] for
/// whatever else the payment carries towards deciding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaymentStatus {
    /// `1`.
    Success,
    /// `2` — a refused payment **or** one still mid-3-D-Secure. iyzico's own
    /// documentation does not distinguish them.
    FailureOrInitThreeDs,
    /// `3` — waiting on the 3-D Secure callback.
    CallbackThreeDs,
    /// A code iyzico has started sending since this was written.
    Other(i64),
}

impl PaymentStatus {
    /// The code iyzico sent, however it was read.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::Success => 1,
            Self::FailureOrInitThreeDs => 2,
            Self::CallbackThreeDs => 3,
            Self::Other(code) => code,
        }
    }
}

impl From<i64> for PaymentStatus {
    fn from(value: i64) -> Self {
        match value {
            1 => Self::Success,
            2 => Self::FailureOrInitThreeDs,
            3 => Self::CallbackThreeDs,
            other => Self::Other(other),
        }
    }
}

/// iyzico's `paymentRefundStatus`, on `payment/details`.
///
/// Not folded into [`Status`] either, and for the reason [`Status`]'s own
/// documentation gives: no provider's status names a refund, because a
/// variant only reporting could ever produce would be a branch that never
/// runs anywhere else. This is that fact, kept as its own field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaymentRefundStatus {
    /// Nothing has been refunded.
    NotRefunded,
    /// Part of the payment has been refunded.
    PartiallyRefunded,
    /// The whole payment has been refunded.
    TotallyRefunded,
    /// A word iyzico has started sending since this was written.
    Other(Box<str>),
}

impl PaymentRefundStatus {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::NotRefunded => "NOT_REFUNDED",
            Self::PartiallyRefunded => "PARTIALLY_REFUNDED",
            Self::TotallyRefunded => "TOTALLY_REFUNDED",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for PaymentRefundStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PaymentRefundStatus {
    fn from(value: &str) -> Self {
        match value {
            "NOT_REFUNDED" => Self::NotRefunded,
            "PARTIALLY_REFUNDED" => Self::PartiallyRefunded,
            "TOTALLY_REFUNDED" => Self::TotallyRefunded,
            other => Self::Other(other.into()),
        }
    }
}

/// Where one line stands with fraud review.
///
/// iyzico's four codes, documented word for word the same on
/// [`ItemTransaction::status`] and [`DailyTransactionItem::transaction_status`]
/// — one description, reused by both rather than typed out twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionApprovalStatus {
    /// `0` — held for fraud review.
    InFraudReview,
    /// `-1` — rejected after review.
    RejectedAfterReview,
    /// `1` — approved; in a marketplace this means waiting for the
    /// sub-merchant's own approval.
    Approved,
    /// `2` — approved, and in a marketplace the sub-merchant has approved it
    /// too.
    MarketplaceApprovalGranted,
    /// A code iyzico has started sending since this was written.
    Other(i64),
}

impl TransactionApprovalStatus {
    /// The code iyzico sent, however it was read.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::InFraudReview => 0,
            Self::RejectedAfterReview => -1,
            Self::Approved => 1,
            Self::MarketplaceApprovalGranted => 2,
            Self::Other(code) => code,
        }
    }
}

impl From<i64> for TransactionApprovalStatus {
    fn from(value: i64) -> Self {
        match value {
            0 => Self::InFraudReview,
            -1 => Self::RejectedAfterReview,
            1 => Self::Approved,
            2 => Self::MarketplaceApprovalGranted,
            other => Self::Other(other),
        }
    }
}

/// Whether a line, on `payment/transactions`, is a payment, a cancel or a
/// refund.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransactionType {
    /// A cancel.
    Cancel,
    /// A payment.
    Payment,
    /// A refund.
    Refund,
    /// A word iyzico has started sending since this was written.
    Other(Box<str>),
}

impl TransactionType {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Cancel => "CANCEL",
            Self::Payment => "PAYMENT",
            Self::Refund => "REFUND",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for TransactionType {
    fn from(value: &str) -> Self {
        match value {
            "CANCEL" => Self::Cancel,
            "PAYMENT" => Self::Payment,
            "REFUND" => Self::Refund,
            other => Self::Other(other.into()),
        }
    }
}

/// Reads one of iyzico's amounts, when there is a currency to read it in.
///
/// `None` for an amount with no currency beside it, a currency
/// [`Currency`] has no name for, or digits that are not an amount in it. The
/// bytes stay in whatever carried them — see each type's own `raw`.
fn money(value: Option<&str>, currency: Option<Currency>) -> Option<Money> {
    Money::parse(value?, currency?).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        PaymentQuery, PaymentRefundStatus, PaymentStatus, TransactionApprovalStatus,
        TransactionType, money, query_value,
    };
    use kasapay_core::{Currency, Money};

    #[test]
    fn a_query_value_that_would_change_the_query_string_is_refused() {
        assert!(query_value("paymentId", "123456789").is_ok());
        assert!(query_value("paymentId", "").is_err());
        for hostile in ["a&b=c", "a b", "a#b", "a?b", "a=b"] {
            assert!(query_value("paymentId", hostile).is_err(), "{hostile}");
        }
    }

    #[test]
    fn payment_query_names_the_field_iyzico_expects() {
        let by_id = PaymentQuery::Id("123".into());
        assert_eq!(by_id.as_query_param().unwrap(), ("paymentId", "123"));
        let by_conversation = PaymentQuery::Conversation("conv-1".into());
        assert_eq!(
            by_conversation.as_query_param().unwrap(),
            ("paymentConversationId", "conv-1")
        );
    }

    #[test]
    fn the_documented_payment_status_codes_round_trip_and_the_rest_are_kept() {
        assert_eq!(PaymentStatus::from(1), PaymentStatus::Success);
        assert_eq!(PaymentStatus::from(2), PaymentStatus::FailureOrInitThreeDs);
        assert_eq!(PaymentStatus::from(3), PaymentStatus::CallbackThreeDs);
        assert_eq!(PaymentStatus::from(9), PaymentStatus::Other(9));
        for code in [1, 2, 3, 9] {
            assert_eq!(PaymentStatus::from(code).code(), code);
        }
    }

    #[test]
    fn the_documented_transaction_approval_codes_round_trip() {
        assert_eq!(
            TransactionApprovalStatus::from(0),
            TransactionApprovalStatus::InFraudReview
        );
        assert_eq!(
            TransactionApprovalStatus::from(-1),
            TransactionApprovalStatus::RejectedAfterReview
        );
        assert_eq!(
            TransactionApprovalStatus::from(1),
            TransactionApprovalStatus::Approved
        );
        assert_eq!(
            TransactionApprovalStatus::from(2),
            TransactionApprovalStatus::MarketplaceApprovalGranted
        );
        assert_eq!(
            TransactionApprovalStatus::from(7),
            TransactionApprovalStatus::Other(7)
        );
        for code in [0, -1, 1, 2, 7] {
            assert_eq!(TransactionApprovalStatus::from(code).code(), code);
        }
    }

    #[test]
    fn the_words_iyzico_uses_round_trip_and_the_rest_are_kept() {
        for name in ["NOT_REFUNDED", "PARTIALLY_REFUNDED", "TOTALLY_REFUNDED"] {
            assert_eq!(PaymentRefundStatus::from(name).to_string(), name);
        }
        assert_eq!(
            PaymentRefundStatus::from("WRITTEN_OFF"),
            PaymentRefundStatus::Other("WRITTEN_OFF".into())
        );
        for name in ["CANCEL", "PAYMENT", "REFUND"] {
            assert_eq!(TransactionType::from(name).to_string(), name);
        }
        assert_eq!(
            TransactionType::from("CHARGEBACK").to_string(),
            "CHARGEBACK"
        );
    }

    #[test]
    fn an_amount_with_no_currency_beside_it_is_none_and_nothing_is_lost() {
        assert_eq!(money(Some("100.00"), None), None);
        assert_eq!(
            money(Some("100.00"), Some(Currency::Try)),
            Some(Money::parse("100.00", Currency::Try).unwrap())
        );
        assert_eq!(money(None, Some(Currency::Try)), None);
    }
}
