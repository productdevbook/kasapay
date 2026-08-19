//! The subscription client.

use std::fmt;

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Raw};
use reqwest::Method;
use serde_json::value::RawValue;

use crate::classic;
use crate::subscription::catalogue::{
    NewPlan, NewProduct, PaymentInterval, PlanPaymentType, PlanUpdate, ProductUpdate,
};
use crate::subscription::subscriber::{self, InitialStatus, NewSubscription, Subscriber, Upgrade};
use crate::subscription::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where the products live.
const PRODUCTS: &str = "/v2/subscription/products";
/// Where a plan lives once it exists, away from the product it hangs off.
const PLANS: &str = "/v2/subscription/pricing-plans";
/// What kind of plan iyzico takes. Their only value; see [`PlanPaymentType`].
const RECURRING: &str = "RECURRING";
/// What language iyzico answers in.
///
/// Sent on every call rather than left out, because iyzico's two documentation
/// languages disagree about the default: the English pages say `en` and the
/// Turkish ones say `tr`, for the same field on the same endpoint.
const LOCALE: &str = "tr";
/// The query every call that has no body carries.
const LOCALE_QUERY: &str = "?locale=tr";
/// The hosted form that starts a subscription.
const SUBSCRIPTION_FORM: &str = "/v2/subscription/checkoutform/initialize";
/// Where that form's result is read back.
const SUBSCRIPTION_FORM_RESULT: &str = "/v2/subscription/checkoutform";
/// Subscribing somebody iyzico already holds a card for.
const SUBSCRIBE_EXISTING: &str = "/v2/subscription/initialize/with-customer";
/// The hosted form that replaces the card a subscription is charged to.
const CARD_UPDATE_FORM: &str = "/v2/subscription/card-update/checkoutform/initialize";
/// Running subscriptions.
const SUBSCRIPTIONS: &str = "/v2/subscription/subscriptions";
/// The people who hold them.
const SUBSCRIBERS: &str = "/v2/subscription/customers";
/// Taking a failed payment again.
const RETRY: &str = "/v2/subscription/operation/retry";

/// Talks to iyzico's subscription API.
///
/// Built over a [`classic::Client`], because that is what subscription is: the
/// same host, the same [`IYZWSv2`](crate::Credentials) signing, the same
/// `status: "failure"` envelope. Cloning shares the one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    classic: classic::Client,
}

impl Client {
    /// Speaks subscription over a classic client.
    #[must_use]
    pub const fn new(classic: classic::Client) -> Self {
        Self { classic }
    }

    /// The classic client underneath, for everything that is not a catalogue.
    #[must_use]
    pub const fn classic(&self) -> &classic::Client {
        &self.classic
    }

    /// Creates a product for plans to hang off.
    ///
    /// A name is all iyzico asks for, and it must be one no other product of
    /// this merchant has.
    pub async fn create_product(&self, product: &NewProduct) -> Result<Product, Error> {
        let body = wire::ProductRequest {
            locale: LOCALE,
            conversation_id: product.conversation_id.as_deref(),
            name: &product.name,
            description: product.description.as_deref(),
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, PRODUCTS, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to create the product",
            Product::read,
        )
    }

    /// Replaces a product's name and description.
    ///
    /// A replacement rather than a patch: a description left out of
    /// [`ProductUpdate`] is not a description kept.
    pub async fn update_product(
        &self,
        reference: &str,
        update: &ProductUpdate,
    ) -> Result<Product, Error> {
        let path = product_path(reference)?;
        let body = wire::ProductRequest {
            locale: LOCALE,
            conversation_id: update.conversation_id.as_deref(),
            name: &update.name,
            description: update.description.as_deref(),
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, &path, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to update the product",
            Product::read,
        )
    }

    /// Reads one product back, with the plans hanging off it.
    pub async fn product(&self, reference: &str) -> Result<Product, Error> {
        let path = product_path(reference)?;
        let (response, _) = self
            .classic
            .request::<(), wire::Envelope>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        one(
            response,
            "iyzico refused to read the product",
            Product::read,
        )
    }

    /// Lists the merchant's products, one page at a time.
    ///
    /// `page` counts from one and `count` is how many are on it. iyzico
    /// documents no maximum, so none is imposed here; a page or a count of
    /// zero is refused before a socket opens, because iyzico's own SDKs leave
    /// both out of the URL rather than send a zero.
    ///
    /// # The paging is a query, not a body
    ///
    /// iyzico documents this endpoint as taking `page` and `count` in a JSON
    /// **request body on a GET**, and documents the very similar plan listing
    /// as taking them in the **query string**. They cannot both be how it
    /// works. Their PHP SDK builds `?page=&count=` for both and sends no body
    /// on either, so that is what this does.
    pub async fn products(&self, page: u32, count: u32) -> Result<Page<Product>, Error> {
        let query = paging(page, count)?;
        let (response, raw) = self
            .classic
            .request::<(), wire::ListEnvelope>(Method::GET, PRODUCTS, &query, None)
            .await?;
        read_page(
            response,
            raw,
            "iyzico refused the product listing",
            Product::read,
        )
    }

    /// Deletes a product.
    ///
    /// Only a product with no plans left on it: iyzico refuses one that still
    /// has any, and the plans have to go first.
    pub async fn delete_product(&self, reference: &str) -> Result<(), Error> {
        let path = product_path(reference)?;
        self.delete(&path, "iyzico refused to delete the product")
            .await
    }

    /// Creates a pricing plan against a product.
    pub async fn create_plan(
        &self,
        product_reference: &str,
        plan: &NewPlan,
    ) -> Result<PricingPlan, Error> {
        let path = format!("{}/pricing-plans", product_path(product_reference)?);
        let body = wire::CreatePlanRequest {
            locale: LOCALE,
            conversation_id: plan.conversation_id.as_deref(),
            product_reference_code: product_reference,
            name: &plan.name,
            price: plan.price.to_decimal_string(),
            currency_code: plan.price.currency().code(),
            payment_interval: plan.interval.as_str(),
            payment_interval_count: plan.interval_count,
            plan_payment_type: RECURRING,
            recurrence_count: plan.recurrences,
            trial_period_days: plan.trial_days,
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, &path, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to create the plan",
            PricingPlan::read,
        )
    }

    /// Changes a plan's name and trial period.
    ///
    /// Those two and nothing else — see [`PlanUpdate`]. iyzico says
    /// subscriptions already running on the plan are not affected.
    pub async fn update_plan(
        &self,
        reference: &str,
        update: &PlanUpdate,
    ) -> Result<PricingPlan, Error> {
        let path = plan_path(reference)?;
        let body = wire::UpdatePlanRequest {
            locale: LOCALE,
            conversation_id: update.conversation_id.as_deref(),
            pricing_plan_reference_code: reference,
            name: &update.name,
            trial_period_days: update.trial_days,
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, &path, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to update the plan",
            PricingPlan::read,
        )
    }

    /// Reads one plan back.
    pub async fn plan(&self, reference: &str) -> Result<PricingPlan, Error> {
        let path = plan_path(reference)?;
        let (response, _) = self
            .classic
            .request::<(), wire::Envelope>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        one(
            response,
            "iyzico refused to read the plan",
            PricingPlan::read,
        )
    }

    /// Lists a product's plans, one page at a time.
    ///
    /// Paged the same way as [`Client::products`].
    pub async fn plans(
        &self,
        product_reference: &str,
        page: u32,
        count: u32,
    ) -> Result<Page<PricingPlan>, Error> {
        let path = format!("{}/pricing-plans", product_path(product_reference)?);
        let query = paging(page, count)?;
        let (response, raw) = self
            .classic
            .request::<(), wire::ListEnvelope>(Method::GET, &path, &query, None)
            .await?;
        read_page(
            response,
            raw,
            "iyzico refused the plan listing",
            PricingPlan::read,
        )
    }

    /// Deletes a plan.
    ///
    /// Only a plan nothing is subscribed to: iyzico refuses one that carries an
    /// active subscription or a pending update.
    pub async fn delete_plan(&self, reference: &str) -> Result<(), Error> {
        let path = plan_path(reference)?;
        self.delete(&path, "iyzico refused to delete the plan")
            .await
    }

    /// Opens the hosted form that starts a subscription.
    ///
    /// `POST /v2/subscription/checkoutform/initialize`. **This is the way a
    /// subscription is started here**, and the only one that does not put a
    /// card number through the caller's process: iyzico hosts the form, takes
    /// the card on its own page, and posts the outcome to
    /// [`NewSubscription::callback_url`].
    ///
    /// What comes back is a [`SubscriptionForm`]: the token to read the result
    /// with, and iyzico's own HTML for the form. Unlike the classic checkout
    /// form, iyzico answers **no page URL** here — their schema documents
    /// `checkoutFormContent` and nothing to redirect to — so a caller embeds
    /// what they sent rather than sending the payer somewhere.
    ///
    /// [`NewSubscription::initial_status`] decides whether the subscription
    /// starts running when the payer finishes or waits for
    /// [`Client::activate`].
    pub async fn start_subscription_form(
        &self,
        subscription: &NewSubscription,
    ) -> Result<SubscriptionForm, Error> {
        let body = wire::SubscriptionFormRequest {
            locale: LOCALE,
            conversation_id: subscription.conversation_id.as_deref(),
            callback_url: &subscription.callback_url,
            pricing_plan_reference_code: &subscription.plan_reference,
            subscription_initial_status: subscription.initial_status.as_str(),
            customer: subscriber_body(&subscription.subscriber),
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::SubscriptionFormResponse>(
                Method::POST,
                SUBSCRIPTION_FORM,
                "",
                Some(&body),
            )
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to open the subscription form",
        ) {
            return Err(error);
        }
        Ok(SubscriptionForm {
            token: response.token.map(String::into_boxed_str).ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PROVIDER,
                    "an opened subscription form carried no token",
                )
            })?,
            content: response.checkout_form_content.map(String::into_boxed_str),
            expires_in_seconds: response.token_expire_time,
            raw,
        })
    }

    /// Reads what became of a subscription form, by the token it was opened
    /// with.
    ///
    /// `GET /v2/subscription/checkoutform/{token}`. The answer is whatever
    /// iyzico put in `data` — the subscription if the payer finished, and
    /// nothing this crate models beyond that, because their schema documents
    /// the field as an object and names nothing inside it.
    ///
    /// So this hands back [`FormResult::raw`] and the token, and a caller who
    /// needs the subscription reads it there or asks
    /// [`Client::subscriptions`] for the subscriber's own.
    pub async fn subscription_form_result(&self, token: &str) -> Result<FormResult, Error> {
        let path = format!("{SUBSCRIPTION_FORM_RESULT}/{}", path_segment(token)?);
        let (response, raw) = self
            .classic
            .request::<(), wire::Envelope>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to read the subscription form",
        ) {
            return Err(error);
        }
        Ok(FormResult {
            token: token.into(),
            raw,
        })
    }

    /// Subscribes somebody iyzico already holds a card for.
    ///
    /// `POST /v2/subscription/initialize/with-customer`, keyed by a
    /// `customerReferenceCode` — the subscriber iyzico made when an earlier
    /// subscription was started. **No card number crosses this process**, and
    /// none is asked for: iyzico charges the card the earlier subscription
    /// left it with.
    ///
    /// This is the second subscription a customer takes, and the first is
    /// [`Client::start_subscription_form`].
    pub async fn subscribe(
        &self,
        subscriber_reference: &str,
        plan_reference: &str,
        initial_status: InitialStatus,
    ) -> Result<Subscription, Error> {
        let body = wire::SubscribeExistingRequest {
            locale: LOCALE,
            conversation_id: None,
            customer_reference_code: subscriber_reference,
            pricing_plan_reference_code: plan_reference,
            subscription_initial_status: initial_status.as_str(),
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, SUBSCRIBE_EXISTING, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to start the subscription",
            Subscription::read,
        )
    }

    /// Opens the hosted form that replaces the card a subscription is charged
    /// to.
    ///
    /// `POST /v2/subscription/card-update/checkoutform/initialize`. The other
    /// place a card would otherwise have to be typed into a merchant's page,
    /// and iyzico hosts this one too.
    ///
    /// One of `subscription_reference` and `subscriber_reference` is enough:
    /// iyzico documents both as optional and the difference is scope — a
    /// subscription's card, or every subscription that subscriber holds. Both
    /// absent is [`ErrorKind::InvalidRequest`] before a socket opens, because
    /// a card update against nothing is not a request iyzico can answer.
    pub async fn start_card_update_form(
        &self,
        subscription_reference: Option<&str>,
        subscriber_reference: Option<&str>,
        callback_url: &str,
    ) -> Result<SubscriptionForm, Error> {
        if subscription_reference.is_none() && subscriber_reference.is_none() {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a card update names the subscription or the subscriber whose card is \
                 being replaced, and this named neither",
            ));
        }
        let body = wire::CardUpdateFormRequest {
            locale: LOCALE,
            callback_url,
            customer_reference_code: subscriber_reference,
            subscription_reference_code: subscription_reference,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::SubscriptionFormResponse>(
                Method::POST,
                CARD_UPDATE_FORM,
                "",
                Some(&body),
            )
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to open the card update form",
        ) {
            return Err(error);
        }
        Ok(SubscriptionForm {
            token: response.token.map(String::into_boxed_str).ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PROVIDER,
                    "an opened card update form carried no token",
                )
            })?,
            content: response.checkout_form_content.map(String::into_boxed_str),
            expires_in_seconds: response.token_expire_time,
            raw,
        })
    }

    /// Reads one subscription back.
    pub async fn subscription(&self, reference: &str) -> Result<Subscription, Error> {
        let path = format!("{SUBSCRIPTIONS}/{}", path_segment(reference)?);
        let (response, _) = self
            .classic
            .request::<(), wire::Envelope>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        one(
            response,
            "iyzico refused to read the subscription",
            Subscription::read,
        )
    }

    /// Lists subscriptions, one page at a time.
    ///
    /// `page` counts from one, the same as every other listing here. iyzico
    /// documents six filters on this endpoint — by subscription, subscriber,
    /// plan, parent, status and a date range — and none is sent: they go into
    /// the query string, which is not signed, and a caller who wants one can
    /// filter what comes back. Adding them would mean deciding what a date
    /// looks like on iyzico's side, which their own documentation does not say.
    pub async fn subscriptions(&self, page: u32, count: u32) -> Result<Page<Subscription>, Error> {
        let query = paging(page, count)?;
        let (response, raw) = self
            .classic
            .request::<(), wire::ListEnvelope>(Method::GET, SUBSCRIPTIONS, &query, None)
            .await?;
        read_page(
            response,
            raw,
            "iyzico refused the subscription listing",
            Subscription::read,
        )
    }

    /// Starts a subscription that was created pending.
    ///
    /// `POST /v2/subscription/subscriptions/{ref}/activate`. **The first
    /// payment is taken here**, not when the subscription was created — which
    /// is the whole point of [`InitialStatus::Pending`].
    pub async fn activate(&self, reference: &str) -> Result<(), Error> {
        self.act_on(
            reference,
            "activate",
            "iyzico refused to activate the subscription",
        )
        .await
    }

    /// Stops a subscription.
    ///
    /// `POST /v2/subscription/subscriptions/{ref}/cancel`. No further payments
    /// are taken; what has already been paid is not given back, which is a
    /// refund and
    /// [`classic::Client::refund`](crate::classic::Client::refund).
    pub async fn cancel(&self, reference: &str) -> Result<(), Error> {
        self.act_on(
            reference,
            "cancel",
            "iyzico refused to cancel the subscription",
        )
        .await
    }

    /// Moves a subscription to another plan.
    ///
    /// `POST /v2/subscription/subscriptions/{ref}/upgrade`. [`Upgrade`] is
    /// where the three decisions with money in them live: when it takes
    /// effect, whether the new plan's trial applies to somebody who has
    /// already paid, and whether the count of payments starts again.
    pub async fn upgrade(&self, reference: &str, upgrade: &Upgrade) -> Result<(), Error> {
        let path = format!("{SUBSCRIPTIONS}/{}/upgrade", path_segment(reference)?);
        let body = wire::UpgradeRequest {
            locale: LOCALE,
            new_pricing_plan_reference_code: &upgrade.plan_reference,
            upgrade_period: upgrade.period.as_str(),
            use_trial: upgrade.use_trial,
            reset_recurrence_count: upgrade.reset_recurrence_count,
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Ack>(Method::POST, &path, "", Some(&body))
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to upgrade the subscription",
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Takes a subscription payment that failed again.
    ///
    /// `POST /v2/subscription/operation/retry`, keyed by the **order's**
    /// reference code rather than the subscription's — iyzico's `referenceCode`
    /// here names one period's payment, which is what failed.
    ///
    /// A retry takes money. iyzico documents no idempotency mechanism for it,
    /// so read the subscription back before sending a second one.
    pub async fn retry_payment(&self, order_reference: &str) -> Result<(), Error> {
        let body = wire::RetryRequest {
            locale: LOCALE,
            reference_code: order_reference,
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Ack>(Method::POST, RETRY, "", Some(&body))
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to take the payment again",
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Reads one subscriber back.
    pub async fn subscriber(&self, reference: &str) -> Result<SubscriberDetail, Error> {
        let path = format!("{SUBSCRIBERS}/{}", path_segment(reference)?);
        let (response, _) = self
            .classic
            .request::<(), wire::Envelope>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        one(
            response,
            "iyzico refused to read the subscriber",
            SubscriberDetail::read,
        )
    }

    /// Lists subscribers, one page at a time.
    pub async fn subscribers(
        &self,
        page: u32,
        count: u32,
    ) -> Result<Page<SubscriberDetail>, Error> {
        let query = paging(page, count)?;
        let (response, raw) = self
            .classic
            .request::<(), wire::ListEnvelope>(Method::GET, SUBSCRIBERS, &query, None)
            .await?;
        read_page(
            response,
            raw,
            "iyzico refused the subscriber listing",
            SubscriberDetail::read,
        )
    }

    /// Replaces what iyzico holds about a subscriber.
    ///
    /// `POST /v2/subscription/customers/{ref}`. A replacement rather than a
    /// patch, the same as [`Client::update_product`]: iyzico's request carries
    /// every field, so one left out of [`Subscriber`] is one cleared.
    ///
    /// **This is not how the card is changed.** That is
    /// [`Client::start_card_update_form`], because a card number does not go
    /// through here.
    pub async fn update_subscriber(
        &self,
        reference: &str,
        subscriber: &Subscriber,
    ) -> Result<SubscriberDetail, Error> {
        let path = format!("{SUBSCRIBERS}/{}", path_segment(reference)?);
        let body = subscriber_body(subscriber);
        let (response, _) = self
            .classic
            .request::<_, wire::Envelope>(Method::POST, &path, "", Some(&body))
            .await?;
        one(
            response,
            "iyzico refused to update the subscriber",
            SubscriberDetail::read,
        )
    }

    /// Both of the calls that act on a subscription by name alone.
    async fn act_on(&self, reference: &str, action: &str, fallback: &str) -> Result<(), Error> {
        let path = format!("{SUBSCRIPTIONS}/{}/{action}", path_segment(reference)?);
        let (response, _) = self
            .classic
            .request::<(), wire::Ack>(Method::POST, &path, LOCALE_QUERY, None)
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            fallback,
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Deletes whatever is at `path`, carrying nothing.
    ///
    /// Unlike the classic API's card delete, these carry no body: the reference
    /// code is in the path. iyzico's PHP SDK sends `{"locale":…}` here as well
    /// as in the query; nothing they document asks for it, so this sends the
    /// query alone.
    async fn delete(&self, path: &str, fallback: &str) -> Result<(), Error> {
        let (response, _) = self
            .classic
            .request::<(), wire::Ack>(Method::DELETE, path, LOCALE_QUERY, None)
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            fallback,
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl From<classic::Client> for Client {
    fn from(classic: classic::Client) -> Self {
        Self::new(classic)
    }
}

/// A thing a merchant sells subscriptions to.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Product {
    /// What iyzico calls this product, and what every other call names it by.
    pub reference_code: Box<str>,
    /// What the product is called.
    pub name: Option<Box<str>>,
    /// What it is.
    pub description: Option<Box<str>>,
    /// Whether it is in use.
    pub status: Option<RecordStatus>,
    /// When it was created, as iyzico wrote it.
    ///
    /// Left as text: iyzico documents this as `YYYY-MM-DD hh:mm:ss` on a
    /// product and as epoch milliseconds on a plan, so there is no one type
    /// that reads both without a guess about which page was right.
    pub created_date: Option<Box<str>>,
    /// The plans that sell it.
    pub plans: Vec<PricingPlan>,
    /// iyzico's own answer for this product, untouched.
    pub raw: Raw,
}

impl Product {
    /// Reads one product out of the bytes iyzico sent for it.
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::ProductItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a product was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;

        let plans = item.pricing_plans.unwrap_or_default();
        let mut read_plans = Vec::with_capacity(plans.len());
        for plan in &plans {
            read_plans.push(PricingPlan::read(plan)?);
        }

        Ok(Self {
            reference_code: item.reference_code.unwrap_or_default().into_boxed_str(),
            name: item.name.map(String::into_boxed_str),
            description: item.description.map(String::into_boxed_str),
            status: item.status.as_deref().map(RecordStatus::from),
            created_date: item
                .created_date
                .as_deref()
                .map(|date| wire::text(date).into()),
            plans: read_plans,
            raw: Raw::from_text(value.get()),
        })
    }
}

/// What a subscriber pays for a product, and how often.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PricingPlan {
    /// What iyzico calls this plan, and what a subscription names it by.
    pub reference_code: Box<str>,
    /// What the plan is called.
    pub name: Option<Box<str>>,
    /// The product it sells.
    pub product_reference_code: Option<Box<str>>,
    /// What is charged each period.
    ///
    /// `None` when iyzico priced it in a currency
    /// [`Currency`](kasapay_core::Currency) has no name for. The amount is
    /// still in [`PricingPlan::raw`].
    pub price: Option<Money>,
    /// How often the charge happens.
    pub interval: Option<PaymentInterval>,
    /// How many intervals apart the charges are.
    pub interval_count: Option<i64>,
    /// How many charges there are in total, when the plan is not open-ended.
    pub recurrences: Option<i64>,
    /// The free trial, in days, before the first charge.
    pub trial_days: Option<i64>,
    /// What kind of plan it is. iyzico documents one kind.
    pub payment_type: Option<PlanPaymentType>,
    /// Whether it is in use.
    pub status: Option<RecordStatus>,
    /// When it was created, as iyzico wrote it. See [`Product::created_date`].
    pub created_date: Option<Box<str>>,
    /// iyzico's own answer for this plan, untouched.
    pub raw: Raw,
}

impl PricingPlan {
    /// Reads one plan out of the bytes iyzico sent for it.
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::PlanItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a pricing plan was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;

        let price = match (item.price.as_deref(), item.currency_code.as_deref()) {
            (Some(price), Some(code)) => match code.parse::<Currency>() {
                Ok(currency) => Some(
                    Money::parse(wire::text(price), currency)
                        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?,
                ),
                // Refusing a whole listing over one plan in a currency this
                // crate cannot name would be worse than answering None and
                // leaving the amount in raw.
                Err(_) => None,
            },
            _ => None,
        };

        Ok(Self {
            reference_code: item.reference_code.unwrap_or_default().into_boxed_str(),
            name: item.name.map(String::into_boxed_str),
            product_reference_code: item.product_reference_code.map(String::into_boxed_str),
            price,
            interval: item.payment_interval.as_deref().map(PaymentInterval::from),
            interval_count: item.payment_interval_count,
            recurrences: item.recurrence_count,
            trial_days: item.trial_period_days,
            payment_type: item.plan_payment_type.as_deref().map(PlanPaymentType::from),
            status: item.status.as_deref().map(RecordStatus::from),
            created_date: item
                .created_date
                .as_deref()
                .map(|date| wire::text(date).into()),
            raw: Raw::from_text(value.get()),
        })
    }
}

/// A hosted form iyzico has opened — for a new subscription, or for replacing
/// the card an existing one is charged to.
///
/// **There is no URL to send the payer to.** iyzico's schema for both of these
/// documents `checkoutFormContent` and nothing to redirect to, unlike the
/// classic checkout form which answers a `paymentPageUrl`. So a caller embeds
/// what iyzico sent.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionForm {
    /// What reads the result back — [`Client::subscription_form_result`].
    pub token: Box<str>,
    /// iyzico's own HTML for the form.
    pub content: Option<Box<str>>,
    /// How long the token lasts, in seconds, as iyzico said.
    pub expires_in_seconds: Option<i64>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// What a subscription form's own result carried.
///
/// Thin on purpose: iyzico documents this answer's `data` as an object and
/// names no field inside it, in either language. Inventing a shape for it here
/// would be a fixture standing in for a body nobody has seen — the same reason
/// PayTR's instalment rates are untyped — so the body is kept whole and
/// [`Client::subscription`] is what reads a subscription with fields.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormResult {
    /// The token this was read with.
    pub token: Box<str>,
    /// iyzico's own answer, untouched. The subscription is in here.
    pub raw: Raw,
}

/// A running subscription.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Subscription {
    /// What iyzico calls this subscription, and what every other call names it
    /// by.
    pub reference_code: Box<str>,
    /// Where it stands — iyzico's `subscriptionStatus`.
    pub status: Option<SubscriptionStatus>,
    /// The plan it is sold on.
    pub plan_reference: Option<Box<str>>,
    /// What that plan is called.
    pub plan_name: Option<Box<str>>,
    /// The product the plan hangs off.
    pub product_reference: Option<Box<str>>,
    /// What that product is called.
    pub product_name: Option<Box<str>>,
    /// The subscriber, as iyzico names them.
    pub subscriber_reference: Option<Box<str>>,
    /// Their email, as iyzico echoes it.
    pub subscriber_email: Option<Box<str>>,
    /// How many trial days the plan gives.
    pub trial_days: Option<i64>,
    /// When iyzico says it started, exactly as they wrote it.
    ///
    /// Text rather than a timestamp, the same choice [`Product::created_date`]
    /// makes and for the same reason: iyzico writes epoch milliseconds in one
    /// place and `YYYY-MM-DD hh:mm:ss` in another for the same kind of field.
    pub start_date: Option<Box<str>>,
    /// When iyzico says it ends, where it says so.
    pub end_date: Option<Box<str>>,
    /// iyzico's own answer for this subscription, untouched. The periods
    /// (`orders`) are in here.
    pub raw: Raw,
}

impl Subscription {
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::SubscriptionItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a subscription was not the JSON iyzico documents",
            )
            .with_source(e)
        })?;
        Ok(Self {
            reference_code: item
                .reference_code
                .map(String::into_boxed_str)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Malformed,
                        PROVIDER,
                        "a subscription carried no referenceCode",
                    )
                })?,
            status: item
                .subscription_status
                .as_deref()
                .map(SubscriptionStatus::from),
            plan_reference: item.pricing_plan_reference_code.map(String::into_boxed_str),
            plan_name: item.pricing_plan_name.map(String::into_boxed_str),
            product_reference: item.product_reference_code.map(String::into_boxed_str),
            product_name: item.product_name.map(String::into_boxed_str),
            subscriber_reference: item.customer_reference_code.map(String::into_boxed_str),
            subscriber_email: item.customer_email.map(String::into_boxed_str),
            trial_days: item.trial_days,
            start_date: item
                .start_date
                .as_deref()
                .map(|date| wire::text(date).into()),
            end_date: item.end_date.as_deref().map(|date| wire::text(date).into()),
            raw: Raw::from_text(value.get()),
        })
    }
}

/// Where a subscription stands.
///
/// iyzico documents five values on `subscriptionStatus`. A sixth they start
/// sending is [`SubscriptionStatus::Other`] rather than an error: a status
/// this crate has not met is still a subscription somebody is paying for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscriptionStatus {
    /// Running, and being charged.
    Active,
    /// Created and waiting for [`Client::activate`].
    Pending,
    /// Stopped by [`Client::cancel`].
    Canceled,
    /// Stopped because a payment could not be taken.
    Unpaid,
    /// Ran to the end of what it was sold for.
    Expired,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl SubscriptionStatus {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "ACTIVE",
            Self::Pending => "PENDING",
            Self::Canceled => "CANCELED",
            Self::Unpaid => "UNPAID",
            Self::Expired => "EXPIRED",
            Self::Other(name) => name,
        }
    }
}

impl From<&str> for SubscriptionStatus {
    fn from(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "PENDING" => Self::Pending,
            "CANCELED" => Self::Canceled,
            "UNPAID" => Self::Unpaid,
            "EXPIRED" => Self::Expired,
            other => Self::Other(other.into()),
        }
    }
}

impl fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A subscriber, as iyzico holds them.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriberDetail {
    /// What iyzico calls this subscriber, and what [`Client::subscribe`] takes.
    pub reference_code: Box<str>,
    /// Given name.
    pub name: Option<Box<str>>,
    /// Family name.
    pub surname: Option<Box<str>>,
    /// Email address.
    pub email: Option<Box<str>>,
    /// Mobile number.
    pub gsm_number: Option<Box<str>>,
    /// iyzico's own answer for this subscriber, untouched. The addresses and
    /// the identity number are in here.
    pub raw: Raw,
}

impl SubscriberDetail {
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::SubscriberItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a subscriber was not the JSON iyzico documents",
            )
            .with_source(e)
        })?;
        Ok(Self {
            reference_code: item
                .reference_code
                .map(String::into_boxed_str)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Malformed,
                        PROVIDER,
                        "a subscriber carried no referenceCode",
                    )
                })?,
            name: item.name.map(String::into_boxed_str),
            surname: item.surname.map(String::into_boxed_str),
            email: item.email.map(String::into_boxed_str),
            gsm_number: item.gsm_number.map(String::into_boxed_str),
            raw: Raw::from_text(value.get()),
        })
    }
}

/// The subscriber, as every subscription request wants them.
fn subscriber_body(subscriber: &Subscriber) -> wire::SubscriberBody<'_> {
    wire::SubscriberBody {
        name: &subscriber.name,
        surname: &subscriber.surname,
        email: &subscriber.email,
        gsm_number: &subscriber.gsm_number,
        identity_number: &subscriber.identity_number,
        billing_address: address_body(&subscriber.billing_address),
        shipping_address: subscriber.shipping_address.as_ref().map(address_body),
    }
}

fn address_body(address: &subscriber::Address) -> wire::AddressBody<'_> {
    wire::AddressBody {
        contact_name: &address.contact_name,
        address: &address.address,
        city: &address.city,
        country: &address.country,
        zip_code: address.zip_code.as_deref(),
    }
}

/// One page of whatever was listed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Page<T> {
    /// What is on this page.
    pub items: Vec<T>,
    /// How many there are in total.
    pub total_count: Option<i64>,
    /// Which page this is.
    pub current_page: Option<i64>,
    /// How many pages there are.
    pub page_count: Option<i64>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// Whether a product or a plan is in use.
///
/// iyzico documents one value for both — `ACTIVE`, which is what a new one
/// gets — and never says what a product that has been deleted reads as, or
/// whether one can be turned off without being deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecordStatus {
    /// In use.
    Active,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl RecordStatus {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "ACTIVE",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for RecordStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for RecordStatus {
    fn from(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            other => Self::Other(other.into()),
        }
    }
}

/// Reads the answer to anything that returns one product or one plan.
fn one<T>(
    response: wire::Envelope,
    fallback: &str,
    read: impl Fn(&RawValue) -> Result<T, Error>,
) -> Result<T, Error> {
    if let Some(error) = classic::refused(
        response.status.as_deref(),
        response.error_message,
        response.error_code,
        fallback,
    ) {
        return Err(error);
    }
    let data = response
        .data
        .ok_or_else(|| Error::new(ErrorKind::Malformed, PROVIDER, "the answer carried no data"))?;
    read(&data)
}

/// Reads the answer to a listing.
fn read_page<T>(
    response: wire::ListEnvelope,
    raw: Raw,
    fallback: &str,
    read: impl Fn(&RawValue) -> Result<T, Error>,
) -> Result<Page<T>, Error> {
    if let Some(error) = classic::refused(
        response.status.as_deref(),
        response.error_message,
        response.error_code,
        fallback,
    ) {
        return Err(error);
    }
    let data = response.data.unwrap_or_default();
    let items = data.items.unwrap_or_default();
    let mut read_items = Vec::with_capacity(items.len());
    for item in &items {
        read_items.push(read(item)?);
    }
    Ok(Page {
        items: read_items,
        total_count: data.total_count.as_deref().and_then(wire::integer),
        current_page: data.current_page,
        page_count: data.page_count,
        raw,
    })
}

/// The query a listing carries.
fn paging(page: u32, count: u32) -> Result<String, Error> {
    if page == 0 || count == 0 {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "iyzico pages a subscription listing from page 1, in counts of at least 1",
        ));
    }
    Ok(format!("?locale={LOCALE}&page={page}&count={count}"))
}

/// `/v2/subscription/products/{reference}`, once the code is safe to put there.
fn product_path(reference: &str) -> Result<String, Error> {
    Ok(format!("{PRODUCTS}/{}", path_segment(reference)?))
}

/// `/v2/subscription/pricing-plans/{reference}`, likewise.
fn plan_path(reference: &str) -> Result<String, Error> {
    Ok(format!("{PLANS}/{}", path_segment(reference)?))
}

/// Refuses anything that would change the path rather than sit in it.
///
/// The path is what gets signed as well as what gets requested, so a reference
/// code carrying `/` or `?` would sign one endpoint and call another. Only the
/// unreserved characters of RFC 3986 are let through — iyzico's reference codes
/// are hyphenated hex, and anything else did not come from them.
fn path_segment(value: &str) -> Result<&str, Error> {
    let unreserved = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    if value.is_empty() || !value.chars().all(unreserved) {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "a subscription reference code goes into the signed request path, so it may \
             hold only letters, digits and -._~",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        PaymentInterval, PlanPaymentType, RecordStatus, paging, path_segment, plan_path,
        product_path,
    };

    #[test]
    fn a_reference_code_that_would_change_the_path_is_refused() {
        assert!(path_segment("d9e8f7a6-1234-4b2c-9d8e-0f1a2b3c4d5e").is_ok());
        assert!(path_segment("").is_err());
        // Each of these signs one path and calls another.
        for hostile in ["../products", "ref/../..", "ref?locale=en", "a b", "a#b"] {
            assert!(path_segment(hostile).is_err(), "{hostile} was let through");
        }
    }

    #[test]
    fn the_paths_are_the_ones_iyzico_documents() {
        assert_eq!(
            product_path("AbC123").expect("a plain code"),
            "/v2/subscription/products/AbC123"
        );
        assert_eq!(
            plan_path("AbC123").expect("a plain code"),
            "/v2/subscription/pricing-plans/AbC123"
        );
    }

    #[test]
    fn a_page_iyzico_would_not_be_asked_for_is_refused() {
        assert!(paging(1, 10).is_ok());
        // iyzico's own SDKs leave a zero out of the URL rather than send it.
        assert!(paging(0, 10).is_err());
        assert!(paging(1, 0).is_err());
    }

    #[test]
    fn the_words_iyzico_uses_round_trip_and_the_rest_are_kept() {
        for name in ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] {
            assert_eq!(PaymentInterval::from(name).to_string(), name);
        }
        assert_eq!(PlanPaymentType::from("RECURRING").to_string(), "RECURRING");
        assert_eq!(RecordStatus::from("ACTIVE"), RecordStatus::Active);
        assert_eq!(
            PaymentInterval::from("QUARTERLY"),
            PaymentInterval::Other("QUARTERLY".into())
        );
        assert_eq!(RecordStatus::from("PASSIVE").to_string(), "PASSIVE");
    }
}
