//! The onboarding client.

use kasapay_core::{Currency, Error, ErrorKind, ProviderId, Raw, Secret};
use reqwest::Method;

use crate::classic;
use crate::onboarding::submerchant::{NewSubmerchant, SubmerchantKind, SubmerchantUpdate};
use crate::onboarding::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where a sub-merchant is created, updated and looked up.
const SUBMERCHANT: &str = "/onboarding/submerchant";
/// Where a sub-merchant's own details are read back.
const SUBMERCHANT_DETAIL: &str = "/onboarding/submerchant/detail";

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
