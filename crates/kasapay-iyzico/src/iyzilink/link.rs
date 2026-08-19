//! What a link is made of, on the way out.
//!
//! iyzico's shape rather than kasapay's: a link is a little product page —
//! a name, a description, a picture and a price — and none of that is what
//! [`ChargeRequest`](kasapay_core::ChargeRequest) carries.

use std::fmt;

use kasapay_core::{Currency, Money, MoneyError};

/// A new link, ready to be created.
///
/// Build one with [`NewLink::builder`]. iyzico requires all four of the
/// builder's arguments, the picture included: a link with no image is refused.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewLink {
    /// The product name the payer sees.
    pub name: Box<str>,
    /// The product description the payer sees.
    pub description: Box<str>,
    /// What it costs.
    pub price: Money,
    /// The product picture, base64-encoded.
    pub image: Box<str>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
    /// Whether checkout skips asking the payer for an address.
    pub address_ignorable: Option<bool>,
    /// Whether instalments are offered.
    pub instalments: Option<bool>,
    /// How many there are to sell, when stock is being counted.
    pub stock: Option<u32>,
    /// What kind of thing is being sold.
    pub category: Option<Category>,
    /// Whether the payer chooses the amount.
    ///
    /// iyzico only accepts this on an account the feature is enabled for.
    pub flexible_price: bool,
}

impl NewLink {
    /// Starts building a link.
    #[must_use]
    pub fn builder(
        name: impl Into<Box<str>>,
        description: impl Into<Box<str>>,
        price: Money,
        image: impl Into<Box<str>>,
    ) -> NewLinkBuilder {
        NewLinkBuilder {
            name: name.into(),
            description: description.into(),
            price,
            image: image.into(),
            conversation_id: None,
            address_ignorable: None,
            instalments: None,
            stock: None,
            category: None,
            flexible_price: false,
        }
    }
}

/// Collects the parts of a [`NewLink`] before it is checked.
#[derive(Debug, Clone)]
pub struct NewLinkBuilder {
    name: Box<str>,
    description: Box<str>,
    price: Money,
    image: Box<str>,
    conversation_id: Option<Box<str>>,
    address_ignorable: Option<bool>,
    instalments: Option<bool>,
    stock: Option<u32>,
    category: Option<Category>,
    flexible_price: bool,
}

impl NewLinkBuilder {
    /// Sets the caller's own reference for this link.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Says whether checkout asks the payer for an address.
    #[must_use]
    pub const fn address_ignorable(mut self, ignorable: bool) -> Self {
        self.address_ignorable = Some(ignorable);
        self
    }

    /// Says whether instalments are offered.
    #[must_use]
    pub const fn instalments(mut self, offered: bool) -> Self {
        self.instalments = Some(offered);
        self
    }

    /// Counts stock, starting at `count`.
    ///
    /// Sends `stockEnabled` as well: iyzico ignores a count without it.
    #[must_use]
    pub const fn stock(mut self, count: u32) -> Self {
        self.stock = Some(count);
        self
    }

    /// Says what kind of thing is being sold.
    #[must_use]
    pub fn category(mut self, category: Category) -> Self {
        self.category = Some(category);
        self
    }

    /// Lets the payer choose the amount, rather than paying `price`.
    ///
    /// Only on an account iyzico has enabled the feature for; anywhere else it
    /// is refused. The three preset amounts a flexible link can offer are not
    /// sent — see the [module documentation](crate::iyzilink) for why.
    #[must_use]
    pub const fn flexible_price(mut self) -> Self {
        self.flexible_price = true;
        self
    }

    /// Checks the link and produces it.
    pub fn build(self) -> Result<NewLink, LinkError> {
        self.price.require_positive()?;
        iyzilink_currency(self.price.currency())?;
        Ok(NewLink {
            name: self.name,
            description: self.description,
            price: self.price,
            image: self.image,
            conversation_id: self.conversation_id,
            address_ignorable: self.address_ignorable,
            instalments: self.instalments,
            stock: self.stock,
            category: self.category,
            flexible_price: self.flexible_price,
        })
    }
}

/// Everything an update sends, for a link that already exists.
///
/// Build one with [`LinkUpdate::builder`]. The name, the description and the
/// price are required even when they are not what changed — iyzico's update
/// replaces the link rather than patching it, so anything left out is lost.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LinkUpdate {
    /// The product name the payer sees.
    pub name: Box<str>,
    /// The product description the payer sees.
    pub description: Box<str>,
    /// What it costs.
    pub price: Money,
    /// A new picture, base64-encoded, or the one already there if `None`.
    pub image: Option<Box<str>>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
    /// Whether checkout skips asking the payer for an address.
    pub address_ignorable: Option<bool>,
    /// Whether instalments are offered.
    pub instalments: Option<bool>,
    /// How many there are to sell, when stock is being counted.
    pub stock: Option<u32>,
    /// What kind of thing is being sold.
    pub category: Option<Category>,
}

impl LinkUpdate {
    /// Starts building an update.
    #[must_use]
    pub fn builder(
        name: impl Into<Box<str>>,
        description: impl Into<Box<str>>,
        price: Money,
    ) -> LinkUpdateBuilder {
        LinkUpdateBuilder {
            name: name.into(),
            description: description.into(),
            price,
            image: None,
            conversation_id: None,
            address_ignorable: None,
            instalments: None,
            stock: None,
            category: None,
        }
    }
}

/// Collects the parts of a [`LinkUpdate`] before it is checked.
#[derive(Debug, Clone)]
pub struct LinkUpdateBuilder {
    name: Box<str>,
    description: Box<str>,
    price: Money,
    image: Option<Box<str>>,
    conversation_id: Option<Box<str>>,
    address_ignorable: Option<bool>,
    instalments: Option<bool>,
    stock: Option<u32>,
    category: Option<Category>,
}

impl LinkUpdateBuilder {
    /// Replaces the picture, base64-encoded.
    #[must_use]
    pub fn image(mut self, image: impl Into<Box<str>>) -> Self {
        self.image = Some(image.into());
        self
    }

    /// Sets the caller's own reference for this update.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Says whether checkout asks the payer for an address.
    #[must_use]
    pub const fn address_ignorable(mut self, ignorable: bool) -> Self {
        self.address_ignorable = Some(ignorable);
        self
    }

    /// Says whether instalments are offered.
    #[must_use]
    pub const fn instalments(mut self, offered: bool) -> Self {
        self.instalments = Some(offered);
        self
    }

    /// Counts stock, starting at `count`.
    #[must_use]
    pub const fn stock(mut self, count: u32) -> Self {
        self.stock = Some(count);
        self
    }

    /// Says what kind of thing is being sold.
    #[must_use]
    pub fn category(mut self, category: Category) -> Self {
        self.category = Some(category);
        self
    }

    /// Checks the update and produces it.
    pub fn build(self) -> Result<LinkUpdate, LinkError> {
        self.price.require_positive()?;
        iyzilink_currency(self.price.currency())?;
        Ok(LinkUpdate {
            name: self.name,
            description: self.description,
            price: self.price,
            image: self.image,
            conversation_id: self.conversation_id,
            address_ignorable: self.address_ignorable,
            instalments: self.instalments,
            stock: self.stock,
            category: self.category,
        })
    }
}

/// A one-off link for a small amount.
///
/// Build one with [`FastLink::builder`]. A fast link takes one payment and
/// then stops working, needs an approved iyzico link on the account before it
/// can be created at all, and iyzico documents it as being for payments of
/// **750 TRY and under**. That ceiling is not checked here — it is theirs to
/// move, and a check written against today's number would start refusing
/// payments they accept.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FastLink {
    /// What it costs.
    pub price: Money,
    /// What the payer is told they are paying for.
    pub description: Option<Box<str>>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl FastLink {
    /// Starts building a fast link.
    #[must_use]
    pub const fn builder(price: Money) -> FastLinkBuilder {
        FastLinkBuilder {
            price,
            description: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`FastLink`] before it is checked.
#[derive(Debug, Clone)]
pub struct FastLinkBuilder {
    price: Money,
    description: Option<Box<str>>,
    conversation_id: Option<Box<str>>,
}

impl FastLinkBuilder {
    /// Says what the payer is paying for.
    #[must_use]
    pub fn describe(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the caller's own reference for this link.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the link and produces it.
    pub fn build(self) -> Result<FastLink, LinkError> {
        self.price.require_positive()?;
        iyzilink_currency(self.price.currency())?;
        Ok(FastLink {
            price: self.price,
            description: self.description,
            conversation_id: self.conversation_id,
        })
    }
}

/// The currencies iyzico takes a link in.
///
/// iyzico documents seven — `TRY`, `USD`, `EUR`, `GBP`, `RUB`, `CHF`, `NOK` —
/// and `Currency` names all seven since #10. Yen and dinar are refused here
/// rather than sent.
fn iyzilink_currency(currency: Currency) -> Result<(), LinkError> {
    match currency {
        Currency::Try
        | Currency::Usd
        | Currency::Eur
        | Currency::Gbp
        | Currency::Rub
        | Currency::Chf
        | Currency::Nok => Ok(()),
        _ => Err(LinkError::UnsupportedCurrency(currency)),
    }
}

/// A link was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LinkError {
    /// The price was not an amount iyzico will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
    /// iyzico does not document links in this currency.
    #[error("iyzico does not document an iyzico Link priced in {0}")]
    UnsupportedCurrency(Currency),
}

/// What kind of thing a link sells.
///
/// iyzico's own list, and it is short: their categories are a fraud-scoring
/// input rather than a shop's taxonomy. `Unknown` is what a link gets when
/// none is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Category {
    /// Gold, and anything else priced by the gram.
    Gold,
    /// Food.
    Food,
    /// A phone.
    Phone,
    /// Anything iyzico has no category for. Their default.
    Unknown,
    /// A computer.
    Pc,
    /// A tablet.
    Tablet,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl Category {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Gold => "GOLD",
            Self::Food => "FOOD",
            Self::Phone => "PHONE",
            Self::Unknown => "UNKNOWN",
            Self::Pc => "PC",
            Self::Tablet => "TABLET",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Category {
    fn from(value: &str) -> Self {
        match value {
            "GOLD" => Self::Gold,
            "FOOD" => Self::Food,
            "PHONE" => Self::Phone,
            "UNKNOWN" => Self::Unknown,
            "PC" => Self::Pc,
            "TABLET" => Self::Tablet,
            other => Self::Other(other.into()),
        }
    }
}
