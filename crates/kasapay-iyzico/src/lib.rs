//! iyzico, behind kasapay's [`Provider`](kasapay_core::Provider) trait.
//!
//! # Four APIs, not one
//!
//! iyzico runs four that barely resemble each other, and which one a merchant
//! uses decides everything about how their code looks — including how it
//! authenticates, because no two of them do it the same way.
//!
//! | | [`in_store`] | [`terminal`] | [`agent`] and [`softpos`] | the rest |
//! |---|---|---|---|---|
//! | What it is | the counter-side flow: a till starts a payment, the payer finishes it in iyzico's app | a cash register driving a physical POS device over the counter | a sale on the payer's own phone, over NFC | ordinary card payments, subscriptions, marketplace, card storage, pay-by-link, mass payout, reporting |
//! | Where | [`in_store`] | [`terminal`] | [`agent`], [`softpos`] | [`classic`], with [`iyzilink`], [`subscription`], [`mass`] and [`reporting`] over the same client |
//! | Authentication | three plain headers | an OAuth2 bearer token that expires | a dealer secret key, then a session key — **not iyzico's own scheme, and not iyzico's own host** | [`IYZWSv2`](Credentials) request signing |
//! | Currency | Turkish lira only | lira, dollars, euro | Turkish lira only, by inference — see [`softpos`] | several |
//! | Implemented here | see `scripts/coverage.py` | the three-call login is filed under In-Store | all five | see `scripts/coverage.py` |
//!
//! A till that cannot hold a secret key safely cannot sign one, which is the
//! likely reason [`in_store`] does not. Whether its plain headers are the
//! current mechanism or a legacy one iyzico has not said, and this crate does
//! not guess.
//!
//! [`agent`] and [`softpos`] are not iyzico's own API at all: every one of
//! their five fragments titles itself `"PayPOS (Paynet) API"` and points at
//! `api.paynet.com.tr`, a Paynet host, not `api.iyzipay.com`. iyzico
//! documents it because it resells it. [`agent`]'s module documentation has
//! the full evidence, including why `specs/iyzico/agent/latest.yaml` and
//! `specs/iyzico/softpos/latest.yaml` show the wrong host at their top level.
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
//! Read the payment back before sending it again — and if the timeout means
//! there is no payment id to read it back *by*,
//! [`Provider::lookup`](kasapay_core::Provider::lookup) on [`classic`] asks
//! iyzico by the `conversationId` the charge was sent with. `Ok(None)` there
//! is iyzico having no record, which is the only honest licence to send it
//! again.
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
//! Ninety-six operations across eleven groups. How many are implemented is
//! not written here, because a number in prose is wrong the moment a module
//! lands: `python3 scripts/coverage.py` counts it from the endpoint strings
//! the code actually calls, and CI fails when one is neither reached nor
//! explained.
//!
//! Grouping is by path, which is why three of [`terminal`]'s belong to a group
//! named after another product: its login sits at `/in-store/oauth2/…` and is
//! filed with In-Store's twelve. `specs/README.md` says why it is the Terminal
//! API's all the same.
//!
//! # Not every response is signed
//!
//! iyzico signs the money-moving ones and this crate refuses a signature that
//! does not match. It does not sign all of them: the classic cancel carries no
//! signature, [`iyzilink`] documents none on any of its seven,
//! [`subscription`] documents none on any of its twenty-four, [`mass`] none on
//! any of its six — including the ones that report where money that has
//! already left got to — [`onboarding`] none on any of its three,
//! [`reporting`] none on either of its two, [`terminal`]
//! none on any of its fourteen, and neither does [`agent`] nor [`softpos`] —
//! Paynet's own [Response Signature Validation]-equivalent page, if one
//! exists, was not found; PayPOS's pages name no signature field at all. Each
//! module says which of its calls are checked and which are only as
//! trustworthy as the connection they arrived over.
//!
//! # Nothing here implements [`Webhook`](kasapay_core::Webhook)
//!
//! Not an omission — the trait cannot express what iyzico's callback needs.
//! [`Webhook::verify`](kasapay_core::Webhook::verify) takes the headers and
//! the bytes of a delivery, and In-Store's callback cannot be opened with
//! those alone: the body is an encrypted `data` blob, and the only thing that
//! opens it is `/crypt/decrypt` **with the `paymentSessionToken` of the
//! payment it belongs to** — a value the merchant stored when the payment was
//! opened, not one the delivery carries.
//!
//! So it is [`in_store::Client::decrypt_callback`], which takes that token,
//! and it stays that way until either iyzico posts something that names its
//! own session or the shared trait grows somewhere to put the caller's own
//! state. Signing *this* callback is not what iyzico does: the In-Store one
//! arrives encrypted rather than signed. Elsewhere they do sign — the classic
//! API signs the *response* to a request, which is what
//! [`Credentials::verify_response`] checks, and its hosted-form callback has a
//! signature of its own that [`classic::signature::Signed::Callback`] names
//! the fields of.
//!
//! [Response Signature Validation]: https://docs.iyzico.com/en/advanced/response-signature-validation

pub mod agent;
pub mod classic;
mod errors;
pub mod in_store;
pub mod iyzilink;
pub mod mass;
pub mod onboarding;
pub mod reporting;
mod signing;
pub mod softpos;
pub mod subscription;
pub mod terminal;

#[doc(inline)]
pub use crate::signing::Credentials;
