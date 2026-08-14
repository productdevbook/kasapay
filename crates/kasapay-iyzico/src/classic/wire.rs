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

/// `POST /cardstorage/cards`.
#[derive(Debug, Serialize)]
pub(crate) struct CardListRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "cardUserKey")]
    pub(crate) card_user_key: &'a str,
}

/// `DELETE /cardstorage/card`.
#[derive(Debug, Serialize)]
pub(crate) struct CardDeleteRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "cardUserKey")]
    pub(crate) card_user_key: &'a str,
    #[serde(rename = "cardToken")]
    pub(crate) card_token: &'a str,
}

/// The answer to `POST /cardstorage/cards`.
#[derive(Debug, Deserialize)]
pub(crate) struct CardListResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "cardDetails")]
    pub(crate) card_details: Option<Vec<StoredCardItem>>,
}

/// One stored card, as the list reports it.
#[derive(Debug, Deserialize)]
pub(crate) struct StoredCardItem {
    #[serde(rename = "cardToken")]
    pub(crate) card_token: Option<String>,
    #[serde(rename = "cardAlias")]
    pub(crate) card_alias: Option<String>,
    #[serde(rename = "binNumber")]
    pub(crate) bin_number: Option<String>,
    #[serde(rename = "lastFourDigits")]
    pub(crate) last_four_digits: Option<String>,
    #[serde(rename = "cardType")]
    pub(crate) card_type: Option<String>,
    #[serde(rename = "cardAssociation")]
    pub(crate) card_association: Option<String>,
    #[serde(rename = "cardFamily")]
    pub(crate) card_family: Option<String>,
    #[serde(rename = "cardBankName")]
    pub(crate) card_bank_name: Option<String>,
}

/// The envelope every classic operation answers with.
#[derive(Debug, Deserialize)]
pub(crate) struct BaseResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// `POST /payment/iyzipos/checkoutform/initialize/auth/ecom`.
#[derive(Debug, Serialize)]
pub(crate) struct CheckoutFormRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    /// A decimal string. iyzico types these `BigDecimal`; a float would round.
    pub(crate) price: String,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: String,
    pub(crate) currency: &'a str,
    #[serde(rename = "basketId")]
    pub(crate) basket_id: &'a str,
    #[serde(rename = "callbackUrl")]
    pub(crate) callback_url: String,
    #[serde(rename = "enabledInstallments", skip_serializing_if = "Vec::is_empty")]
    pub(crate) enabled_installments: Vec<u8>,
    pub(crate) buyer: BuyerBody<'a>,
    #[serde(rename = "billingAddress")]
    pub(crate) billing_address: AddressBody<'a>,
    #[serde(rename = "shippingAddress")]
    pub(crate) shipping_address: AddressBody<'a>,
    #[serde(rename = "basketItems")]
    pub(crate) basket_items: Vec<BasketItemBody<'a>>,
}

/// The buyer, as the form wants them.
#[derive(Debug, Serialize)]
pub(crate) struct BuyerBody<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) surname: &'a str,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: &'a str,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    #[serde(rename = "registrationAddress")]
    pub(crate) registration_address: &'a str,
    pub(crate) city: &'a str,
    pub(crate) country: &'a str,
    #[serde(rename = "zipCode", skip_serializing_if = "Option::is_none")]
    pub(crate) zip_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) ip: Option<&'a str>,
}

/// An address, as the form wants it.
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

/// One basket line, as the form wants it.
#[derive(Debug, Serialize)]
pub(crate) struct BasketItemBody<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) category1: &'a str,
    #[serde(rename = "itemType")]
    pub(crate) item_type: &'a str,
    pub(crate) price: String,
}

/// The answer to a checkout form initialise.
#[derive(Debug, Deserialize)]
pub(crate) struct CheckoutFormResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) token: Option<String>,
    #[serde(rename = "paymentPageUrl")]
    pub(crate) payment_page_url: Option<String>,
    pub(crate) signature: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
}

/// `POST /payment/auth` — a payment against a card iyzico already holds.
#[derive(Debug, Serialize)]
pub(crate) struct SavedCardPaymentRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    pub(crate) price: String,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: String,
    pub(crate) currency: &'a str,
    #[serde(rename = "basketId")]
    pub(crate) basket_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) installment: Option<u8>,
    #[serde(rename = "paymentCard")]
    pub(crate) payment_card: SavedCardBody<'a>,
    pub(crate) buyer: BuyerBody<'a>,
    #[serde(rename = "billingAddress")]
    pub(crate) billing_address: AddressBody<'a>,
    #[serde(rename = "shippingAddress")]
    pub(crate) shipping_address: AddressBody<'a>,
    #[serde(rename = "basketItems")]
    pub(crate) basket_items: Vec<BasketItemBody<'a>>,
}

/// The `paymentCard` of a stored-card payment: two handles and no card.
///
/// iyzico's `paymentCard` will take a `cardNumber` as well. This type has no
/// field for one, which is what keeps the choice out of the caller's hands.
#[derive(Debug, Serialize)]
pub(crate) struct SavedCardBody<'a> {
    #[serde(rename = "cardUserKey")]
    pub(crate) card_user_key: &'a str,
    #[serde(rename = "cardToken")]
    pub(crate) card_token: &'a str,
}

/// `POST /payment/iyzipos/checkoutform/auth/ecom/detail`.
#[derive(Debug, Serialize)]
pub(crate) struct CheckoutResultRequest<'a> {
    pub(crate) locale: &'a str,
    pub(crate) token: &'a str,
}

/// `POST /payment/detail` — a payment read back by its own id.
#[derive(Debug, Serialize)]
pub(crate) struct PaymentDetailRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
}

/// A payment as iyzico reports it, from the form's result, from the payment
/// itself, or from a stored-card charge.
///
/// `token` is only on the first and `paymentStatus` only on the first two; a
/// stored-card charge says how it went in `status` and `fraudStatus` instead.
/// The three differ in what they sign, not in what they answer.
#[derive(Debug, Deserialize)]
pub(crate) struct PaymentResultResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "paymentStatus")]
    pub(crate) payment_status: Option<String>,
    /// 1 approved, 0 under review, -1 rejected.
    #[serde(rename = "fraudStatus")]
    pub(crate) fraud_status: Option<i64>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    #[serde(rename = "basketId")]
    pub(crate) basket_id: Option<String>,
    #[serde(rename = "paidPrice")]
    pub(crate) paid_price: Option<String>,
    pub(crate) price: Option<String>,
    pub(crate) currency: Option<String>,
    pub(crate) token: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    pub(crate) signature: Option<String>,
}

/// `POST /v2/payment/refund` — an amount off a payment.
#[derive(Debug, Serialize)]
pub(crate) struct RefundRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    pub(crate) price: String,
    pub(crate) currency: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// `POST /payment/refund` — an amount off one line of a payment.
#[derive(Debug, Serialize)]
pub(crate) struct RefundTransactionRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    #[serde(rename = "paymentTransactionId")]
    pub(crate) payment_transaction_id: &'a str,
    pub(crate) price: String,
    pub(crate) currency: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// `POST /payment/cancel`.
#[derive(Debug, Serialize)]
pub(crate) struct CancelRequest<'a> {
    pub(crate) locale: &'a str,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: &'a str,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<&'a str>,
}

/// The answer to either refund, and to a cancel.
#[derive(Debug, Deserialize)]
pub(crate) struct ReversalResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "paymentId")]
    pub(crate) payment_id: Option<String>,
    pub(crate) price: Option<String>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "conversationId")]
    pub(crate) conversation_id: Option<String>,
    #[serde(rename = "hostReference")]
    pub(crate) host_reference: Option<String>,
    /// iyzico's own word on whether a failed reversal is worth sending again.
    pub(crate) retryable: Option<bool>,
    pub(crate) signature: Option<String>,
}
