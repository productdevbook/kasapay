//! The iyzilink client.

use std::fmt;

use kasapay_core::{Currency, Error, ErrorKind, Money, ProviderId, Raw};
use reqwest::Method;
use serde_json::value::RawValue;
use url::Url;

use crate::classic;
use crate::iyzilink::link::{Category, FastLink, LinkUpdate, NewLink};
use crate::iyzilink::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where the links live.
const PRODUCTS: &str = "/v2/iyzilink/products";
/// Where a one-off link is made.
const FAST_LINK: &str = "/v2/iyzilink/fast-link/products";
/// What language iyzico answers in.
///
/// Sent on every call rather than left out, because iyzico's two
/// documentation languages disagree about the default: the English pages say
/// `en` and the Turkish ones say `tr`, for the same three endpoints.
const LOCALE: &str = "tr";
/// The query every call that has no body carries.
const LOCALE_QUERY: &str = "?locale=tr";
/// The largest page iyzico documents.
const MAX_COUNT: u32 = 100;

/// Talks to iyzico's iyzilink API.
///
/// Built over a [`classic::Client`], because that is what iyzilink is: the
/// same host, the same [`IYZWSv2`](crate::Credentials) signing, the same
/// `status: "failure"` envelope. Cloning shares the one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    classic: classic::Client,
}

impl Client {
    /// Speaks iyzilink over a classic client.
    #[must_use]
    pub const fn new(classic: classic::Client) -> Self {
        Self { classic }
    }

    /// The classic client underneath, for everything that is not a link.
    #[must_use]
    pub const fn classic(&self) -> &classic::Client {
        &self.classic
    }

    /// Creates a link and hands back the URL to send people to.
    ///
    /// The picture is required: iyzico refuses a link without one.
    pub async fn create(&self, link: &NewLink) -> Result<Link, Error> {
        let body = wire::CreateRequest {
            locale: LOCALE,
            conversation_id: link.conversation_id.as_deref(),
            name: &link.name,
            description: &link.description,
            price: link.price.to_decimal_string(),
            currency_code: link.price.currency().code(),
            encoded_image_file: &link.image,
            address_ignorable: link.address_ignorable,
            installment_requested: link.instalments,
            stock_enabled: link.stock.map(|_| true),
            stock_count: link.stock,
            flexible_link: link.flexible_price.then_some(true),
            category_type: link.category.as_ref().map(Category::as_str),
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::SaveResponse>(Method::POST, PRODUCTS, "", Some(&body))
            .await?;
        into_link(response, raw, "iyzico refused to create the link")
    }

    /// Creates a one-off link for a small amount.
    ///
    /// A fast link takes one payment and then stops working, and the account
    /// needs an approved iyzico link before it can make one at all.
    pub async fn create_fast_link(&self, link: &FastLink) -> Result<Link, Error> {
        let body = wire::FastLinkRequest {
            locale: LOCALE,
            conversation_id: link.conversation_id.as_deref(),
            description: link.description.as_deref(),
            price: link.price.to_decimal_string(),
            currency_code: link.price.currency().code(),
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::SaveResponse>(Method::POST, FAST_LINK, "", Some(&body))
            .await?;
        into_link(response, raw, "iyzico refused to create the fast link")
    }

    /// Replaces a link's details, by the token it was created with.
    ///
    /// An update is a replacement rather than a patch: a field left out of
    /// [`LinkUpdate`] is not a field kept.
    pub async fn update(&self, token: &str, update: &LinkUpdate) -> Result<Link, Error> {
        let path = product_path(token)?;
        let body = wire::UpdateRequest {
            locale: LOCALE,
            conversation_id: update.conversation_id.as_deref(),
            name: &update.name,
            description: &update.description,
            price: update.price.to_decimal_string(),
            currency_code: update.price.currency().code(),
            encoded_image_file: update.image.as_deref(),
            address_ignorable: update.address_ignorable,
            installment_requested: update.instalments,
            stock_enabled: update.stock.map(|_| true),
            stock_count: update.stock,
            category_type: update.category.as_ref().map(Category::as_str),
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::SaveResponse>(Method::PUT, &path, "", Some(&body))
            .await?;
        into_link(response, raw, "iyzico refused to update the link")
    }

    /// Reads one link back.
    pub async fn get(&self, token: &str) -> Result<LinkDetails, Error> {
        let path = product_path(token)?;
        let (response, _) = self
            .classic
            .request::<(), wire::DetailResponse>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to read the link",
        ) {
            return Err(error);
        }
        let data = response.data.ok_or_else(|| {
            Error::new(ErrorKind::Malformed, PROVIDER, "the answer carried no data")
        })?;
        LinkDetails::read(&data)
    }

    /// Lists the links a merchant has, one page at a time.
    ///
    /// `page` counts from one and `count` is how many are on it, up to the
    /// hundred iyzico documents as the maximum. Both are checked here rather
    /// than sent: iyzico answers a page out of range with an error code and
    /// nothing a caller can act on.
    pub async fn list(&self, page: u32, count: u32) -> Result<LinkPage, Error> {
        if page < 1 || count < 1 || count > MAX_COUNT {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                format!("iyzico pages links from page 1 in counts of 1 to {MAX_COUNT}"),
            ));
        }
        let query = format!("?locale={LOCALE}&page={page}&count={count}");
        let (response, raw) = self
            .classic
            .request::<(), wire::ListResponse>(Method::GET, PRODUCTS, &query, None)
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the link listing",
        ) {
            return Err(error);
        }
        let data = response.data.unwrap_or_default();
        let items = data.items.unwrap_or_default();
        let mut links = Vec::with_capacity(items.len());
        for item in &items {
            links.push(LinkDetails::read(item)?);
        }
        Ok(LinkPage {
            links,
            total_count: data.total_count,
            current_page: data.current_page,
            page_count: data.page_count,
            listing_reviewed: data.listing_reviewed,
            raw,
        })
    }

    /// Turns a link on or off.
    ///
    /// A passive link is not a deleted one: the URL still resolves, and stops
    /// taking money. This is the reversible half of [`Client::delete`].
    pub async fn set_status(&self, token: &str, status: &LinkStatus) -> Result<(), Error> {
        let path = format!(
            "{}/status/{}",
            product_path(token)?,
            path_segment(status.as_str())?
        );
        let (response, _) = self
            .classic
            .request::<(), wire::Ack>(Method::PATCH, &path, LOCALE_QUERY, None)
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the status change",
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Deletes a link.
    ///
    /// Unlike the classic API's card delete, this one carries no body: the
    /// token is in the path.
    pub async fn delete(&self, token: &str) -> Result<(), Error> {
        let path = product_path(token)?;
        let (response, _) = self
            .classic
            .request::<(), wire::Ack>(Method::DELETE, &path, LOCALE_QUERY, None)
            .await?;
        match classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to delete the link",
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

/// A link as iyzico hands it back when it is made or changed.
#[derive(Debug, Clone)]
pub struct Link {
    /// What iyzico calls this link, and what every other call names it by.
    pub token: Box<str>,
    /// Where to send the payer. This is the thing to share.
    pub url: Option<Url>,
    /// Where the picture ended up.
    pub image_url: Option<Box<str>>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// Everything iyzico says about a link that already exists.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LinkDetails {
    /// What iyzico calls this link.
    pub token: Box<str>,
    /// The product name the payer sees.
    pub name: Option<Box<str>>,
    /// The product description the payer sees.
    pub description: Option<Box<str>>,
    /// The reference the link was created with.
    pub conversation_id: Option<Box<str>>,
    /// What it costs.
    ///
    /// `None` when iyzico priced it in a currency
    /// [`Currency`](kasapay_core::Currency) has no name for — `RUB`, `CHF` and
    /// `NOK` are iyzilink currencies and are not kasapay ones. The amount is
    /// still in [`LinkDetails::raw`].
    pub price: Option<Money>,
    /// Where to send the payer.
    pub url: Option<Url>,
    /// The product picture.
    pub image_url: Option<Box<str>>,
    /// An ordinary link or a one-off one.
    pub kind: Option<LinkKind>,
    /// Whether it is taking money.
    pub status: Option<LinkStatus>,
    /// iyzico's own id for the merchant.
    pub merchant_id: Option<i64>,
    /// Whether checkout skips asking the payer for an address.
    pub address_ignorable: Option<bool>,
    /// How many have been sold through it.
    pub sold_count: Option<i64>,
    /// Whether instalments are offered.
    pub instalments: Option<bool>,
    /// Whether stock is being counted.
    pub stock_enabled: Option<bool>,
    /// How many are left, when it is.
    pub stock_count: Option<i64>,
    /// Whether the payer chooses the amount.
    pub flexible_price: Option<bool>,
    /// The amounts a flexible link offers, as iyzico wrote them.
    ///
    /// Left as text rather than [`Money`]: iyzico documents this as an array of
    /// decimals in the answer and as a single integer in the request, so there
    /// is no one shape to read it into that both halves agree with.
    pub preset_prices: Vec<Box<str>>,
    /// What kind of thing is being sold.
    pub category: Option<Category>,
    /// iyzico's own answer for this link, untouched.
    pub raw: Raw,
}

impl LinkDetails {
    /// Reads one link out of the bytes iyzico sent for it.
    fn read(value: &RawValue) -> Result<Self, Error> {
        let product: wire::Product = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a link was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;

        let price = match (product.price.as_deref(), product.currency_code.as_deref()) {
            (Some(price), Some(code)) => match code.parse::<Currency>() {
                Ok(currency) => Some(
                    Money::parse(wire::decimal(price), currency)
                        .map_err(|e| Error::new(ErrorKind::Malformed, PROVIDER, e.to_string()))?,
                ),
                // RUB, CHF and NOK are links iyzico takes and Currency cannot
                // name. Refusing the whole listing over one of them would be
                // worse than answering None and leaving it in raw.
                Err(_) => None,
            },
            _ => None,
        };

        Ok(Self {
            token: product.token.unwrap_or_default().into_boxed_str(),
            name: product.name.map(String::into_boxed_str),
            description: product.description.map(String::into_boxed_str),
            conversation_id: product.conversation_id.map(String::into_boxed_str),
            price,
            url: product.url.as_deref().map(parse_url).transpose()?,
            image_url: product.image_url.map(String::into_boxed_str),
            kind: product.product_type.as_deref().map(LinkKind::from),
            status: product.product_status.as_deref().map(LinkStatus::from),
            merchant_id: product.merchant_id,
            address_ignorable: product.address_ignorable,
            sold_count: product.sold_count,
            instalments: product.installment_requested,
            stock_enabled: product.stock_enabled,
            stock_count: product.stock_count,
            flexible_price: product.flexible_link,
            preset_prices: product
                .preset_price_values
                .unwrap_or_default()
                .iter()
                .map(|value| wire::decimal(value).into())
                .collect(),
            category: product.category_type.as_deref().map(Category::from),
            raw: Raw::from_text(value.get()),
        })
    }
}

/// One page of a merchant's links.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LinkPage {
    /// The links on this page.
    pub links: Vec<LinkDetails>,
    /// How many there are in total.
    pub total_count: Option<i64>,
    /// Which page this is.
    pub current_page: Option<i64>,
    /// How many pages there are.
    pub page_count: Option<i64>,
    /// iyzico's own flag on the listing, which they describe only as "status
    /// information for the listing operation".
    pub listing_reviewed: Option<bool>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// Whether a link is taking money.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkStatus {
    /// Taking money.
    Active,
    /// Not taking money, and still there.
    Passive,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl LinkStatus {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "ACTIVE",
            Self::Passive => "PASSIVE",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for LinkStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for LinkStatus {
    fn from(value: &str) -> Self {
        match value {
            "ACTIVE" => Self::Active,
            "PASSIVE" => Self::Passive,
            other => Self::Other(other.into()),
        }
    }
}

/// An ordinary link or a one-off one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LinkKind {
    /// A link that goes on taking payments.
    IyziLink,
    /// A link that takes one payment and stops.
    FastLink,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl LinkKind {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::IyziLink => "IYZILINK",
            Self::FastLink => "FASTLINK",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for LinkKind {
    fn from(value: &str) -> Self {
        match value {
            "IYZILINK" => Self::IyziLink,
            "FASTLINK" => Self::FastLink,
            other => Self::Other(other.into()),
        }
    }
}

/// Reads the answer to a create or an update.
fn into_link(response: wire::SaveResponse, raw: Raw, fallback: &str) -> Result<Link, Error> {
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
    let token = data.token.ok_or_else(|| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "a saved link carried no token",
        )
    })?;
    Ok(Link {
        token: token.into_boxed_str(),
        url: data.url.as_deref().map(parse_url).transpose()?,
        image_url: data.image_url.map(String::into_boxed_str),
        raw,
    })
}

fn parse_url(value: &str) -> Result<Url, Error> {
    Url::parse(value).map_err(|e| {
        Error::new(ErrorKind::Malformed, PROVIDER, "a link URL is not a URL").with_source(e)
    })
}

/// `/v2/iyzilink/products/{token}`, once the token is safe to put there.
fn product_path(token: &str) -> Result<String, Error> {
    Ok(format!("{PRODUCTS}/{}", path_segment(token)?))
}

/// Refuses anything that would change the path rather than sit in it.
///
/// The path is what gets signed as well as what gets requested, so a token
/// carrying `/` or `?` would sign one endpoint and call another. Only the
/// unreserved characters of RFC 3986 are let through — iyzico's tokens are
/// alphanumeric, and anything else did not come from them.
fn path_segment(value: &str) -> Result<&str, Error> {
    let unreserved = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    if value.is_empty() || !value.chars().all(unreserved) {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "an iyzilink token goes into the signed request path, so it may hold \
             only letters, digits and -._~",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{LinkKind, LinkStatus, path_segment, product_path};

    #[test]
    fn a_token_that_would_change_the_path_is_refused() {
        assert!(path_segment("A1b2C3").is_ok());
        assert!(path_segment("").is_err());
        // Each of these signs one path and calls another.
        for hostile in ["../products", "tok/../..", "tok?locale=en", "tok en", "a#b"] {
            assert!(path_segment(hostile).is_err(), "{hostile} was let through");
        }
    }

    #[test]
    fn the_path_is_the_one_iyzico_documents() {
        assert_eq!(
            product_path("AbC123").expect("a plain token"),
            "/v2/iyzilink/products/AbC123"
        );
    }

    #[test]
    fn the_words_iyzico_uses_round_trip_and_the_rest_are_kept() {
        for name in ["ACTIVE", "PASSIVE"] {
            assert_eq!(LinkStatus::from(name).to_string(), name);
        }
        for name in ["IYZILINK", "FASTLINK"] {
            assert_eq!(LinkKind::from(name).to_string(), name);
        }
        assert_eq!(
            LinkStatus::from("EXPIRED"),
            LinkStatus::Other("EXPIRED".into())
        );
        assert_eq!(LinkKind::from("SUBLINK").to_string(), "SUBLINK");
    }
}
