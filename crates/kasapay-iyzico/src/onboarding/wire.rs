//! The request and response bodies of the onboarding API, as iyzico documents them.

use serde::{Deserialize, Serialize};

use crate::onboarding::submerchant::{CompanyUpdate, NewSubmerchant, SubmerchantUpdate};

/// `POST /onboarding/submerchant`.
///
/// Untagged: each of the three variants carries its own `subMerchantType`
/// field as an ordinary property, so nothing extra needs to wrap it.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum CreateBody<'a> {
    Personal(PersonalCreateBody<'a>),
    PrivateCompany(PrivateCompanyCreateBody<'a>),
    LimitedJoint(LimitedJointCreateBody<'a>),
}

impl<'a> From<&'a NewSubmerchant> for CreateBody<'a> {
    fn from(value: &'a NewSubmerchant) -> Self {
        match value {
            NewSubmerchant::Personal(s) => Self::Personal(PersonalCreateBody {
                sub_merchant_type: "PERSONAL",
                name: s.name.as_deref(),
                email: &s.email,
                gsm_number: &s.phone,
                address: &s.address,
                iban: s.iban.as_ref().map(kasapay_core::Secret::expose),
                contact_name: &s.contact_name,
                contact_surname: &s.contact_surname,
                sub_merchant_external_id: &s.external_id,
                identity_number: &s.identity_number,
                currency: s.currency.map(kasapay_core::Currency::code),
                locale: "tr",
                conversation_id: s.conversation_id.as_deref(),
            }),
            NewSubmerchant::PrivateCompany(s) => Self::PrivateCompany(PrivateCompanyCreateBody {
                sub_merchant_type: "PRIVATE_COMPANY",
                name: s.name.as_deref(),
                email: &s.email,
                gsm_number: &s.phone,
                address: &s.address,
                iban: s.iban.as_ref().map(kasapay_core::Secret::expose),
                tax_office: &s.tax_office,
                tax_number: s.tax_number.as_deref(),
                legal_company_title: &s.legal_company_title,
                sub_merchant_external_id: &s.external_id,
                identity_number: s.identity_number.as_deref(),
                currency: s.currency.map(kasapay_core::Currency::code),
                locale: "tr",
                conversation_id: s.conversation_id.as_deref(),
            }),
            NewSubmerchant::LimitedOrJointStockCompany(s) => {
                Self::LimitedJoint(LimitedJointCreateBody {
                    sub_merchant_type: "LIMITED_OR_JOINT_STOCK_COMPANY",
                    name: s.name.as_deref(),
                    email: &s.email,
                    gsm_number: &s.phone,
                    address: &s.address,
                    iban: s.iban.as_ref().map(kasapay_core::Secret::expose),
                    tax_office: &s.tax_office,
                    tax_number: &s.tax_number,
                    legal_company_title: &s.legal_company_title,
                    sub_merchant_external_id: &s.external_id,
                    identity_number: s.identity_number.as_deref(),
                    currency: s.currency.map(kasapay_core::Currency::code),
                    locale: "tr",
                    conversation_id: s.conversation_id.as_deref(),
                })
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PersonalCreateBody<'a> {
    #[serde(rename = "subMerchantType")]
    pub(crate) sub_merchant_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<&'a str>,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    pub(crate) address: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) iban: Option<&'a str>,
    #[serde(rename = "contactName")]
    pub(crate) contact_name: &'a str,
    #[serde(rename = "contactSurname")]
    pub(crate) contact_surname: &'a str,
    #[serde(rename = "subMerchantExternalId")]
    pub(crate) sub_merchant_external_id: &'a str,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<&'static str>,
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PrivateCompanyCreateBody<'a> {
    #[serde(rename = "subMerchantType")]
    pub(crate) sub_merchant_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<&'a str>,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    pub(crate) address: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) iban: Option<&'a str>,
    #[serde(rename = "taxOffice")]
    pub(crate) tax_office: &'a str,
    #[serde(rename = "taxNumber", skip_serializing_if = "Option::is_none")]
    pub(crate) tax_number: Option<&'a str>,
    #[serde(rename = "legalCompanyTitle")]
    pub(crate) legal_company_title: &'a str,
    #[serde(rename = "subMerchantExternalId")]
    pub(crate) sub_merchant_external_id: &'a str,
    #[serde(rename = "identityNumber", skip_serializing_if = "Option::is_none")]
    pub(crate) identity_number: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<&'static str>,
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LimitedJointCreateBody<'a> {
    #[serde(rename = "subMerchantType")]
    pub(crate) sub_merchant_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<&'a str>,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    pub(crate) address: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) iban: Option<&'a str>,
    #[serde(rename = "taxOffice")]
    pub(crate) tax_office: &'a str,
    #[serde(rename = "taxNumber")]
    pub(crate) tax_number: &'a str,
    #[serde(rename = "legalCompanyTitle")]
    pub(crate) legal_company_title: &'a str,
    #[serde(rename = "subMerchantExternalId")]
    pub(crate) sub_merchant_external_id: &'a str,
    #[serde(rename = "identityNumber", skip_serializing_if = "Option::is_none")]
    pub(crate) identity_number: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<&'static str>,
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

/// `PUT /onboarding/submerchant`.
///
/// Untagged, and carrying no `subMerchantType`: iyzico's own documentation
/// says explicitly not to send one on an update.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum UpdateBody<'a> {
    Personal(PersonalUpdateBody<'a>),
    Company(CompanyUpdateBody<'a>),
}

impl<'a> From<&'a SubmerchantUpdate> for UpdateBody<'a> {
    fn from(value: &'a SubmerchantUpdate) -> Self {
        match value {
            SubmerchantUpdate::Personal(u) => Self::Personal(PersonalUpdateBody {
                name: u.name.as_deref(),
                email: &u.email,
                gsm_number: &u.phone,
                address: &u.address,
                iban: u.iban.expose(),
                contact_name: &u.contact_name,
                contact_surname: &u.contact_surname,
                identity_number: &u.identity_number,
                sub_merchant_key: &u.sub_merchant_key,
                currency: u.currency.map(kasapay_core::Currency::code),
                locale: "tr",
                conversation_id: u.conversation_id.as_deref(),
            }),
            SubmerchantUpdate::PrivateCompany(u)
            | SubmerchantUpdate::LimitedOrJointStockCompany(u) => {
                Self::Company(company_update_body(u))
            }
        }
    }
}

fn company_update_body(u: &CompanyUpdate) -> CompanyUpdateBody<'_> {
    CompanyUpdateBody {
        name: u.name.as_deref(),
        email: &u.email,
        gsm_number: &u.phone,
        address: &u.address,
        iban: u.iban.expose(),
        tax_office: &u.tax_office,
        legal_company_title: &u.legal_company_title,
        tax_number: u.tax_number.as_deref(),
        sub_merchant_key: &u.sub_merchant_key,
        identity_number: &u.identity_number,
        currency: u.currency.map(kasapay_core::Currency::code),
        locale: "tr",
        conversation_id: u.conversation_id.as_deref(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PersonalUpdateBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<&'a str>,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    pub(crate) address: &'a str,
    pub(crate) iban: &'a str,
    #[serde(rename = "contactName")]
    pub(crate) contact_name: &'a str,
    #[serde(rename = "contactSurname")]
    pub(crate) contact_surname: &'a str,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: &'a str,
    #[serde(rename = "subMerchantKey")]
    pub(crate) sub_merchant_key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<&'static str>,
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CompanyUpdateBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<&'a str>,
    pub(crate) email: &'a str,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: &'a str,
    pub(crate) address: &'a str,
    pub(crate) iban: &'a str,
    #[serde(rename = "taxOffice")]
    pub(crate) tax_office: &'a str,
    #[serde(rename = "legalCompanyTitle")]
    pub(crate) legal_company_title: &'a str,
    #[serde(rename = "taxNumber", skip_serializing_if = "Option::is_none")]
    pub(crate) tax_number: Option<&'a str>,
    #[serde(rename = "subMerchantKey")]
    pub(crate) sub_merchant_key: &'a str,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<&'static str>,
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
}

/// `POST /onboarding/submerchant/detail`.
#[derive(Debug, Serialize)]
pub(crate) struct DetailRequest<'a> {
    pub(crate) locale: &'static str,
    #[serde(rename = "conversationId", skip_serializing_if = "Option::is_none")]
    pub(crate) conversation_id: Option<&'a str>,
    #[serde(rename = "subMerchantExternalId")]
    pub(crate) sub_merchant_external_id: &'a str,
}

/// The answer to a create.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    #[serde(rename = "subMerchantKey")]
    pub(crate) sub_merchant_key: Option<String>,
}

/// The answer to an update.
#[derive(Debug, Deserialize)]
pub(crate) struct UpdateResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
}

/// The answer to a detail read.
#[derive(Debug, Deserialize)]
pub(crate) struct DetailResponse {
    pub(crate) status: Option<String>,
    #[serde(rename = "errorCode")]
    pub(crate) error_code: Option<String>,
    #[serde(rename = "errorMessage")]
    pub(crate) error_message: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) email: Option<String>,
    #[serde(rename = "gsmNumber")]
    pub(crate) gsm_number: Option<String>,
    pub(crate) address: Option<String>,
    pub(crate) iban: Option<String>,
    #[serde(rename = "bankCountry")]
    pub(crate) bank_country: Option<String>,
    pub(crate) currency: Option<String>,
    #[serde(rename = "taxOffice")]
    pub(crate) tax_office: Option<String>,
    #[serde(rename = "legalCompanyTitle")]
    pub(crate) legal_company_title: Option<String>,
    #[serde(rename = "subMerchantExternalId")]
    pub(crate) sub_merchant_external_id: Option<String>,
    #[serde(rename = "identityNumber")]
    pub(crate) identity_number: Option<String>,
    #[serde(rename = "subMerchantType")]
    pub(crate) sub_merchant_type: Option<String>,
    #[serde(rename = "subMerchantKey")]
    pub(crate) sub_merchant_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::CreateBody;
    use crate::onboarding::submerchant::{NewSubmerchant, PersonalSubmerchant};

    #[test]
    fn a_personal_body_carries_the_words_iyzico_documents_and_nothing_it_does_not() {
        let submerchant = NewSubmerchant::Personal(
            PersonalSubmerchant::builder(
                "ext-1",
                "a@b.com",
                "+905555856935",
                "Adres",
                "Ayşe",
                "Yılmaz",
                "11111111110",
            )
            .build()
            .expect("valid"),
        );
        let body = CreateBody::from(&submerchant);
        let json = serde_json::to_value(&body).expect("serialises");
        assert_eq!(json["subMerchantType"], "PERSONAL");
        assert_eq!(json["identityNumber"], "11111111110");
        // Nothing was set, so nothing is sent — not even a null.
        assert!(json.get("iban").is_none());
        assert!(json.get("taxOffice").is_none());
    }
}
