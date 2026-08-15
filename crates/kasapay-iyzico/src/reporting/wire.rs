//! The response bodies of the reporting API, as iyzico documents them.
//!
//! Both operations are `GET`s and carry no request body — everything they
//! take travels on the query string, built in [`crate::reporting::client`].

use serde::Deserialize;
use serde_json::value::RawValue;

/// The answer to `payment/details`.
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentDetailsResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) payments: Option<Vec<Box<RawValue>>>,
}

/// The answer to `payment/transactions`.
#[derive(Debug, Deserialize)]
pub(crate) struct DailyTransactionsResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) transactions: Option<Vec<Box<RawValue>>>,
    #[serde(rename = "currentPage")]
    pub(crate) current_page: Option<i64>,
    #[serde(rename = "totalPageCount")]
    pub(crate) total_page_count: Option<i64>,
}

/// One payment, out of `payment/details`.
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentDetailItem {
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    #[serde(rename = "paymentStatus")]
    pub(crate) payment_status: Option<i64>,
    #[serde(rename = "paymentRefundStatus")]
    pub(crate) payment_refund_status: Option<String>,
    pub(crate) price: Option<String>,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: Option<String>,
    pub(crate) installment: Option<i64>,
    #[serde(rename = "merchantCommissionRate")]
    pub(crate) merchant_commission_rate: Option<String>,
    #[serde(rename = "merchantCommissionRateAmount")]
    pub(crate) merchant_commission_rate_amount: Option<String>,
    #[serde(rename = "iyziCommissionRateAmount")]
    pub(crate) iyzi_commission_rate_amount: Option<String>,
    #[serde(rename = "iyziCommissionFee")]
    pub(crate) iyzi_commission_fee: Option<String>,
    #[serde(rename = "paymentConversationId")]
    pub(crate) payment_conversation_id: Option<String>,
    #[serde(rename = "fraudStatus")]
    pub(crate) fraud_status: Option<i64>,
    #[serde(rename = "cardType")]
    pub(crate) card_type: Option<String>,
    #[serde(rename = "cardAssociation")]
    pub(crate) card_association: Option<String>,
    #[serde(rename = "cardFamily")]
    pub(crate) card_family: Option<String>,
    #[serde(rename = "binNumber")]
    pub(crate) bin_number: Option<String>,
    #[serde(rename = "lastFourDigits")]
    pub(crate) last_four_digits: Option<String>,
    #[serde(rename = "basketId")]
    pub(crate) basket_id: Option<String>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "connectorName")]
    pub(crate) connector_name: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "threeDS")]
    pub(crate) three_ds: Option<bool>,
    pub(crate) phase: Option<String>,
    #[serde(rename = "acquirerBankName")]
    pub(crate) acquirer_bank_name: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
    #[serde(rename = "createdDate")]
    pub(crate) created_date: Option<String>,
    pub(crate) cancels: Option<Vec<CancelItem>>,
    #[serde(rename = "itemTransactions")]
    pub(crate) item_transactions: Option<Vec<ItemTransactionItem>>,
}

/// One of `payments[].cancels`.
#[derive(Debug, Deserialize)]
pub(crate) struct CancelItem {
    #[serde(rename = "refundId")]
    pub(crate) refund_id: Option<Box<RawValue>>,
    #[serde(rename = "cancelConversationId")]
    pub(crate) cancel_conversation_id: Option<String>,
    #[serde(rename = "refundPrice")]
    pub(crate) refund_price: Option<String>,
    #[serde(rename = "refundStatus")]
    pub(crate) refund_status: Option<i64>,
    #[serde(rename = "createdDate")]
    pub(crate) created_date: Option<String>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
}

/// One of `payments[].itemTransactions`.
#[derive(Debug, Deserialize)]
pub(crate) struct ItemTransactionItem {
    #[serde(rename = "paymentTransactionId")]
    pub(crate) payment_transaction_id: Option<String>,
    #[serde(rename = "transactionStatus")]
    pub(crate) transaction_status: Option<i64>,
    pub(crate) price: Option<String>,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: Option<String>,
    #[serde(rename = "merchantCommissionRate")]
    pub(crate) merchant_commission_rate: Option<String>,
    #[serde(rename = "merchantCommissionRateAmount")]
    pub(crate) merchant_commission_rate_amount: Option<String>,
    #[serde(rename = "iyziCommissionRateAmount")]
    pub(crate) iyzi_commission_rate_amount: Option<String>,
    #[serde(rename = "iyziCommissionFee")]
    pub(crate) iyzi_commission_fee: Option<String>,
    #[serde(rename = "blockageRate")]
    pub(crate) blockage_rate: Option<String>,
    #[serde(rename = "blockageRateAmountMerchant")]
    pub(crate) blockage_rate_amount_merchant: Option<String>,
    #[serde(rename = "blockageRateAmountSubMerchant")]
    pub(crate) blockage_rate_amount_sub_merchant: Option<String>,
    #[serde(rename = "blockageResolvedDate")]
    pub(crate) blockage_resolved_date: Option<String>,
    #[serde(rename = "subMerchantPrice")]
    pub(crate) sub_merchant_price: Option<String>,
    #[serde(rename = "subMerchantPayoutRate")]
    pub(crate) sub_merchant_payout_rate: Option<String>,
    #[serde(rename = "subMerchantPayoutAmount")]
    pub(crate) sub_merchant_payout_amount: Option<String>,
    #[serde(rename = "merchantPayoutAmount")]
    pub(crate) merchant_payout_amount: Option<String>,
    #[serde(rename = "convertedPayout")]
    pub(crate) converted_payout: Option<ConvertedPayoutItem>,
    pub(crate) refunds: Option<Vec<RefundItem>>,
}

/// `payments[].itemTransactions[].convertedPayout`.
#[derive(Debug, Deserialize)]
pub(crate) struct ConvertedPayoutItem {
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: Option<String>,
    #[serde(rename = "iyziCommissionRateAmount")]
    pub(crate) iyzi_commission_rate_amount: Option<String>,
    #[serde(rename = "iyziCommissionFee")]
    pub(crate) iyzi_commission_fee: Option<String>,
    #[serde(rename = "blockageRateAmountMerchant")]
    pub(crate) blockage_rate_amount_merchant: Option<String>,
    #[serde(rename = "blockageRateAmountSubMerchant")]
    pub(crate) blockage_rate_amount_sub_merchant: Option<String>,
    #[serde(rename = "subMerchantPayoutAmount")]
    pub(crate) sub_merchant_payout_amount: Option<String>,
    #[serde(rename = "merchantPayoutAmount")]
    pub(crate) merchant_payout_amount: Option<String>,
    #[serde(rename = "iyziConversionRate")]
    pub(crate) iyzi_conversion_rate: Option<String>,
    #[serde(rename = "iyziConversionRateAmount")]
    pub(crate) iyzi_conversion_rate_amount: Option<String>,
    pub(crate) currency: Option<String>,
}

/// One of `payments[].itemTransactions[].refunds`.
#[derive(Debug, Deserialize)]
pub(crate) struct RefundItem {
    #[serde(rename = "refundTxId")]
    pub(crate) refund_tx_id: Option<Box<RawValue>>,
    #[serde(rename = "refundConversationId")]
    pub(crate) refund_conversation_id: Option<String>,
    #[serde(rename = "refundPrice")]
    pub(crate) refund_price: Option<String>,
    #[serde(rename = "refundStatus")]
    pub(crate) refund_status: Option<i64>,
    #[serde(rename = "isAfterSettlement")]
    pub(crate) is_after_settlement: Option<bool>,
    #[serde(rename = "createdDate")]
    pub(crate) created_date: Option<String>,
    #[serde(rename = "currencyCode")]
    pub(crate) currency_code: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
    #[serde(rename = "iyziCommissionRateAmount")]
    pub(crate) iyzi_commission_rate_amount: Option<String>,
}

/// One line, out of `payment/transactions`.
#[derive(Debug, Deserialize)]
pub(crate) struct DailyTransactionItem {
    #[serde(rename = "transactionType")]
    pub(crate) transaction_type: Option<String>,
    #[serde(rename = "transactionDate")]
    pub(crate) transaction_date: Option<String>,
    #[serde(rename = "transactionId")]
    pub(crate) transaction_id: Option<String>,
    #[serde(rename = "transactionStatus")]
    pub(crate) transaction_status: Option<i64>,
    #[serde(rename = "afterSettlement")]
    pub(crate) after_settlement: Option<i64>,
    #[serde(rename = "paymentTxId")]
    pub(crate) payment_tx_id: Option<String>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(rename = "paymentPhase")]
    pub(crate) payment_phase: Option<String>,
    pub(crate) price: Option<String>,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: Option<String>,
    #[serde(rename = "transactionCurrency")]
    pub(crate) transaction_currency: Option<String>,
    pub(crate) installment: Option<i64>,
    #[serde(rename = "threeDS")]
    pub(crate) three_ds: Option<bool>,
    #[serde(rename = "settlementCurrency")]
    pub(crate) settlement_currency: Option<String>,
    #[serde(rename = "connectorType")]
    pub(crate) connector_type: Option<String>,
    #[serde(rename = "posOrderId")]
    pub(crate) pos_order_id: Option<String>,
    #[serde(rename = "authCode")]
    pub(crate) auth_code: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
    #[serde(rename = "basketId")]
    pub(crate) basket_id: Option<String>,
    #[serde(rename = "iyzicoCommission")]
    pub(crate) iyzico_commission: Option<String>,
    #[serde(rename = "iyzicoFee")]
    pub(crate) iyzico_fee: Option<String>,
    pub(crate) parity: Option<String>,
    #[serde(rename = "iyzicoConversionAmount")]
    pub(crate) iyzico_conversion_amount: Option<String>,
    #[serde(rename = "merchantPayoutAmount")]
    pub(crate) merchant_payout_amount: Option<String>,
    #[serde(rename = "subMerchantPayoutAmount")]
    pub(crate) sub_merchant_payout_amount: Option<String>,
}

/// A value iyzico wrote as bare JSON, whether that was `123` or `"123"`.
///
/// `refundId` and `refundTxId` are documented `oneOf: [string, integer]` —
/// the only two fields in this API typed that loosely — so they arrive as
/// [`RawValue`] and are read with this rather than a single serde type either
/// shape would refuse.
pub(crate) fn text(value: &RawValue) -> &str {
    value.get().trim_matches('"')
}
