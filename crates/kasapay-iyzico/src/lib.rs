//! iyzico, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! # Two APIs, not one
//!
//! iyzico runs two that barely resemble each other, and which one a merchant
//! uses decides everything about how their code looks.
//!
//! | | [`in_store`] | the rest |
//! |---|---|---|
//! | What it is | the counter-side flow: a till starts a payment, the payer finishes it in iyzico's app | ordinary card payments, subscriptions, marketplace, card storage |
//! | Authentication | three plain headers | [`IYZWSv2`](Credentials) request signing |
//! | Currency | Turkish lira only | several |
//! | Implemented here | four of twelve operations | one of eighty-four |
//!
//! A till that cannot hold a secret key safely cannot sign one, which is the
//! likely reason for the split. Whether the plain headers are the current
//! mechanism or a legacy one iyzico has not said, and this crate does not
//! guess.
//!
//! # Where the types come from
//!
//! iyzico publishes no OpenAPI document. The ones in `specs/iyzico/` are
//! reassembled from the per-endpoint fragments embedded across their whole
//! documentation site, in both languages, one file per part of the API. They
//! record what was documented, not a contract iyzico offers — and they are
//! incomplete in one direction worth knowing about: the authentication scheme
//! is documented on a page carrying no fragment, so a spec that declares no
//! security scheme means the fragment was silent, not that the endpoint is
//! open.
//!
//! Ninety-six operations across eleven groups. Four are implemented.

pub mod classic;
pub mod in_store;
mod signing;

#[doc(inline)]
pub use crate::signing::Credentials;
