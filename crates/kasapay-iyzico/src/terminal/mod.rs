//! iyzico's Terminal API — a cash register driving a physical POS device.
//!
//! Nothing else in this crate looks like this. There is no shopper with a
//! browser and no callback address: a till sends a request, a person standing
//! at a counter presents a card to a terminal named by `deviceUniqueId`, and
//! the answer to that same request says whether the bank approved it. iyzico
//! calls it "a specialized communication protocol… between Cash Register
//! systems (ECR) and Physical POS devices", and as of writing it speaks to one
//! manufacturer's hardware, Pavo.
//!
//! It is also the first module here with an authentication of its own. The
//! classic API signs each request `IYZWSv2`; In-Store sends three plain
//! headers; this one logs in over OAuth2 and carries a bearer token that
//! expires. [`Login`] is that login.
//!
//! # Two modes, and only one of them is here
//!
//! iyzico runs the Terminal API in two shapes named after the tax rulings they
//! satisfy, and says plainly that they are alternatives: *"VUK 507 and VUK 509
//! services must not be used together within the same integration. The model
//! to be used should be determined before integration."*
//!
//! | | VUK 509 | VUK 507 |
//! |---|---|---|
//! | The device is | a payment terminal | a fiscal cash register, with a GMU |
//! | Takes | cards | cards, cash, meal cards, QR |
//! | Produces | no fiscal receipt; e-invoice elsewhere | a fiscal receipt, with the sale's line items |
//! | Paths | `/v2/terminal-host/…` | `/v2/terminal-host/gmu/…` |
//! | Here | the four payment operations | none |
//!
//! **This module speaks VUK 509**, and a caller on the other model wants none
//! of it.
//!
//! # What is here
//!
//! The login, and the payment lifecycle:
//!
//! - [`Login::log_in`], [`Login::authorize`], [`Login::token`], [`Login::refresh`]
//! - [`Client::pay`] — `POST /v2/terminal-host/payment`
//! - [`Client::payment`] — `POST /v2/terminal-host/payment/query-transaction-status`
//! - [`Client::refund`] — `POST /v2/terminal-host/payment/refund`
//! - [`Client::void`] — `POST /v2/terminal-host/payment/void`
//! - [`Client::end_of_day`] — `POST /v2/terminal-host/eod`, which a till runs
//!   once a day: until the batch is closed the day's sales are authorised and
//!   not settled
//!
//! # What is not
//!
//! Nine of iyzico's fourteen `terminal-host` operations, and **all nine are
//! VUK 507**. None is guessed at; each is left for something specific.
//!
//! | Not implemented | Why |
//! |---|---|
//! | `POST /v2/terminal-host/gmu/payment`, `…/refund`, `…/void`, `…/refundable-sale-info`, `…/eod` | VUK 507. A different integration a merchant chooses instead of this one, not a fallback within it. Its sale carries the fiscal document type, every line item with its unit code and VAT group, and the buyer's tax office and number — a receipt, not a payment |
//! | `POST /v2/terminal-host/gmu/partial-payment/start`, `…/add-payment`, `…/complete` | VUK 507 as well, and a stateful flow on top of it: one sale settled by several instruments, held open across calls by a `saleNumber` |
//! | `POST /v2/terminal-host/gmu/payment/query-transaction-status` | VUK 507's own status query. Its answer adds two fields to the VUK 507 sale shape, not this one's |
//!
//! All nine are VUK 507, which is not a gap in this module so much as the
//! other product. They are worth adding together, with the sale-item and buyer
//! types they all share, rather than one call at a time.
//!
//! # The login is at an address that names the wrong product
//!
//! `/in-store/oauth2/authorize`, `/in-store/oauth2/token` and
//! `/in-store/oauth2/token/refresh`. Those paths say In-Store and they are not
//! In-Store's: they are documented under Physical POS → Terminal API
//! Integration → Login, their OpenAPI fragments are titled *Terminal API –
//! Outside Flow* like the `/v2/terminal-host/*` ones, and iyzico's own
//! description of what comes back says the `access_token` is "used as Bearer
//! Token in Terminal Host services". The Turkish page opens by saying it
//! outright: *Terminal API servislerine erişim sağlamak için iyzico OAuth2
//! kimlik doğrulama yapısı kullanılmaktadır.* No `CepPOS` page mentions OAuth in
//! either language.
//!
//! They are filed under `in-store` in `specs/` because that file is grouped by
//! path, and their fragments declare a bare `https://api.iyzipay.com` server
//! with no base path at all. `specs/README.md` says the same at more length.
//!
//! # What a caller supplies, and what iyzico supplies
//!
//! Four secrets, in two pairs. `client_id` and `client_secret` identify the
//! integration and come from iyzico; `username` and `password` identify the
//! till or the person the token acts as. All four go in the body of
//! `/authorize`, and the first pair goes again as HTTP Basic on every token
//! call — so the client secret leaves the machine twice, in two forms, on one
//! login.
//!
//! Then `deviceUniqueId`: the terminal, out of band, from whoever installed
//! it. iyzico's example is `PAV860047264`. Nothing in this API lists the
//! devices a merchant has, so a caller cannot discover one and must be told
//! it.
//!
//! And a `conversationId` and a `transactionReferenceId` per request, both the
//! caller's own. [`Reference`] carries all three.
//!
//! # The token expires, and this crate will not renew it behind your back
//!
//! `expires_in` in iyzico's own example is `7199` — two hours less a second.
//! When it runs out, a call comes back as `100311`, "Access token expired!",
//! which [`Client`] reports as [`ErrorKind::Auth`].
//!
//! [`Client`] does not refresh. It could — it has the token and knows what an
//! expired one looks like — and it would then have to decide what to do with
//! the request that failed. Retrying it is the whole point of doing this
//! automatically, and retrying [`Client::pay`] means putting a terminal back
//! into its card-reading state for a sale that may already have been taken.
//! iyzico offers no idempotency key here and nothing on the answer says
//! whether the first attempt reached the bank. So the caller decides:
//!
//! ```no_run
//! # use std::time::Duration;
//! # use kasapay_iyzico::terminal::{Client, Login, Token};
//! # async fn run(login: &Login, client: &Client, token: &mut Token) -> Result<(), Box<dyn std::error::Error>> {
//! if token.expires_within(Duration::from_secs(120)) {
//!     let renewed = match &token.refresh_token {
//!         Some(refresh) => login.refresh(refresh).await?,
//!         None => login.log_in().await?,
//!     };
//!     client.set_access_token(renewed.access_token.clone());
//!     *token = renewed;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Token::expires_within`] takes a margin for the reason a payment needs
//! one: the call does not return until somebody has presented a card, so a
//! token with four seconds left is not a token to start a sale on.
//!
//! [`Client::set_access_token`] takes `&self` and every clone of the client
//! sees the new token, so the renewing task and the till can be the same
//! client.
//!
//! # Nothing here is verified
//!
//! **Terminal Host responses carry no signature, so none of this is checked.**
//! iyzico's [Response Signature Validation] page names the payment, 3-D
//! Secure, checkout form and refund endpoints of the classic API and no
//! Terminal API endpoint at all, in either language; no `terminal-host` schema
//! documents a `signature` field. There is therefore no field list to verify
//! against, and this module invents none — the same position [`iyzilink`] and
//! [`subscription`] are in, and said so.
//!
//! What follows for a caller: that a sale was approved is what the connection
//! said, not what iyzico can be shown to have said. TLS is what stands between
//! the two.
//!
//! [Response Signature Validation]: https://docs.iyzico.com/en/advanced/response-signature-validation
//! [`iyzilink`]: crate::iyzilink
//! [`subscription`]: crate::subscription
//!
//! # This is not a [`Provider`](kasapay_core::Provider)
//!
//! Deliberately, for two reasons that are the same reason twice.
//!
//! A [`ChargeRequest`](kasapay_core::ChargeRequest) has nowhere to put a
//! terminal. `deviceUniqueId` is not the payer, not the order and not a
//! metadata key — it is which piece of hardware in the shop, and every call
//! here needs it.
//!
//! And [`Payment`] cannot be a [`Charge`](kasapay_core::Charge) honestly: see
//! its own documentation for why a successful answer does not say whether the
//! money was taken or is only being held.
//!
//! # Currencies
//!
//! `TRY`, `USD` and `EUR`. Three, as in [`subscription`], and four fewer than
//! an [`iyzilink`] link.
//!
//! Reaching that number took reading the group rather than the endpoint: the
//! VUK 509 request this module sends types `currency` as a bare string with no
//! `enum`, and the three come from every `currency` in `terminal-host` that
//! carries one — the VUK 507 sale and both its response shapes. Sterling is
//! refused before a socket opens rather than sent to find out.
//!
//! # Where iyzico's documentation disagrees with itself
//!
//! - **`salesType` or `saleType`.** The OpenAPI fragment says `salesType`, in
//!   both languages, on the page that defines the operation. The worked
//!   example on the overview page sends `saleType`. Only one can be the field,
//!   and this sends `salesType` — the fragment is the API description and the
//!   sample is prose beside it. The field is required, so being wrong here is
//!   a refusal rather than a silent default, which is why both are not sent:
//!   an unexpected field is the kind of thing a strict server rejects, and
//!   sending both would risk every call to save a loud failure on one.
//! - **`locale`.** The fragment's `enum` is `tr` and `en`; the same worked
//!   example sends `"TR"`, and the end-of-day example sends `"TR-EN"`, which
//!   is not a value anywhere. [`Locale`] sends the lowercase the `enum` names.
//! - **The query's required fields.** `QueryTransactionStatusRequest` marks
//!   `paymentId`, `deviceUniqueId` and `transactionReferenceId` all required,
//!   and the warning printed beside it says they are "not all mandatory at the
//!   same time" and lists three combinations that work. Error `380111` — "either
//!   `transactionReferenceId` or `paymentId` must be provided" — is what a fourth
//!   gets. [`Query`] is an enum of exactly those three.
//! - **Two addresses renew a token.** `/in-store/oauth2/token` documents a body
//!   of `grant_type=refresh_token`, and `/in-store/oauth2/token/refresh`
//!   documents the same exchange as a service of its own. iyzico says nothing
//!   about which to use or whether they differ. [`Login::refresh`] calls the
//!   dedicated one.
//! - **Renewal exists only in English.** The Turkish Login page documents
//!   `/authorize` and `/token` and then says an expired token means logging in
//!   again. [`Login::log_in`] is that, and it is what a reader of the Turkish
//!   pages alone would have.
//! - **A void answers 422 and a refund answers 400** for what is documented as
//!   the same failure shape. Both are read here.
//! - **`request_timestamp` has no unit.** "The Unix timestamp value of the
//!   relevant request", and nothing more. [`Login::authorize`] sends seconds;
//!   iyzico's classic API writes its own `systemTime` in milliseconds. Not
//!   checked against a live account — and the one of these three whose failure
//!   would not be loud, since a rejected timestamp reads as a login that did
//!   not work. [`Config::timestamps`] is the switch, so a caller with a real
//!   till changes a line rather than this crate.
//!
//! # Not checked against a live account
//!
//! None of it. There is no Terminal API sandbox reachable without hardware and
//! a merchant agreement, so every behaviour above is what iyzico documents.
//! The three worth checking first are `request_timestamp`'s unit, `salesType`,
//! and whether a query by `paymentId` alone really may leave `deviceUniqueId`
//! out — the prose says so and the schema says the opposite.
//!
//! Two of the three fail loudly if the reading is wrong: `salesType` is a
//! required field, so the wrong spelling is a refusal, and a query missing a
//! field iyzico wants is error `380111`. Loud is survivable — the answer
//! arrives with the first call and the fix is one line here. The timestamp is
//! the quiet one, which is why it is the only one with a switch.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money};
//! use kasapay_iyzico::terminal;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let config = terminal::Config::sandbox();
//! let login = terminal::Login::new(
//!     config.clone(),
//!     terminal::Credentials::new("client-id", "client-secret", "kasiyer", "parola"),
//! )?;
//! let token = login.log_in().await?;
//!
//! let till = terminal::Client::new(config, token.access_token.clone())?;
//! let sale = till
//!     .pay(
//!         &terminal::Sale::builder(
//!             terminal::Reference::new("conv-2026-0001", "PAV860047264", "satis-0001"),
//!             Money::parse("149.90", Currency::Try)?,
//!         )
//!         .build()?,
//!     )
//!     .await?;
//!
//! println!("approved as {:?}", sale.payment_id);
//! # Ok(())
//! # }
//! ```

mod client;
mod errors;
mod login;
mod request;
mod wire;

use kasapay_core::{Error, ErrorKind, ProviderId};

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// A request that never got a usable answer.
fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PROVIDER, error.to_string())
}

#[doc(inline)]
pub use crate::terminal::client::{
    BatchTotal, CardType, Client, Config, EndOfDay, EndOfDayRequest, Payment, Timestamps,
};
#[doc(inline)]
pub use crate::terminal::login::{AuthCode, Credentials, Login, Token};
#[doc(inline)]
pub use crate::terminal::request::{
    Locale, Query, Reference, Refund, RefundBuilder, RequestError, Sale, SaleBuilder, SalesType,
    Void, VoidBuilder,
};
