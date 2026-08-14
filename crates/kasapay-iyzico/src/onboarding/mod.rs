//! iyzico Onboarding — creating, updating and reading back a marketplace
//! sub-merchant.
//!
//! A sub-merchant is a **different legal person** taking money through the
//! platform's own iyzico integration — a marketplace's seller, a
//! platform's connected business. So what this module sends is not payment
//! data: it is compliance data — a name, an email, a national ID or a tax
//! number, an IBAN — and what iyzico refuses is not a declined card but a
//! sub-merchant it will not open an account for.
//!
//! # What is here
//!
//! All three operations iyzico documents for a sub-merchant:
//!
//! | | |
//! |---|---|
//! | [`Client::create`] | `POST /onboarding/submerchant` |
//! | [`Client::update`] | `PUT /onboarding/submerchant` |
//! | [`Client::detail`] | `POST /onboarding/submerchant/detail` |
//!
//! # What is not: the Agent API
//!
//! `specs/iyzico/agent/latest.yaml` documents two more operations —
//! `POST /v1/agent/get_auth_key` and `POST /v1/agent/logout` — that were
//! considered for this module and left out. They are not a sub-merchant
//! operation at all: iyzico's own description is a mobile "app2app" session
//! for the PayPOS product, and they authenticate with a **plain
//! `Authorization: Basic sck_…` header and a required `PaynetMobile` header**,
//! neither of which is [`IYZWSv2`](crate::Credentials) signing. Bolting a
//! second authentication scheme onto a client built for a different one, for
//! two calls that have nothing to do with a sub-merchant's paperwork, is not
//! "fitting cleanly" — so they are implemented instead in
//! [`crate::agent`], with [`crate::softpos`] on the session key they answer.
//! Its module documentation goes further than the paragraph above: the two
//! headers are not PayPOS's *only* difference from the classic API — every
//! fragment behind them points at a Paynet host, not iyzico's own.
//!
//! # Three types, not a bag of `Option`s
//!
//! iyzico carries a sub-merchant's kind as one `subMerchantType` string next
//! to a set of fields that are conditionally required depending on it: a
//! personal sub-merchant requires a contact's first and last name, a company
//! requires a tax office and a registered title, and only a limited or
//! joint-stock company requires a tax number. [`NewSubmerchant`]'s three
//! variants — [`PersonalSubmerchant`], [`PrivateCompanySubmerchant`],
//! [`LimitedJointSubmerchant`] — each carry only the fields their own kind
//! requires, so a personal sub-merchant with no contact surname, or a limited
//! company with no tax number, does not compile. [`SubmerchantUpdate`] does
//! the same for an update, with one more constraint an update adds: see
//! [`SubmerchantUpdate`]'s own documentation for why its `PrivateCompany` and
//! `LimitedOrJointStockCompany` variants share one struct.
//!
//! # An IBAN is not a card number, and it is still somebody's banking detail
//!
//! Every place an IBAN travels through this module — [`PersonalSubmerchant::iban`],
//! [`PrivateCompanySubmerchant::iban`], [`LimitedJointSubmerchant::iban`],
//! [`PersonalUpdate::iban`], [`CompanyUpdate::iban`], [`SubmerchantDetail::iban`] —
//! is a [`kasapay_core::Secret`] rather than a plain string, the same guard
//! [`Credentials`](crate::Credentials) uses for an API key: `{:?}` on any of
//! these types prints `Secret(***)` rather than the account number, so a
//! `tracing::debug!("{config:?}")` or a panic message does not put it in a
//! log by accident. It is still readable through
//! [`Secret::expose`](kasapay_core::Secret::expose) for the one place that has
//! to send it.
//!
//! A national ID and a tax number travel as plain `Box<str>`, the same as
//! everywhere else in this crate — [`classic::checkout::Buyer::identity_number`](crate::classic::checkout::Buyer::identity_number)
//! and [`mass::Recipient::IdentityNumber`](crate::mass::Recipient::IdentityNumber)
//! do the same. iyzico's own documentation singles out neither of them for
//! the same handling it gives a bank account, and consistency with the rest
//! of the crate matters more here than a guess about which of two sensitive
//! fields deserves the stronger guard.
//!
//! **Nothing here checks an identifier against a person.** No IBAN checksum,
//! no TCKN check digit, no tax-number format — iyzico documents none of them
//! — and no name is matched against whoever the numbers belong to. A
//! well-formed IBAN or ID belonging to someone else is sent as written, the
//! same caveat [`mass::Recipient`](crate::mass::Recipient) carries for a
//! payout.
//!
//! # It runs on the classic client
//!
//! Onboarding is part of iyzico's classic API: same host, same
//! [`IYZWSv2`](crate::Credentials) request signing, same `status: "failure"`
//! envelope. So [`Client`] is built over a [`classic::Client`](crate::classic::Client)
//! rather than beside it, and shares its credentials, timeout and connection
//! pool.
//!
//! # Nothing here is verified
//!
//! **Onboarding responses carry no signature, so none of this is checked.**
//! The page that lists which fields each endpoint signs — [Response Signature
//! Validation](https://docs.iyzico.com/en/advanced/response-signature-validation)
//! — names payments, 3-D Secure, the checkout form and refunds, and no
//! onboarding endpoint at all; nor does any of the three onboarding schemas
//! document a `signature` field. There is therefore no field list to verify
//! against, and this module invents none.
//!
//! What follows for a caller: a `subMerchantKey`, an IBAN or a tax office read
//! back here is what the connection said it was, not what iyzico can be shown
//! to have said. TLS is what stands between the two.
//!
//! [`Config::allow_unsigned`](crate::classic::Config::allow_unsigned) makes no
//! difference here: it governs endpoints that do sign, and these do not.
//!
//! # No live sub-merchant account was checked against
//!
//! iyzico's marketplace pages document every field's name, type and which are
//! required, and neither language gives a worked example — a request or a
//! response with real values — for any of the three operations. The tests in
//! `tests/onboarding.rs` are therefore built from the schema's own field names
//! with stand-in values, the same as [`mass`](crate::mass)'s `authorize`,
//! `cancel`, `balance` and single-item read, which iyzico leaves undemonstrated
//! the same way. What is checked is the shape iyzico documents; what is not is
//! that a real sandbox account accepts and returns exactly that shape.
//!
//! # What is required changes between creation and an update
//!
//! iyzico accepts a sub-merchant created with no IBAN — its own words: *"If
//! not sent during creation, it must be provided before product approval for
//! payouts"* — but **requires one on an update**, along with an
//! `identityNumber` that creation only requires for a personal sub-merchant.
//! [`SubmerchantUpdate`]'s three types reflect that: `iban` is
//! `Secret` rather than `Option<Secret>` on both [`PersonalUpdate`] and
//! [`CompanyUpdate`], and `identity_number` is required on both, where
//! [`PrivateCompanySubmerchant::identity_number`] and
//! [`LimitedJointSubmerchant::identity_number`] are optional on creation.
//!
//! # Where iyzico's documentation disagrees with itself
//!
//! - **The two company update schemas are the same schema.**
//!   `SubmerchantPrivateCompanyUpdateRequest` and
//!   `SubmerchantLimitedJointUpdateRequest` document the identical required
//!   list, the identical optional fields and the identical names. Nothing in
//!   an update body distinguishes a private company from a limited/joint-stock
//!   one — only the `subMerchantKey` does, by pointing at a sub-merchant whose
//!   type was fixed at creation. [`SubmerchantUpdate::PrivateCompany`] and
//!   [`SubmerchantUpdate::LimitedOrJointStockCompany`] both hold a
//!   [`CompanyUpdate`] rather than two structs that would differ only in name.
//! - **Whether an update needs a tax number.** `taxNumber` is optional on both
//!   company update schemas, even for a limited/joint-stock company, where
//!   creating one requires it. [`CompanyUpdate`] follows the update schema:
//!   `tax_number` is optional there regardless of which variant it is built
//!   for.
//!
//! # Currencies
//!
//! `TRY`, `USD`, `EUR`, `GBP`, `RUB`, `CHF` and `NOK` — the same seven
//! [`iyzilink`](crate::iyzilink) documents, and `specs/README.md`'s currency
//! table confirms it: onboarding and iyzilink are the one product pairing
//! that takes all seven, where payments, subscriptions and mass payout each
//! take fewer. `JPY` and `KWD` are refused by every builder before a socket
//! opens.
//!
//! Reading is the permissive direction: [`SubmerchantDetail::currency`] is
//! `None` for a currency [`Currency`](kasapay_core::Currency) cannot name, and
//! the code iyzico sent stays in [`SubmerchantDetail::raw`].
//!
//! # Example
//!
//! ```no_run
//! use kasapay_iyzico::{Credentials, classic, onboarding};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//! let marketplace = onboarding::Client::new(iyzipay);
//!
//! let seller = onboarding::PersonalSubmerchant::builder(
//!     "seller-914",
//!     "ayse@example.com",
//!     "+905555856935",
//!     "Kadıköy, İstanbul",
//!     "Ayşe",
//!     "Yılmaz",
//!     "11111111110",
//! )
//! .build()?;
//!
//! let created = marketplace
//!     .create(&onboarding::NewSubmerchant::Personal(seller))
//!     .await?;
//!
//! // No IBAN was sent at creation. iyzico will not approve a product for
//! // payouts until one is added, so it goes on next.
//! let with_iban = onboarding::PersonalUpdate::builder(
//!     created.key.clone(),
//!     "ayse@example.com",
//!     "+905555856935",
//!     "Kadıköy, İstanbul",
//!     "TR920086402100002353983528",
//!     "Ayşe",
//!     "Yılmaz",
//!     "11111111110",
//! )
//! .build()?;
//! marketplace
//!     .update(&onboarding::SubmerchantUpdate::Personal(with_iban))
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod submerchant;
mod wire;

#[doc(inline)]
pub use crate::onboarding::client::{Client, Created, SubmerchantDetail};
#[doc(inline)]
pub use crate::onboarding::submerchant::{
    CompanyUpdate, CompanyUpdateBuilder, LimitedJointSubmerchant, LimitedJointSubmerchantBuilder,
    NewSubmerchant, PersonalSubmerchant, PersonalSubmerchantBuilder, PersonalUpdate,
    PersonalUpdateBuilder, PrivateCompanySubmerchant, PrivateCompanySubmerchantBuilder,
    SubmerchantError, SubmerchantKind, SubmerchantUpdate,
};
