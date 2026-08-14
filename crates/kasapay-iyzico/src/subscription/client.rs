//! The subscription client.

use std::fmt;

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Raw};
use reqwest::Method;
use serde_json::value::RawValue;

use crate::classic;
use crate::subscription::catalogue::{
    NewPlan, NewProduct, PaymentInterval, PlanPaymentType, PlanUpdate, ProductUpdate,
};
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
