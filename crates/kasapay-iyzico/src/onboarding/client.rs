//! The onboarding client.

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Raw, Secret};
use reqwest::Method;
use serde_json::value::RawValue;

use crate::classic;
use crate::classic::instalments::number_as_money;
use crate::onboarding::submerchant::{NewSubmerchant, SubmerchantKind, SubmerchantUpdate};
use crate::onboarding::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// The language iyzico answers in. Every request in this module sends it.
const LOCALE: &str = "tr";

/// Where a sub-merchant is created, updated and looked up.
const SUBMERCHANT: &str = "/onboarding/submerchant";
/// Where a sub-merchant's own details are read back.
const SUBMERCHANT_DETAIL: &str = "/onboarding/submerchant/detail";
/// Letting one basket line's payout go to the sub-merchant.
const ITEM_APPROVE: &str = "/payment/iyzipos/item/approve";
/// Taking that permission back.
const ITEM_DISAPPROVE: &str = "/payment/iyzipos/item/disapprove";
/// Changing what a sub-merchant is to be paid for one line.
const ITEM: &str = "/payment/item";

/// Talks to iyzico's marketplace onboarding API — creating, updating and
/// reading back a sub-merchant.
///
/// Built over a [`classic::Client`], because that is what onboarding is: the
/// same host, the same [`IYZWSv2`](crate::Credentials) signing, the same
/// `status: "failure"` envelope. Cloning shares the one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    classic: classic::Client,
}

impl Client {
    /// Speaks onboarding over a classic client.
    #[must_use]
    pub const fn new(classic: classic::Client) -> Self {
        Self { classic }
    }

    /// The classic client underneath, for everything that is not onboarding.
    #[must_use]
    pub const fn classic(&self) -> &classic::Client {
        &self.classic
    }

    /// Creates a sub-merchant. **No money moves, and none can be paid out
    /// until iyzico approves the product and an IBAN is on file.**
    ///
    /// `POST /onboarding/submerchant`. The request body's shape is
    /// [`NewSubmerchant`]'s three variants, and iyzico answers a
    /// `subMerchantKey`, which [`Client::update`] and every other operation on
    /// this sub-merchant addresses it by from here on.
    pub async fn create(&self, submerchant: &NewSubmerchant) -> Result<Created, Error> {
        let body = wire::CreateBody::from(submerchant);
        let (response, raw) = self
            .classic
            .request::<_, wire::CreateResponse>(Method::POST, SUBMERCHANT, "", Some(&body))
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to create the sub-merchant",
        ) {
            return Err(error);
        }
        let key = response.sub_merchant_key.ok_or_else(|| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a created sub-merchant carried no subMerchantKey",
            )
        })?;
        Ok(Created {
            key: key.into_boxed_str(),
            raw,
        })
    }

    /// Replaces a sub-merchant's details, by the `subMerchantKey`
    /// [`Client::create`] gave it.
    ///
    /// `PUT /onboarding/submerchant`. iyzico's own words: **do not send
    /// `subMerchantType`** on an update, and [`SubmerchantUpdate`] has nowhere
    /// to put one. An update is a replacement rather than a patch: a field
    /// [`SubmerchantUpdate`] leaves optional and unset is a field iyzico's
    /// documentation does not say is kept — see the module documentation.
    ///
    /// The answer carries nothing but `status`, `locale`, `systemTime` and
    /// `conversationId`, so `Ok(())` means iyzico accepted the change rather
    /// than describing what changed; read [`Client::detail`] afterwards to see
    /// it.
    pub async fn update(&self, update: &SubmerchantUpdate) -> Result<(), Error> {
        let body = wire::UpdateBody::from(update);
        let (response, _) = self
            .classic
            .request::<_, wire::UpdateResponse>(Method::PUT, SUBMERCHANT, "", Some(&body))
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to update the sub-merchant",
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Reads a sub-merchant back, by the `subMerchantExternalId` it was
    /// created with.
    ///
    /// `POST /onboarding/submerchant/detail`. Named by the external id rather
    /// than the `subMerchantKey`: that is the field iyzico's own request
    /// schema takes here, and the only one.
    pub async fn detail(&self, external_id: &str) -> Result<SubmerchantDetail, Error> {
        let body = wire::DetailRequest {
            locale: "tr",
            conversation_id: None,
            sub_merchant_external_id: external_id,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::DetailResponse>(Method::POST, SUBMERCHANT_DETAIL, "", Some(&body))
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message.clone(),
            response.error_code.clone(),
            "iyzico refused to read the sub-merchant",
        ) {
            return Err(error);
        }
        Ok(SubmerchantDetail::read(response, raw))
    }

    /// Lets one basket line's money go to the sub-merchant who earned it.
    ///
    /// `POST /payment/iyzipos/item/approve`. **This is the other half of a
    /// marketplace**, and until it existed this module could open a
    /// sub-merchant's account and never pay it: iyzico holds a split line's
    /// share until the platform says the buyer got what they paid for, and
    /// this is the platform saying it.
    ///
    /// `transaction` is the split's own `paymentTransactionId` — the id
    /// iyzico answers per basket line on the payment, the same one
    /// [`classic::Client::refund_transaction`] refunds a single line by. It is
    /// not a payment id, and a payment with three lines has three of them.
    ///
    /// # Not signed
    ///
    /// iyzico's response here carries no `signature` field, so this answer is
    /// only as trustworthy as the connection it arrived over — the same as
    /// every other operation in this module.
    pub async fn approve_item(&self, transaction: &str) -> Result<ItemAction, Error> {
        let body = wire::ItemActionRequest {
            locale: LOCALE,
            conversation_id: None,
            payment_transaction_id: transaction,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::ItemActionResponse>(Method::POST, ITEM_APPROVE, "", Some(&body))
            .await?;
        Self::read_action(
            response,
            raw,
            transaction,
            "iyzico refused to approve the item",
        )
    }

    /// Takes back the permission [`Client::approve_item`] gave.
    ///
    /// `POST /payment/iyzipos/item/disapprove`, by the same split id. iyzico's
    /// own words are that it revokes the approval of the line — the money goes
    /// back to being held rather than back to the buyer, which is a refund and
    /// a different call.
    pub async fn disapprove_item(&self, transaction: &str) -> Result<ItemAction, Error> {
        let body = wire::ItemActionRequest {
            locale: LOCALE,
            conversation_id: None,
            payment_transaction_id: transaction,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::ItemActionResponse>(Method::POST, ITEM_DISAPPROVE, "", Some(&body))
            .await?;
        Self::read_action(
            response,
            raw,
            transaction,
            "iyzico refused to disapprove the item",
        )
    }

    /// Changes what the sub-merchant is to be paid for one line.
    ///
    /// `PUT /payment/item`. What a marketplace reaches for when the split was
    /// wrong: a commission agreed after the payment, a line the platform is
    /// covering part of. The buyer paid what they paid — this moves the line
    /// between the platform and the sub-merchant, and iyzico answers the whole
    /// arithmetic back.
    ///
    /// `price` is what the sub-merchant is to receive, in the currency the
    /// payment was taken in. iyzico's request has no currency field and
    /// neither does its answer, so every [`Money`] here is in that same one —
    /// the same shape [`classic::Client::instalments`] has, and for the same
    /// reason.
    pub async fn update_item_payout(
        &self,
        transaction: &str,
        submerchant_key: &str,
        price: Money,
    ) -> Result<ItemPayout, Error> {
        price.require_positive().map_err(|e| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a sub-merchant's share is an amount above zero",
            )
            .with_source(e)
        })?;
        let currency = price.currency();
        // The decimal string as a JSON number, without an f64 in the middle.
        let decimal = RawValue::from_string(price.to_decimal_string()).map_err(|e| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "the amount is not a number iyzico can be sent",
            )
            .with_source(e)
        })?;
        let body = wire::ItemPayoutUpdateRequest {
            locale: LOCALE,
            conversation_id: None,
            payment_transaction_id: transaction,
            sub_merchant_key: submerchant_key,
            sub_merchant_price: &decimal,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::ItemPayoutUpdateResponse>(Method::PUT, ITEM, "", Some(&body))
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message.clone(),
            response.error_code.clone(),
            "iyzico refused to change the sub-merchant's share",
        ) {
            return Err(error);
        }
        // The same check the item actions make, and it matters more here: this
        // call moves money between the platform and a seller, and an answer
        // about another split is another seller's arithmetic.
        if let Some(changed) = response.payment_transaction_id.as_deref()
            && changed != transaction
        {
            return Err(Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                format!("asked about split {transaction} and iyzico answered about {changed}"),
            ));
        }
        let money = |value: Option<&RawValue>| {
            value
                .map(|raw| number_as_money(raw.get(), currency))
                .transpose()
        };
        Ok(ItemPayout {
            item_id: response.item_id.map(String::into_boxed_str),
            transaction: transaction.into(),
            submerchant_key: response.sub_merchant_key.map(String::into_boxed_str),
            transaction_status: response.transaction_status,
            price: money(response.price.as_deref())?,
            paid_price: money(response.paid_price.as_deref())?,
            submerchant_price: money(response.sub_merchant_price.as_deref())?,
            submerchant_payout: money(response.sub_merchant_payout_amount.as_deref())?,
            merchant_payout: money(response.merchant_payout_amount.as_deref())?,
            blockage_resolved: response.blockage_resolved_date.map(String::into_boxed_str),
            raw,
        })
    }

    /// Both item actions, which differ in the path and in nothing else.
    fn read_action(
        response: wire::ItemActionResponse,
        raw: Raw,
        asked_about: &str,
        refusal: &str,
    ) -> Result<ItemAction, Error> {
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            refusal,
        ) {
            return Err(error);
        }
        // iyzico echoes the split it acted on. An answer about another line is
        // not this call's answer, and acting on it would pay the wrong seller.
        if let Some(acted) = response.payment_transaction_id.as_deref()
            && acted != asked_about
        {
            return Err(Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                format!("asked about split {asked_about} and iyzico answered about {acted}"),
            ));
        }
        Ok(ItemAction {
            transaction: asked_about.into(),
            raw,
        })
    }
}

/// One split line, after iyzico acted on it.
#[derive(Debug, Clone)]
pub struct ItemAction {
    /// The split the call was about — iyzico's `paymentTransactionId`, echoed
    /// back and checked against what was asked.
    pub transaction: Box<str>,
    /// iyzico's own response, untouched.
    pub raw: Raw,
}

/// What a sub-merchant is to be paid for one line, after a change.
///
/// Every amount is in the currency the payment was taken in: iyzico's answer
/// names none, so this carries the one the caller asked with.
#[derive(Debug, Clone)]
pub struct ItemPayout {
    /// The basket line's own id, as the payment carried it.
    pub item_id: Option<Box<str>>,
    /// The split this changed, echoed from the request.
    pub transaction: Box<str>,
    /// The sub-merchant iyzico says the line belongs to.
    pub submerchant_key: Option<Box<str>>,
    /// iyzico's own code for where the line stands.
    ///
    /// Kept as the number they sent: their schema types it as an integer and
    /// names no values for it anywhere, so there is nothing to map it onto
    /// that would not be invented here.
    pub transaction_status: Option<i64>,
    /// What the line came to on the basket.
    pub price: Option<Money>,
    /// What was collected for it.
    pub paid_price: Option<Money>,
    /// What the sub-merchant is to receive — the figure this call changed.
    pub submerchant_price: Option<Money>,
    /// What will actually reach them after iyzico's blockage, where iyzico
    /// worked it out.
    pub submerchant_payout: Option<Money>,
    /// What reaches the platform for this line.
    pub merchant_payout: Option<Money>,
    /// When iyzico says the blockage on this line lifts, as they wrote it.
    pub blockage_resolved: Option<Box<str>>,
    /// iyzico's own response, untouched.
    pub raw: Raw,
}

impl From<classic::Client> for Client {
    fn from(classic: classic::Client) -> Self {
        Self::new(classic)
    }
}

/// A sub-merchant, as it exists right after [`Client::create`].
#[derive(Debug, Clone)]
pub struct Created {
    /// What iyzico calls this sub-merchant from here on. [`Client::update`]
    /// and [`CompanyUpdate`](crate::onboarding::CompanyUpdate)/
    /// [`PersonalUpdate`](crate::onboarding::PersonalUpdate) both name it by
    /// this — not by the external id it was created with.
    pub key: Box<str>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// Everything iyzico says about a sub-merchant that already exists.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubmerchantDetail {
    /// The caller's own identifier, echoed back.
    pub external_id: Option<Box<str>>,
    /// What iyzico calls this sub-merchant. Feeds [`Client::update`].
    pub key: Option<Box<str>>,
    /// The store name.
    pub name: Option<Box<str>>,
    /// Contact email.
    pub email: Option<Box<str>>,
    /// Contact phone.
    pub phone: Option<Box<str>>,
    /// Contact address.
    pub address: Option<Box<str>>,
    /// Bank account on file, if any.
    ///
    /// Never printed: see the module's own `Debug`. iyzico sends this back in
    /// the clear over TLS; nothing here checks its format, because iyzico
    /// documents none.
    pub iban: Option<Secret>,
    /// The country the bank account is in. iyzico infers this rather than
    /// taking it on [`Client::create`] or [`Client::update`] — neither
    /// request carries a `bankCountry` field.
    pub bank_country: Option<Box<str>>,
    /// Settlement currency. `None` when iyzico named one
    /// [`Currency`] cannot — the digits are still in [`SubmerchantDetail::raw`].
    pub currency: Option<Currency>,
    /// Tax office, for a company.
    pub tax_office: Option<Box<str>>,
    /// Registered company title, for a company.
    pub legal_company_title: Option<Box<str>>,
    /// National ID (TCKN).
    pub identity_number: Option<Box<str>>,
    /// Which of iyzico's three kinds this is.
    pub kind: Option<SubmerchantKind>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

impl SubmerchantDetail {
    fn read(response: wire::DetailResponse, raw: Raw) -> Self {
        let currency = response
            .currency
            .as_deref()
            .and_then(|code| code.parse::<Currency>().ok());
        Self {
            external_id: response
                .sub_merchant_external_id
                .map(String::into_boxed_str),
            key: response.sub_merchant_key.map(String::into_boxed_str),
            name: response.name.map(String::into_boxed_str),
            email: response.email.map(String::into_boxed_str),
            phone: response.gsm_number.map(String::into_boxed_str),
            address: response.address.map(String::into_boxed_str),
            iban: response.iban.map(Secret::new),
            bank_country: response.bank_country.map(String::into_boxed_str),
            currency,
            tax_office: response.tax_office.map(String::into_boxed_str),
            legal_company_title: response.legal_company_title.map(String::into_boxed_str),
            identity_number: response.identity_number.map(String::into_boxed_str),
            kind: response
                .sub_merchant_type
                .as_deref()
                .map(SubmerchantKind::from),
            raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SubmerchantDetail;
    use crate::onboarding::wire;

    #[test]
    fn a_detail_answer_with_an_iban_does_not_print_it() {
        let detail = SubmerchantDetail::read(
            wire::DetailResponse {
                status: Some("success".to_owned()),
                error_code: None,
                error_message: None,
                name: None,
                email: None,
                gsm_number: None,
                address: None,
                iban: Some("TR920086402100002353983528".to_owned()),
                bank_country: None,
                currency: None,
                tax_office: None,
                legal_company_title: None,
                sub_merchant_external_id: None,
                identity_number: None,
                sub_merchant_type: None,
                sub_merchant_key: None,
            },
            kasapay_core::Raw::from_text("{}"),
        );
        let shown = format!("{detail:?}");
        assert!(!shown.contains("TR920086402100002353983528"));
    }
}
