//! A sub-merchant, before it is sent, and the answer to reading one back.
//!
//! iyzico's marketplace product has three kinds of sub-merchant — **personal**,
//! **private company**, **limited or joint-stock company** — and the fields it
//! requires differ per kind: a personal account gives a contact's first and
//! last name where a company gives a tax office and a registered title, and a
//! limited/joint-stock company must give a tax number where a private company
//! may leave it out. iyzico carries this as one `subMerchantType` string next
//! to a bag of fields that are conditionally required depending on it. Here it
//! is three Rust types instead: [`NewSubmerchant::Personal`],
//! [`NewSubmerchant::PrivateCompany`] and
//! [`NewSubmerchant::LimitedOrJointStockCompany`] each carry only the fields
//! their kind requires, so a personal account missing a contact surname, or a
//! limited company missing a tax number, is a compile error rather than a
//! `400` from iyzico.
//!
//! # Nothing here checks a number against a person
//!
//! No identity number, tax number or IBAN is checked for a valid format —
//! iyzico documents none — and none is checked against whoever the sub-merchant
//! claims to be. A well-formed IBAN belonging to someone else is sent as
//! written, the same caveat [`Recipient`](crate::mass::Recipient) carries.

use std::fmt;

use kasapay_core::{Currency, Secret};

/// A sub-merchant, before it is created — which of iyzico's three kinds, and
/// the fields that kind requires.
///
/// `subMerchantType` is not a field here: it is which variant this is, and the
/// wire layer writes the word iyzico expects for whichever one was built.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum NewSubmerchant {
    /// A sub-merchant that is a person, identified by a national ID.
    Personal(PersonalSubmerchant),
    /// A sub-merchant that is a company not incorporated as limited or
    /// joint-stock.
    PrivateCompany(PrivateCompanySubmerchant),
    /// A sub-merchant incorporated as a limited or joint-stock company.
    LimitedOrJointStockCompany(LimitedJointSubmerchant),
}

impl NewSubmerchant {
    /// Which of iyzico's three kinds this is.
    #[must_use]
    pub const fn kind(&self) -> SubmerchantKind {
        match self {
            Self::Personal(_) => SubmerchantKind::Personal,
            Self::PrivateCompany(_) => SubmerchantKind::PrivateCompany,
            Self::LimitedOrJointStockCompany(_) => SubmerchantKind::LimitedOrJointStockCompany,
        }
    }

    /// The caller's own identifier for this sub-merchant.
    #[must_use]
    pub fn external_id(&self) -> &str {
        match self {
            Self::Personal(s) => &s.external_id,
            Self::PrivateCompany(s) => &s.external_id,
            Self::LimitedOrJointStockCompany(s) => &s.external_id,
        }
    }
}

/// A personal sub-merchant: a person, identified by a national ID, rather
/// than a company.
///
/// Build one with [`PersonalSubmerchant::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PersonalSubmerchant {
    /// The caller's own identifier for this sub-merchant.
    pub external_id: Box<str>,
    /// Contact email.
    pub email: Box<str>,
    /// Contact phone.
    pub phone: Box<str>,
    /// Contact address.
    pub address: Box<str>,
    /// Contact first name.
    pub contact_name: Box<str>,
    /// Contact last name.
    pub contact_surname: Box<str>,
    /// National ID (TCKN).
    pub identity_number: Box<str>,
    /// The store name, if it differs from the contact's own name.
    pub name: Option<Box<str>>,
    /// Bank account. Must be consistent with `contact_name`/`contact_surname`.
    ///
    /// iyzico accepts a sub-merchant with none, but requires one before it
    /// approves a product for payouts. Never printed: see the module's own
    /// `Debug`.
    pub iban: Option<Secret>,
    /// Settlement currency. `None` sends nothing and iyzico defaults to `TRY`.
    pub currency: Option<Currency>,
    /// Correlation id for request/response.
    pub conversation_id: Option<Box<str>>,
}

impl PersonalSubmerchant {
    /// Starts building a personal sub-merchant.
    ///
    /// Every argument here is one iyzico's documentation requires.
    #[must_use]
    pub fn builder(
        external_id: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        phone: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        contact_name: impl Into<Box<str>>,
        contact_surname: impl Into<Box<str>>,
        identity_number: impl Into<Box<str>>,
    ) -> PersonalSubmerchantBuilder {
        PersonalSubmerchantBuilder {
            external_id: external_id.into(),
            email: email.into(),
            phone: phone.into(),
            address: address.into(),
            contact_name: contact_name.into(),
            contact_surname: contact_surname.into(),
            identity_number: identity_number.into(),
            name: None,
            iban: None,
            currency: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`PersonalSubmerchant`] before it is checked.
#[derive(Debug, Clone)]
pub struct PersonalSubmerchantBuilder {
    external_id: Box<str>,
    email: Box<str>,
    phone: Box<str>,
    address: Box<str>,
    contact_name: Box<str>,
    contact_surname: Box<str>,
    identity_number: Box<str>,
    name: Option<Box<str>>,
    iban: Option<Secret>,
    currency: Option<Currency>,
    conversation_id: Option<Box<str>>,
}

impl PersonalSubmerchantBuilder {
    /// The store name, if it differs from the contact's own name.
    #[must_use]
    pub fn name(mut self, name: impl Into<Box<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The bank account to pay out to. Must be consistent with the contact's
    /// own name.
    #[must_use]
    pub fn iban(mut self, iban: impl Into<Secret>) -> Self {
        self.iban = Some(iban.into());
        self
    }

    /// Settlement currency, from the seven the module documentation lists
    /// under "Currencies".
    #[must_use]
    pub const fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Correlation id, echoed back on the answer.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the sub-merchant and produces it.
    pub fn build(self) -> Result<PersonalSubmerchant, SubmerchantError> {
        blank("email", &self.email)?;
        blank("gsmNumber", &self.phone)?;
        blank("address", &self.address)?;
        blank("contactName", &self.contact_name)?;
        blank("contactSurname", &self.contact_surname)?;
        blank("subMerchantExternalId", &self.external_id)?;
        blank("identityNumber", &self.identity_number)?;
        if let Some(currency) = self.currency {
            onboarding_currency(currency)?;
        }
        Ok(PersonalSubmerchant {
            external_id: self.external_id,
            email: self.email,
            phone: self.phone,
            address: self.address,
            contact_name: self.contact_name,
            contact_surname: self.contact_surname,
            identity_number: self.identity_number,
            name: self.name,
            iban: self.iban,
            currency: self.currency,
            conversation_id: self.conversation_id,
        })
    }
}

/// A private company sub-merchant: a company not incorporated as limited or
/// joint-stock.
///
/// Build one with [`PrivateCompanySubmerchant::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PrivateCompanySubmerchant {
    /// The caller's own identifier for this sub-merchant.
    pub external_id: Box<str>,
    /// Contact email.
    pub email: Box<str>,
    /// Contact phone.
    pub phone: Box<str>,
    /// Contact address.
    pub address: Box<str>,
    /// Tax office.
    pub tax_office: Box<str>,
    /// Registered company title.
    pub legal_company_title: Box<str>,
    /// The store name, if it differs from the legal title.
    pub name: Option<Box<str>>,
    /// Tax number. iyzico documents it as optional here — unlike
    /// [`LimitedJointSubmerchant::tax_number`], which is required.
    pub tax_number: Option<Box<str>>,
    /// National ID (TCKN), for a private company. Not required at creation;
    /// see [`PersonalUpdate`] and [`CompanyUpdate`] for why a caller should
    /// still collect it before the sub-merchant is updated.
    pub identity_number: Option<Box<str>>,
    /// Bank account. Must match `legal_company_title`.
    ///
    /// iyzico accepts a sub-merchant with none, but requires one before it
    /// approves a product for payouts. Never printed: see the module's own
    /// `Debug`.
    pub iban: Option<Secret>,
    /// Settlement currency. `None` sends nothing and iyzico defaults to `TRY`.
    pub currency: Option<Currency>,
    /// Correlation id for request/response.
    pub conversation_id: Option<Box<str>>,
}

impl PrivateCompanySubmerchant {
    /// Starts building a private-company sub-merchant.
    #[must_use]
    pub fn builder(
        external_id: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        phone: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        tax_office: impl Into<Box<str>>,
        legal_company_title: impl Into<Box<str>>,
    ) -> PrivateCompanySubmerchantBuilder {
        PrivateCompanySubmerchantBuilder {
            external_id: external_id.into(),
            email: email.into(),
            phone: phone.into(),
            address: address.into(),
            tax_office: tax_office.into(),
            legal_company_title: legal_company_title.into(),
            name: None,
            tax_number: None,
            identity_number: None,
            iban: None,
            currency: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`PrivateCompanySubmerchant`] before it is checked.
#[derive(Debug, Clone)]
pub struct PrivateCompanySubmerchantBuilder {
    external_id: Box<str>,
    email: Box<str>,
    phone: Box<str>,
    address: Box<str>,
    tax_office: Box<str>,
    legal_company_title: Box<str>,
    name: Option<Box<str>>,
    tax_number: Option<Box<str>>,
    identity_number: Option<Box<str>>,
    iban: Option<Secret>,
    currency: Option<Currency>,
    conversation_id: Option<Box<str>>,
}

impl PrivateCompanySubmerchantBuilder {
    /// The store name, if it differs from the legal title.
    #[must_use]
    pub fn name(mut self, name: impl Into<Box<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Tax number.
    #[must_use]
    pub fn tax_number(mut self, tax_number: impl Into<Box<str>>) -> Self {
        self.tax_number = Some(tax_number.into());
        self
    }

    /// National ID (TCKN).
    #[must_use]
    pub fn identity_number(mut self, identity_number: impl Into<Box<str>>) -> Self {
        self.identity_number = Some(identity_number.into());
        self
    }

    /// The bank account to pay out to. Must match the legal company title.
    #[must_use]
    pub fn iban(mut self, iban: impl Into<Secret>) -> Self {
        self.iban = Some(iban.into());
        self
    }

    /// Settlement currency, from the seven the module documentation lists
    /// under "Currencies".
    #[must_use]
    pub const fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Correlation id, echoed back on the answer.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the sub-merchant and produces it.
    pub fn build(self) -> Result<PrivateCompanySubmerchant, SubmerchantError> {
        blank("email", &self.email)?;
        blank("gsmNumber", &self.phone)?;
        blank("address", &self.address)?;
        blank("taxOffice", &self.tax_office)?;
        blank("legalCompanyTitle", &self.legal_company_title)?;
        blank("subMerchantExternalId", &self.external_id)?;
        if let Some(currency) = self.currency {
            onboarding_currency(currency)?;
        }
        Ok(PrivateCompanySubmerchant {
            external_id: self.external_id,
            email: self.email,
            phone: self.phone,
            address: self.address,
            tax_office: self.tax_office,
            legal_company_title: self.legal_company_title,
            name: self.name,
            tax_number: self.tax_number,
            identity_number: self.identity_number,
            iban: self.iban,
            currency: self.currency,
            conversation_id: self.conversation_id,
        })
    }
}

/// A limited or joint-stock company sub-merchant.
///
/// Build one with [`LimitedJointSubmerchant::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LimitedJointSubmerchant {
    /// The caller's own identifier for this sub-merchant.
    pub external_id: Box<str>,
    /// Contact email.
    pub email: Box<str>,
    /// Contact phone.
    pub phone: Box<str>,
    /// Contact address.
    pub address: Box<str>,
    /// Tax office.
    pub tax_office: Box<str>,
    /// Tax number. Required for this kind — unlike
    /// [`PrivateCompanySubmerchant::tax_number`], which iyzico documents as
    /// optional.
    pub tax_number: Box<str>,
    /// Registered company title.
    pub legal_company_title: Box<str>,
    /// The store name, if it differs from the legal title.
    pub name: Option<Box<str>>,
    /// National ID (TCKN). Not required at creation; see [`CompanyUpdate`] for
    /// why a caller should still collect it before the sub-merchant is
    /// updated.
    pub identity_number: Option<Box<str>>,
    /// Bank account. Must match `legal_company_title`.
    ///
    /// iyzico accepts a sub-merchant with none, but requires one before it
    /// approves a product for payouts. Never printed: see the module's own
    /// `Debug`.
    pub iban: Option<Secret>,
    /// Settlement currency. `None` sends nothing and iyzico defaults to `TRY`.
    pub currency: Option<Currency>,
    /// Correlation id for request/response.
    pub conversation_id: Option<Box<str>>,
}

impl LimitedJointSubmerchant {
    /// Starts building a limited/joint-stock company sub-merchant.
    #[must_use]
    pub fn builder(
        external_id: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        phone: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        tax_office: impl Into<Box<str>>,
        tax_number: impl Into<Box<str>>,
        legal_company_title: impl Into<Box<str>>,
    ) -> LimitedJointSubmerchantBuilder {
        LimitedJointSubmerchantBuilder {
            external_id: external_id.into(),
            email: email.into(),
            phone: phone.into(),
            address: address.into(),
            tax_office: tax_office.into(),
            tax_number: tax_number.into(),
            legal_company_title: legal_company_title.into(),
            name: None,
            identity_number: None,
            iban: None,
            currency: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`LimitedJointSubmerchant`] before it is checked.
#[derive(Debug, Clone)]
pub struct LimitedJointSubmerchantBuilder {
    external_id: Box<str>,
    email: Box<str>,
    phone: Box<str>,
    address: Box<str>,
    tax_office: Box<str>,
    tax_number: Box<str>,
    legal_company_title: Box<str>,
    name: Option<Box<str>>,
    identity_number: Option<Box<str>>,
    iban: Option<Secret>,
    currency: Option<Currency>,
    conversation_id: Option<Box<str>>,
}

impl LimitedJointSubmerchantBuilder {
    /// The store name, if it differs from the legal title.
    #[must_use]
    pub fn name(mut self, name: impl Into<Box<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// National ID (TCKN).
    #[must_use]
    pub fn identity_number(mut self, identity_number: impl Into<Box<str>>) -> Self {
        self.identity_number = Some(identity_number.into());
        self
    }

    /// The bank account to pay out to. Must match the legal company title.
    #[must_use]
    pub fn iban(mut self, iban: impl Into<Secret>) -> Self {
        self.iban = Some(iban.into());
        self
    }

    /// Settlement currency, from the seven the module documentation lists
    /// under "Currencies".
    #[must_use]
    pub const fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Correlation id, echoed back on the answer.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the sub-merchant and produces it.
    pub fn build(self) -> Result<LimitedJointSubmerchant, SubmerchantError> {
        blank("email", &self.email)?;
        blank("gsmNumber", &self.phone)?;
        blank("address", &self.address)?;
        blank("taxOffice", &self.tax_office)?;
        blank("taxNumber", &self.tax_number)?;
        blank("legalCompanyTitle", &self.legal_company_title)?;
        blank("subMerchantExternalId", &self.external_id)?;
        if let Some(currency) = self.currency {
            onboarding_currency(currency)?;
        }
        Ok(LimitedJointSubmerchant {
            external_id: self.external_id,
            email: self.email,
            phone: self.phone,
            address: self.address,
            tax_office: self.tax_office,
            tax_number: self.tax_number,
            legal_company_title: self.legal_company_title,
            name: self.name,
            identity_number: self.identity_number,
            iban: self.iban,
            currency: self.currency,
            conversation_id: self.conversation_id,
        })
    }
}

/// An update to a sub-merchant that already exists.
///
/// `subMerchantType` is not sent on an update — iyzico's documentation says so
/// explicitly — so nothing here carries one either; [`SubmerchantUpdate`]'s
/// variant is what says which kind is being updated. **`iban` is required on
/// every kind here**, where it was optional at creation: iyzico requires an
/// IBAN before it approves a product for payouts, and an update is the
/// documented way to add one that creation left out.
///
/// # `PrivateCompany` and `LimitedOrJointStockCompany` carry the same fields
///
/// iyzico documents `SubmerchantPrivateCompanyUpdateRequest` and
/// `SubmerchantLimitedJointUpdateRequest` with the same required list, the
/// same optional fields, and the same field names — nothing in the update body
/// distinguishes the two kinds of company. So both variants here hold a
/// [`CompanyUpdate`], the one struct their shared schema describes; the enum
/// variant is what a caller uses to say which kind they mean, since nothing in
/// the request iyzico reads would do it for them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum SubmerchantUpdate {
    /// Updates a personal sub-merchant.
    Personal(PersonalUpdate),
    /// Updates a private-company sub-merchant.
    PrivateCompany(CompanyUpdate),
    /// Updates a limited/joint-stock company sub-merchant.
    LimitedOrJointStockCompany(CompanyUpdate),
}

impl SubmerchantUpdate {
    /// Which of iyzico's three kinds this update is for.
    #[must_use]
    pub const fn kind(&self) -> SubmerchantKind {
        match self {
            Self::Personal(_) => SubmerchantKind::Personal,
            Self::PrivateCompany(_) => SubmerchantKind::PrivateCompany,
            Self::LimitedOrJointStockCompany(_) => SubmerchantKind::LimitedOrJointStockCompany,
        }
    }

    /// The `subMerchantKey` this update is for.
    #[must_use]
    pub fn sub_merchant_key(&self) -> &str {
        match self {
            Self::Personal(u) => &u.sub_merchant_key,
            Self::PrivateCompany(u) | Self::LimitedOrJointStockCompany(u) => &u.sub_merchant_key,
        }
    }
}

/// An update to a personal sub-merchant.
///
/// Build one with [`PersonalUpdate::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PersonalUpdate {
    /// The `subMerchantKey` iyzico issued when this sub-merchant was created.
    pub sub_merchant_key: Box<str>,
    /// Contact email.
    pub email: Box<str>,
    /// Contact phone.
    pub phone: Box<str>,
    /// Contact address.
    pub address: Box<str>,
    /// Bank account. Required on an update, unlike at creation.
    ///
    /// Never printed: see the module's own `Debug`.
    pub iban: Secret,
    /// Contact first name.
    pub contact_name: Box<str>,
    /// Contact last name.
    pub contact_surname: Box<str>,
    /// National ID (TCKN).
    pub identity_number: Box<str>,
    /// The store name, if it differs from the contact's own name.
    pub name: Option<Box<str>>,
    /// Settlement currency. `None` leaves it unchanged.
    pub currency: Option<Currency>,
    /// Correlation id for request/response.
    pub conversation_id: Option<Box<str>>,
}

impl PersonalUpdate {
    /// Starts building an update to a personal sub-merchant.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "every argument is a field iyzico's update schema requires; a builder \
                  that took fewer would build a request iyzico refuses"
    )]
    pub fn builder(
        sub_merchant_key: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        phone: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        iban: impl Into<Secret>,
        contact_name: impl Into<Box<str>>,
        contact_surname: impl Into<Box<str>>,
        identity_number: impl Into<Box<str>>,
    ) -> PersonalUpdateBuilder {
        PersonalUpdateBuilder {
            sub_merchant_key: sub_merchant_key.into(),
            email: email.into(),
            phone: phone.into(),
            address: address.into(),
            iban: iban.into(),
            contact_name: contact_name.into(),
            contact_surname: contact_surname.into(),
            identity_number: identity_number.into(),
            name: None,
            currency: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`PersonalUpdate`] before it is checked.
#[derive(Debug, Clone)]
pub struct PersonalUpdateBuilder {
    sub_merchant_key: Box<str>,
    email: Box<str>,
    phone: Box<str>,
    address: Box<str>,
    iban: Secret,
    contact_name: Box<str>,
    contact_surname: Box<str>,
    identity_number: Box<str>,
    name: Option<Box<str>>,
    currency: Option<Currency>,
    conversation_id: Option<Box<str>>,
}

impl PersonalUpdateBuilder {
    /// The store name, if it differs from the contact's own name.
    #[must_use]
    pub fn name(mut self, name: impl Into<Box<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Settlement currency, from the seven the module documentation lists
    /// under "Currencies".
    #[must_use]
    pub const fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Correlation id, echoed back on the answer.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the update and produces it.
    pub fn build(self) -> Result<PersonalUpdate, SubmerchantError> {
        blank("subMerchantKey", &self.sub_merchant_key)?;
        blank("email", &self.email)?;
        blank("gsmNumber", &self.phone)?;
        blank("address", &self.address)?;
        blank_secret("iban", &self.iban)?;
        blank("contactName", &self.contact_name)?;
        blank("contactSurname", &self.contact_surname)?;
        blank("identityNumber", &self.identity_number)?;
        if let Some(currency) = self.currency {
            onboarding_currency(currency)?;
        }
        Ok(PersonalUpdate {
            sub_merchant_key: self.sub_merchant_key,
            email: self.email,
            phone: self.phone,
            address: self.address,
            iban: self.iban,
            contact_name: self.contact_name,
            contact_surname: self.contact_surname,
            identity_number: self.identity_number,
            name: self.name,
            currency: self.currency,
            conversation_id: self.conversation_id,
        })
    }
}

/// An update to a company sub-merchant — private, or limited/joint-stock.
///
/// The same struct serves both: see [`SubmerchantUpdate`] for why. Build one
/// with [`CompanyUpdate::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CompanyUpdate {
    /// The `subMerchantKey` iyzico issued when this sub-merchant was created.
    pub sub_merchant_key: Box<str>,
    /// Contact email.
    pub email: Box<str>,
    /// Contact phone.
    pub phone: Box<str>,
    /// Contact address.
    pub address: Box<str>,
    /// Bank account. Required on an update, unlike at creation.
    ///
    /// Never printed: see the module's own `Debug`.
    pub iban: Secret,
    /// Registered company title.
    pub legal_company_title: Box<str>,
    /// Tax office.
    pub tax_office: Box<str>,
    /// National ID (TCKN). Required on an update, unlike at creation.
    pub identity_number: Box<str>,
    /// The store name, if it differs from the legal title.
    pub name: Option<Box<str>>,
    /// Tax number.
    pub tax_number: Option<Box<str>>,
    /// Settlement currency. `None` leaves it unchanged.
    pub currency: Option<Currency>,
    /// Correlation id for request/response.
    pub conversation_id: Option<Box<str>>,
}

impl CompanyUpdate {
    /// Starts building an update to a company sub-merchant.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "every argument is a field iyzico's update schema requires; a builder \
                  that took fewer would build a request iyzico refuses"
    )]
    pub fn builder(
        sub_merchant_key: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        phone: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        iban: impl Into<Secret>,
        legal_company_title: impl Into<Box<str>>,
        tax_office: impl Into<Box<str>>,
        identity_number: impl Into<Box<str>>,
    ) -> CompanyUpdateBuilder {
        CompanyUpdateBuilder {
            sub_merchant_key: sub_merchant_key.into(),
            email: email.into(),
            phone: phone.into(),
            address: address.into(),
            iban: iban.into(),
            legal_company_title: legal_company_title.into(),
            tax_office: tax_office.into(),
            identity_number: identity_number.into(),
            name: None,
            tax_number: None,
            currency: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`CompanyUpdate`] before it is checked.
#[derive(Debug, Clone)]
pub struct CompanyUpdateBuilder {
    sub_merchant_key: Box<str>,
    email: Box<str>,
    phone: Box<str>,
    address: Box<str>,
    iban: Secret,
    legal_company_title: Box<str>,
    tax_office: Box<str>,
    identity_number: Box<str>,
    name: Option<Box<str>>,
    tax_number: Option<Box<str>>,
    currency: Option<Currency>,
    conversation_id: Option<Box<str>>,
}

impl CompanyUpdateBuilder {
    /// The store name, if it differs from the legal title.
    #[must_use]
    pub fn name(mut self, name: impl Into<Box<str>>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Tax number.
    #[must_use]
    pub fn tax_number(mut self, tax_number: impl Into<Box<str>>) -> Self {
        self.tax_number = Some(tax_number.into());
        self
    }

    /// Settlement currency, from the seven the module documentation lists
    /// under "Currencies".
    #[must_use]
    pub const fn currency(mut self, currency: Currency) -> Self {
        self.currency = Some(currency);
        self
    }

    /// Correlation id, echoed back on the answer.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the update and produces it.
    pub fn build(self) -> Result<CompanyUpdate, SubmerchantError> {
        blank("subMerchantKey", &self.sub_merchant_key)?;
        blank("email", &self.email)?;
        blank("gsmNumber", &self.phone)?;
        blank("address", &self.address)?;
        blank_secret("iban", &self.iban)?;
        blank("legalCompanyTitle", &self.legal_company_title)?;
        blank("taxOffice", &self.tax_office)?;
        blank("identityNumber", &self.identity_number)?;
        if let Some(currency) = self.currency {
            onboarding_currency(currency)?;
        }
        Ok(CompanyUpdate {
            sub_merchant_key: self.sub_merchant_key,
            email: self.email,
            phone: self.phone,
            address: self.address,
            iban: self.iban,
            legal_company_title: self.legal_company_title,
            tax_office: self.tax_office,
            identity_number: self.identity_number,
            name: self.name,
            tax_number: self.tax_number,
            currency: self.currency,
            conversation_id: self.conversation_id,
        })
    }
}

/// Which of iyzico's three sub-merchant types this is.
///
/// Open, because this is also what [`Client::detail`](crate::onboarding::Client::detail)
/// reads a `subMerchantType` string back as, and iyzico may name a kind that
/// did not exist when this was written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubmerchantKind {
    /// A person, identified by a national ID.
    Personal,
    /// A company not incorporated as limited or joint-stock.
    PrivateCompany,
    /// A company incorporated as limited or joint-stock.
    LimitedOrJointStockCompany,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl SubmerchantKind {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Personal => "PERSONAL",
            Self::PrivateCompany => "PRIVATE_COMPANY",
            Self::LimitedOrJointStockCompany => "LIMITED_OR_JOINT_STOCK_COMPANY",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for SubmerchantKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for SubmerchantKind {
    fn from(value: &str) -> Self {
        match value {
            "PERSONAL" => Self::Personal,
            "PRIVATE_COMPANY" => Self::PrivateCompany,
            "LIMITED_OR_JOINT_STOCK_COMPANY" => Self::LimitedOrJointStockCompany,
            other => Self::Other(other.into()),
        }
    }
}

/// The currencies iyzico documents a sub-merchant's settlement in.
///
/// `TRY`, `USD`, `EUR`, `GBP`, `RUB`, `CHF` and `NOK` — the same seven as
/// [`iyzilink`](crate::iyzilink), and `specs/README.md`'s currency table says
/// so. `JPY` and `KWD` are the two [`Currency`] names this refuses.
fn onboarding_currency(currency: Currency) -> Result<(), SubmerchantError> {
    match currency {
        Currency::Try
        | Currency::Usd
        | Currency::Eur
        | Currency::Gbp
        | Currency::Rub
        | Currency::Chf
        | Currency::Nok => Ok(()),
        _ => Err(SubmerchantError::UnsupportedCurrency(currency)),
    }
}

/// Refuses a field iyzico requires and that carries nothing.
fn blank(field: &'static str, value: &str) -> Result<(), SubmerchantError> {
    if value.trim().is_empty() {
        return Err(SubmerchantError::Blank(field));
    }
    Ok(())
}

/// Refuses an IBAN iyzico requires and that carries nothing.
fn blank_secret(field: &'static str, value: &Secret) -> Result<(), SubmerchantError> {
    blank(field, value.expose())
}

/// A sub-merchant, or an update to one, built out of parts iyzico will not
/// accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SubmerchantError {
    /// A field iyzico requires carried nothing.
    #[error("a sub-merchant's `{0}` cannot be blank")]
    Blank(&'static str),
    /// iyzico does not document a sub-merchant settled in this currency.
    #[error("iyzico documents no sub-merchant settlement in {0}")]
    UnsupportedCurrency(Currency),
}

#[cfg(test)]
mod tests {
    use super::{
        CompanyUpdate, LimitedJointSubmerchant, PersonalSubmerchant, PersonalUpdate,
        SubmerchantError, SubmerchantKind,
    };
    use kasapay_core::Currency;

    fn personal() -> PersonalSubmerchant {
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
        .expect("every required field is present")
    }

    #[test]
    fn a_blank_required_field_is_refused_before_a_socket_opens() {
        let error = PersonalSubmerchant::builder(
            "ext-1",
            "",
            "+905555856935",
            "Adres",
            "Ayşe",
            "Yılmaz",
            "11111111110",
        )
        .build()
        .expect_err("an empty email is not an email");
        assert_eq!(error, SubmerchantError::Blank("email"));
    }

    #[test]
    fn a_currency_iyzico_does_not_settle_a_submerchant_in_is_refused() {
        for currency in [Currency::Jpy, Currency::Kwd] {
            let error = PersonalSubmerchant::builder(
                "ext-1",
                "a@b.com",
                "+905555856935",
                "Adres",
                "Ayşe",
                "Yılmaz",
                "11111111110",
            )
            .currency(currency)
            .build()
            .expect_err("iyzico documents no sub-merchant in this currency");
            assert_eq!(error, SubmerchantError::UnsupportedCurrency(currency));
        }
        for currency in [
            Currency::Try,
            Currency::Usd,
            Currency::Eur,
            Currency::Gbp,
            Currency::Rub,
            Currency::Chf,
            Currency::Nok,
        ] {
            assert!(
                PersonalSubmerchant::builder(
                    "ext-1",
                    "a@b.com",
                    "+905555856935",
                    "Adres",
                    "Ayşe",
                    "Yılmaz",
                    "11111111110",
                )
                .currency(currency)
                .build()
                .is_ok(),
                "{currency} is a currency iyzico documents onboarding in"
            );
        }
    }

    #[test]
    fn an_update_without_an_iban_is_refused() {
        let error = PersonalUpdate::builder(
            "key-1",
            "a@b.com",
            "+905555856935",
            "Adres",
            "   ",
            "Ayşe",
            "Yılmaz",
            "11111111110",
        )
        .build()
        .expect_err("an update needs an IBAN even though creation does not");
        assert_eq!(error, SubmerchantError::Blank("iban"));
    }

    #[test]
    fn private_company_and_limited_joint_updates_share_one_shape() {
        // The point under test is that this compiles at all: iyzico documents
        // the same required and optional fields for both, so both variants of
        // SubmerchantUpdate hold the same CompanyUpdate rather than two
        // structs that would only ever differ by name.
        let update = CompanyUpdate::builder(
            "key-1",
            "a@b.com",
            "+905555856935",
            "Adres",
            "TR920086402100002353983528",
            "Acme A.Ş.",
            "Kadıköy",
            "11111111110",
        )
        .build()
        .expect("every required field is present");
        assert_eq!(&*update.sub_merchant_key, "key-1");
    }

    #[test]
    fn a_personal_submerchant_carries_no_tax_fields() {
        // Not a runtime check — a compile-time one. There is nowhere on
        // PersonalSubmerchant to put a taxOffice or a legalCompanyTitle.
        let submerchant = personal();
        assert_eq!(&*submerchant.contact_name, "Ayşe");
    }

    #[test]
    fn the_words_iyzico_uses_round_trip_and_the_rest_are_kept() {
        for name in [
            "PERSONAL",
            "PRIVATE_COMPANY",
            "LIMITED_OR_JOINT_STOCK_COMPANY",
        ] {
            assert_eq!(SubmerchantKind::from(name).to_string(), name);
        }
        assert_eq!(
            SubmerchantKind::from("SOLE_PROPRIETOR"),
            SubmerchantKind::Other("SOLE_PROPRIETOR".into())
        );
    }

    #[test]
    fn a_limited_joint_submerchant_requires_a_tax_number() {
        let error = LimitedJointSubmerchant::builder(
            "ext-1",
            "a@b.com",
            "+905555856935",
            "Adres",
            "Kadıköy",
            "",
            "Acme A.Ş.",
        )
        .build()
        .expect_err("a limited/joint-stock company must give a tax number");
        assert_eq!(error, SubmerchantError::Blank("taxNumber"));
    }
}
