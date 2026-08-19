//! iyzico Subscription — the catalogue a recurring charge is sold out of.
//!
//! A subscription is a plan and a subscriber. A **plan** says what is charged
//! and how often; it hangs off a **product**, which is the thing being sold.
//! Both are made here, and a subscription then names a plan's reference code.
//!
//! # What is here
//!
//! The catalogue, all ten operations of it:
//!
//! - [`Client::create_product`], [`Client::update_product`],
//!   [`Client::product`], [`Client::products`], [`Client::delete_product`]
//! - [`Client::create_plan`], [`Client::update_plan`], [`Client::plan`],
//!   [`Client::plans`], [`Client::delete_plan`]
//!
//! # And the subscriptions themselves
//!
//! Thirteen more, which is everything except the one that takes a card number:
//!
//! - [`Client::start_subscription_form`] — the hosted form that starts one,
//!   and [`Client::subscription_form_result`] to read what became of it
//! - [`Client::subscribe`] — a second subscription for somebody iyzico already
//!   holds a card for
//! - [`Client::start_card_update_form`] — the hosted form that replaces the
//!   card a subscription is charged to
//! - [`Client::subscription`], [`Client::subscriptions`],
//!   [`Client::activate`], [`Client::cancel`], [`Client::upgrade`],
//!   [`Client::retry_payment`]
//! - [`Client::subscriber`], [`Client::subscribers`],
//!   [`Client::update_subscriber`]
//!
//! # What is not
//!
//! One operation of twenty-four.
//!
//! | Not implemented | Why |
//! |---|---|
//! | `POST /v2/subscription/initialize` | takes the card number, expiry and CVC on the request, which puts the caller in PCI scope. The same reason the classic API's own non-3-D payment is not implemented, and the same reason `POST /cardstorage/card` is not |
//!
//! Every other way in goes through a form iyzico hosts, so no card number
//! crosses a caller's process to start a subscription, to take a second one,
//! or to change the card an existing one is charged to.
//!
//! # It runs on the classic client
//!
//! Subscription is part of iyzico's classic API: same host, same
//! [`IYZWSv2`](crate::Credentials) request signing, same `status: "failure"`
//! envelope. So [`Client`] is built over a [`classic::Client`](crate::classic::Client)
//! rather than beside it, and shares its credentials, timeout and connection
//! pool.
//!
//! **The query string is not signed.** A read, a delete and a listing carry
//! `locale` and paging in the query, and the signature covers the path alone.
//! iyzico's documentation does not say this; their PHP SDK names this API in
//! the code that decides it — the signed payload is cut at the `?` for a URL
//! holding `subscription` — and a signature over the whole URL is refused.
//!
//! # Nothing here is verified
//!
//! **Subscription responses carry no signature, so none of this is checked.**
//! The page that lists which fields each endpoint signs — [Response Signature
//! Validation](https://docs.iyzico.com/en/advanced/response-signature-validation)
//! — names payments, 3-D Secure, the checkout form and refunds, and no
//! subscription endpoint at all; nor does any subscription schema in either
//! language document a `signature` field, in the API reference or in the
//! product pages. There is therefore no field list to verify against, and this
//! module invents none.
//!
//! What follows for a caller: a plan's price is what the connection said it
//! was, not what iyzico can be shown to have said. TLS is what stands between
//! the two.
//!
//! [`Config::allow_unsigned`](crate::classic::Config::allow_unsigned) makes no
//! difference here: it governs endpoints that do sign, and these do not.
//!
//! # Where iyzico's documentation disagrees with itself
//!
//! - **How a listing is paged.** The product listing documents `page` and
//!   `count` in a JSON **request body on a GET**; the plan listing documents
//!   the same two as **query parameters**. Their PHP SDK sends a query and no
//!   body for both, and that is what [`Client::products`] does.
//! - **`createdDate`.** A plan read on its own carries epoch milliseconds; the
//!   same plan summarised inside a product carries `YYYY-MM-DD hh:mm:ss`. So
//!   [`Product::created_date`] and [`PricingPlan::created_date`] are text — the
//!   bytes iyzico sent, unquoted, and no guess about which page was right.
//! - **`price`.** Typed `decimal` in English and `number` in Turkish for the
//!   same field. It is read from iyzico's own bytes either way, so a plan sent
//!   as `50` and one sent as `"50.00"` both come back as the same
//!   [`Money`](kasapay_core::Money).
//! - **`totalCount`.** An integer on a page of plans and a string on a page of
//!   products. [`Page::total_count`] is an integer for both.
//! - **The default language.** The English pages say `locale` defaults to
//!   `en`; the Turkish pages say `tr`, for the same field. Every call this
//!   module makes sends `locale=tr` explicitly, so the answer does not depend
//!   on which page was right.
//! - **The reference code in the body.** iyzico documents `productReferenceCode`
//!   and `pricingPlanReferenceCode` in the path only; their PHP SDK puts them
//!   in the request body as well. This sends both, which is the union — the
//!   path is what the signature covers either way.
//!
//! # Currencies
//!
//! Plans are documented in `TRY`, `USD` and `EUR`, in both languages, and in
//! no others — narrower than the rest of iyzico takes, and narrower than
//! [`iyzilink`](crate::iyzilink), which documents seven. So a plan priced in
//! roubles, francs, kroner, sterling, yen or dinars is refused before a socket
//! opens, even though [`Currency`](kasapay_core::Currency) names every one of
//! them: what this crate can name and what iyzico will take a subscription in
//! are two different lists.
//!
//! Reading is the permissive direction. A plan that comes back in a currency
//! iyzico does not document for a plan still reads — the amount is a
//! [`Money`](kasapay_core::Money) if `Currency` can name the code, and
//! [`PricingPlan::price`] is `None` with the amount still in `raw` if it
//! cannot. One such plan does not fail a whole listing.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_core::{Currency, Money};
//! use kasapay_iyzico::{Credentials, classic, subscription};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//! let catalogue = subscription::Client::new(iyzipay);
//!
//! let magazine = catalogue
//!     .create_product(
//!         &subscription::NewProduct::builder("A Dergisi")
//!             .describe("Aylık dergi")
//!             .build(),
//!     )
//!     .await?;
//!
//! let plan = catalogue
//!     .create_plan(
//!         &magazine.reference_code,
//!         &subscription::NewPlan::builder(
//!             "A Dergisi aylık",
//!             Money::parse("50.00", Currency::Try)?,
//!             subscription::PaymentInterval::Monthly,
//!         )
//!         .trial_days(14)
//!         .build()?,
//!     )
//!     .await?;
//!
//! println!("subscribe onto {}", plan.reference_code);
//! # Ok(())
//! # }
//! ```

mod catalogue;
mod client;
mod subscriber;
mod wire;

#[doc(inline)]
pub use crate::subscription::catalogue::{
    NewPlan, NewPlanBuilder, NewProduct, NewProductBuilder, PaymentInterval, PlanError,
    PlanPaymentType, PlanUpdate, PlanUpdateBuilder, ProductUpdate, ProductUpdateBuilder,
};
#[doc(inline)]
pub use crate::subscription::client::{
    Client, FormResult, Page, PricingPlan, Product, RecordStatus, SubscriberDetail, Subscription,
    SubscriptionForm, SubscriptionStatus,
};
#[doc(inline)]
pub use crate::subscription::subscriber::{
    Address, InitialStatus, NewSubscription, NewSubscriptionBuilder, Subscriber, SubscriberBuilder,
    Upgrade, UpgradePeriod,
};
