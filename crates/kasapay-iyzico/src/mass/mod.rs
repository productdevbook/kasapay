//! iyzico Mass Payout — money going out, to other people's bank accounts.
//!
//! Every other part of this crate takes money in. This one sends it: a list of
//! recipients and amounts, paid out of a separate iyzico balance, to IBANs,
//! phone numbers, identity numbers or iyzico wallets. iyzico gates it —
//! *"internal control and approval are required to use this feature"* — and a
//! merchant asks their support team to switch it on before any of this
//! answers anything but a refusal.
//!
//! # Two steps, and only the first is reversible
//!
//! 1. [`Client::start`] creates the payout. Every valid line is stored `INIT`
//!    and **no money moves.**
//! 2. [`Client::authorize`] sets it going. Every line still `INIT` is queued
//!    for the bank.
//!
//! [`Client::cancel`] undoes step 1 and iyzico documents nothing that undoes
//! step 2. So the gap between them is the only place a mistake is cheap, and
//! [`Client::start`] hands back a [`Started`] whose
//! [`invalid_items`](Started::invalid_items) is meant to be read in it.
//!
//! # What is here
//!
//! All six operations iyzico documents:
//!
//! | | |
//! |---|---|
//! | [`Client::start`] | `POST /v1/mass/payout/init` |
//! | [`Client::authorize`] | `POST /v1/mass/payout/auth` |
//! | [`Client::cancel`] | `POST /v1/mass/payout/cancel/{requestId}` |
//! | [`Client::balance`] | `GET /v1/mass/payout/balance` |
//! | [`Client::payout`] | `POST /v1/mass/payout/retrieve` |
//! | [`Client::item`] | `GET /v1/mass/payout/retrieve/items/{referenceCode}` |
//!
//! # A success is not an acceptance
//!
//! iyzico answers the initialization `status: "success"` and reports the lines
//! it would not take in the same body, as `invalidItems`. Those lines are
//! stored `INVALID`, are never paid, and are not mentioned again by the
//! authorization or by its answer. A caller that treats `Ok` as "the whole
//! list went in" pays some of the people it meant to pay and hears nothing
//! about the rest.
//!
//! The same holds one layer down: a line that reaches `DEPOSIT_SUCCESS` is a
//! line that **failed**. The name describes the money going back to the
//! merchant's balance, not reaching the recipient. See [`ItemStatus`].
//!
//! # It runs on the classic client
//!
//! Mass payout is part of iyzico's classic API: same host, same
//! [`IYZWSv2`](crate::Credentials) request signing, same `status: "failure"`
//! envelope. So [`Client`] is built over a [`classic::Client`](crate::classic::Client)
//! rather than beside it, and shares its credentials, timeout and connection
//! pool.
//!
//! **The query string is not signed.** Every one of the six carries
//! `locale` in the query — including the four that are `POST`s, because unlike
//! [`iyzilink`](crate::iyzilink) and [`subscription`](crate::subscription) the
//! mass payout request bodies document no `locale` field at all — and the
//! signature covers the path alone, with everything from the `?` onwards left
//! out. iyzico's documentation does not say this; their own PHP and Python
//! SDKs both cut the URI at the `?` before hashing, and a signature over the
//! whole URL is refused.
//!
//! # Nothing here is verified
//!
//! **Mass payout responses carry no signature, so none of this is checked.**
//! The page that lists which fields each endpoint signs — [Response Signature
//! Validation](https://docs.iyzico.com/en/advanced/response-signature-validation)
//! — names payments, 3-D Secure, the checkout form and refunds, and no mass
//! payout endpoint at all; nor does any mass payout schema in either language
//! document a `signature` field. There is therefore no field list to verify
//! against, and this module invents none.
//!
//! What follows for a caller: a balance, a status and an amount read back here
//! are what the connection said they were, not what iyzico can be shown to
//! have said. TLS is what stands between the two. That is a worse bargain than
//! it is elsewhere in this crate, because these numbers are the only account
//! of money that has already left.
//!
//! [`Config::allow_unsigned`](crate::classic::Config::allow_unsigned) makes no
//! difference here: it governs endpoints that do sign, and these do not.
//!
//! # What iyzico does not say, and this module will not guess
//!
//! - **What `totalAmount` on a line means.** Their worked example sends
//!   `"value": 100` and reads the same line back as `"totalAmount": 100.50`
//!   with `"commissionAmount": 20.00000000`. So the amount that comes back is
//!   not the amount that went out, the three numbers do not reconcile, and
//!   nothing states whether the commission is added to what the recipient gets
//!   or taken out of it. [`PayoutItem::total_amount`] is the number iyzico
//!   sent and nothing more. Do not settle a ledger against it without checking
//!   one payout by hand first.
//! - **Who bears a line that fails.** `DEPOSIT_SUCCESS` says the amount *and
//!   its commission* came back to the merchant's balance; `DEPOSIT_FAIL` says
//!   they did not, and names nothing that retries it. Whether the commission
//!   is charged for an attempt that paid nobody is not documented for either.
//! - **What the balance is counted in.** [`Client::balance`] answers one
//!   number and no currency, in a product documented for three. This crate
//!   will not decide it is lira: [`Balance::amount`] is the digits, and
//!   [`Balance::in_currency`] is where a caller who knows says so.
//! - **Whether `externalId` really is idempotent.** iyzico calls it the
//!   idempotency key and never says what a second initialization under the
//!   same one does — a second payout, the first one back, or a refusal. Until
//!   that is checked against a live account, treat a retry of
//!   [`Client::start`] as capable of creating a second payout, and read it
//!   back with [`Client::payout`] before authorizing anything.
//! - **The identifier is not checked against a person.** No IBAN checksum is
//!   computed here, because iyzico documents no format for the field, and no
//!   `recipientName` is matched against an account holder by anything either
//!   of them documents. A well-formed IBAN belonging to a stranger is paid.
//!   See [`Recipient`].
//! - **A payout in more than one currency.** The request carries a currency
//!   per line and the answer carries one for the whole payout, so a mixed
//!   payout is a thing that can be sent and cannot be read back. Nothing here
//!   refuses it, because iyzico does not; [`NewPayout::currencies`] is there
//!   to let a caller refuse it themselves.
//!
//! # Where iyzico's documentation disagrees with itself
//!
//! - **Whether an amount is a number or a string.** Every money field is
//!   typed `decimal`, which is not an OpenAPI type — `specs/`'s `type: string`
//!   for these is `scripts/merge_iyzico.py` normalising it, not iyzico's word.
//!   The only concrete evidence either page gives is its worked example, which
//!   sends `"value": 100`, unquoted. So this sends a bare JSON number, written
//!   out of [`Money`](kasapay_core::Money)'s own digits rather than through a
//!   float, and reads either form back.
//! - **`itemType`.** The Turkish reading endpoint requires it, `VALID` or
//!   `INVALID`; the English page for the same endpoint does not mention it.
//!   [`Client::payout`] makes the caller choose and always sends it, which is
//!   the union: an unknown field is ignored far more often than a missing
//!   required one is forgiven.
//! - **Where the paging is.** The schema puts `page`, `size` and `total` at
//!   the top of the answer and the example puts them inside `massPayoutItems`.
//!   [`Payout`] reads both and prefers the top-level one.
//! - **Which page it starts from.** iyzico's example asks for `"page": 0`,
//!   where their subscription listings count from one. This sends what it is
//!   given and documents the zero rather than shifting it.
//! - **What `purpose` may be.** Eight values in English and the first five of
//!   them in Turkish. [`Purpose`] names all eight and says which three are
//!   English-only.
//! - **What is required to create one.** English requires `externalId`,
//!   `conversationId` and at least one item; Turkish requires `externalId` and
//!   `items` with no minimum. [`NewPayout::builder`] takes both identifiers
//!   and [`NewPayoutBuilder::build`] refuses an empty list — the stricter
//!   reading, which satisfies either.
//! - **What a refusal looks like.** No mass payout schema documents an
//!   `errorCode` or an `errorMessage` anywhere, in either language: a failure
//!   is documented as the word `failure` and nothing else. Both are read if
//!   they arrive, because the rest of the classic API sends them, and a
//!   refusal that carries neither becomes an error with this module's own
//!   wording rather than iyzico's.
//! - **`merchantId`.** Typed `integer`, and written `000000` in both worked
//!   examples, which is not a JSON number. Read out of iyzico's own bytes so a
//!   quoted one reads too.
//! - **The reading endpoint's own record.** `specs/iyzico/mass/latest.yaml`
//!   carries the Turkish `POST /v1/mass/payout/retrieve` and the dated index
//!   calls it Turkish-only. It is not: `retrieve-mass-payout.md` documents it
//!   in English too, with a different request. The sweep keeps one fragment
//!   per path and the Turkish one won. Both were read for this module.
//!
//! # Currencies
//!
//! `TRY`, `USD` and `EUR`, and that list comes from the Turkish page alone —
//! the English one types the field `string` and names no currencies at all.
//! Where one page constrains and the other is silent, the constraint is the
//! only thing either of them says, so the other six [`Currency`](kasapay_core::Currency)
//! names are refused by [`NewPayoutItemBuilder::build`] before a socket opens.
//!
//! It is not in `specs/` to be read: the merge keeps one schema per name and
//! took the English `Amount`, so `specs/iyzico/mass/latest.yaml` carries no
//! `enum` on `currency` and mass payout is missing from the currency table in
//! `specs/README.md` altogether. The Turkish page is where it is written.
//! Reading is the permissive direction, as it is for a subscription plan: a
//! line that comes back in something else still reads, with its amount `None`
//! and the digits in `raw`.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money};
//! use kasapay_iyzico::{Credentials, classic, mass};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//! let payouts = mass::Client::new(iyzipay);
//!
//! let payout = mass::NewPayout::builder("odeme-2026-08", "hakedis-914")
//!     .purpose(mass::Purpose::Settlement)
//!     .item(
//!         mass::NewPayoutItem::builder(
//!             "satici-77",
//!             mass::Recipient::Iban {
//!                 iban: "TR920086402100002353983528".into(),
//!                 holder: "Ayşe Yılmaz".into(),
//!             },
//!             Money::parse("100.00", Currency::Try)?,
//!             "Ağustos hakedişi",
//!         )
//!         .build()?,
//!     )
//!     .build()?;
//!
//! let started = payouts.start(&payout).await?;
//! // Nothing has moved yet, and this is the moment to look.
//! if started.invalid_items.is_empty() {
//!     payouts.authorize(&started.request_id).await?;
//! }
//! # Ok(())
//! # }
//! ```

mod client;
mod payout;
mod wire;

#[doc(inline)]
pub use crate::mass::client::{
    Balance, Client, InvalidItem, ItemStatus, Payout, PayoutItem, PayoutStatus, Started, Summary,
};
#[doc(inline)]
pub use crate::mass::payout::{
    ItemFilter, NewPayout, NewPayoutBuilder, NewPayoutItem, NewPayoutItemBuilder, PayoutError,
    PayoutRef, Purpose, Recipient, RecipientKind,
};
