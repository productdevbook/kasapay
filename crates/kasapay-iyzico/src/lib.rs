//! iyzico, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! # Two APIs, not one
//!
//! iyzico runs two that barely resemble each other, and which one a merchant
//! uses decides everything about how their code looks.
//!
//! | | [`in_store`] | the rest |
//! |---|---|---|
//! | What it is | the counter-side flow: a till starts a payment, the payer finishes it in iyzico's app | ordinary card payments, subscriptions, marketplace, card storage, pay-by-link |
//! | Where | [`in_store`] | [`classic`], and [`iyzilink`] over the same client |
//! | Authentication | three plain headers | [`IYZWSv2`](Credentials) request signing |
//! | Currency | Turkish lira only | several |
//! | Implemented here | four of twelve operations | fifteen of eighty-four |
//!
//! A till that cannot hold a secret key safely cannot sign one, which is the
//! likely reason for the split. Whether the plain headers are the current
//! mechanism or a legacy one iyzico has not said, and this crate does not
//! guess.
//!
//! # Retrying a charge is not documented as safe
//!
//! iyzico offers no idempotency key — [`in_store`] refuses one outright with
//! [`ErrorKind::Unsupported`](kasapay_core::ErrorKind::Unsupported) rather than
//! accepting one it cannot honour — and does not document what a reused
//! `orderId` or `conversationId` does.
//!
//! So [`Error::is_retryable`](kasapay_core::Error::is_retryable) can be true
//! for a failure whose retry might take the money twice. A bank timeout is
//! exactly that case: nobody knows whether the first attempt went through.
//! Read the payment back before sending it again.
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
//! Ninety-six operations across eleven groups. Nineteen are implemented.
//!
//! # Not every response is signed
//!
//! iyzico signs the money-moving ones and this crate refuses a signature that
//! does not match. It does not sign all of them: the classic cancel carries no
//! signature, and [`iyzilink`] documents none on any of its seven. Each module
//! says which of its calls are checked and which are only as trustworthy as
//! the connection they arrived over.

pub mod classic;
mod errors;
pub mod in_store;
pub mod iyzilink;
mod signing;

#[doc(inline)]
pub use crate::signing::Credentials;
