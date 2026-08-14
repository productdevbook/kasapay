//! iyzico Link — a payment as a URL, with no integration behind it.
//!
//! A link is a small hosted product page: a name, a description, a picture and
//! a price. iyzico hosts it, the payer's card never comes near the caller, and
//! the thing to share is the URL that comes back. All seven operations iyzico
//! documents are here:
//!
//! - [`Client::create`] and [`Client::create_fast_link`] — make one
//! - [`Client::get`] and [`Client::list`] — read them back
//! - [`Client::update`] — replace a link's details
//! - [`Client::set_status`] — stop it taking money, or start it again
//! - [`Client::delete`] — remove it
//!
//! # It runs on the classic client
//!
//! iyzilink is part of iyzico's classic API: same host, same
//! [`IYZWSv2`](crate::Credentials) request signing, same `status: "failure"`
//! envelope. So [`Client`] is built over a [`classic::Client`](crate::classic::Client)
//! rather than beside it, and shares its credentials, timeout and connection
//! pool.
//!
//! Two things differ from the rest of the classic API and both are in the
//! signing:
//!
//! - **The query string is not signed.** A read, a delete and a status change
//!   carry `locale` and paging in the query, and the signature covers the path
//!   alone — `/v2/iyzilink/products/{token}`, with everything from the `?`
//!   onwards left out. iyzico's documentation does not say this; their own PHP
//!   and Python SDKs both do it, independently, and a signature over the whole
//!   URL is refused.
//! - **Only a body that exists is signed.** Those three send none, so what is
//!   signed is the random key and the path, exactly as iyzico's authentication
//!   page describes for a request without one.
//!
//! # Nothing here is verified
//!
//! **iyzilink responses carry no signature, so none of this is checked.** The
//! page that lists which fields each endpoint signs — [Response Signature
//! Validation](https://docs.iyzico.com/en/advanced/response-signature-validation)
//! — names payments, 3-D Secure, the checkout form and refunds, and no
//! iyzilink endpoint at all; nor does any iyzilink schema in either language
//! document a `signature` field. There is therefore no field list to verify
//! against, and this module invents none.
//!
//! What follows for a caller: a link's details are what the connection said
//! they were, not what iyzico can be shown to have said. TLS is what stands
//! between the two. Do not settle anything against a `soldCount` or a price
//! read back from here — read the payment itself through
//! [`classic::Client`](crate::classic::Client), which is signed.
//!
//! [`Config::allow_unsigned`](crate::classic::Config::allow_unsigned) makes no
//! difference here: it governs endpoints that do sign, and these do not.
//!
//! # Where iyzico's documentation disagrees with itself
//!
//! - **The default language.** The English pages say the `locale` query
//!   parameter defaults to `en`; the Turkish pages say `tr`, for the same
//!   three endpoints. Every call this module makes sends `locale=tr`
//!   explicitly, so the answer does not depend on which page was right.
//! - **`presetPriceValues`.** The request documents one integer, out of
//!   `10`, `20` or `30`, described as "the preset amounts" — plural. The
//!   response documents an array of decimal strings. Both cannot be the shape
//!   of a field that carries three amounts, so this module sends neither:
//!   [`NewLinkBuilder::flexible_price`] turns a flexible link on and leaves the
//!   amounts to iyzico. They are read back as text on
//!   [`LinkDetails::preset_prices`].
//! - **Currencies.** Links are documented in `TRY`, `USD`, `EUR`, `GBP`,
//!   `RUB`, `CHF` and `NOK`. [`Currency`](kasapay_core::Currency) names four
//!   of those, so a link priced in roubles, francs or kroner cannot be built
//!   here, and one read back in them has [`LinkDetails::price`] `None` with
//!   the amount still in `raw`.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money};
//! use kasapay_iyzico::{Credentials, classic, iyzilink};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//! let links = iyzilink::Client::new(iyzipay);
//!
//! let link = links
//!     .create(
//!         &iyzilink::NewLink::builder(
//!             "Kahve",
//!             "250g filtre kahve",
//!             Money::parse("149.90", Currency::Try)?,
//!             "iVBORw0KGgo=", // the picture, base64
//!         )
//!         .stock(40)
//!         .build()?,
//!     )
//!     .await?;
//!
//! println!("share {:?}", link.url);
//! links.set_status(&link.token, &iyzilink::LinkStatus::Passive).await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod link;
mod wire;

#[doc(inline)]
pub use crate::iyzilink::client::{Client, Link, LinkDetails, LinkKind, LinkPage, LinkStatus};
#[doc(inline)]
pub use crate::iyzilink::link::{
    Category, FastLink, FastLinkBuilder, LinkError, LinkUpdate, LinkUpdateBuilder, NewLink,
    NewLinkBuilder,
};
