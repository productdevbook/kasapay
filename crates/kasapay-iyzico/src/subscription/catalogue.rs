//! What a product and a pricing plan are made of, on the way out.
//!
//! A subscription is sold out of a catalogue: a product is the thing, a plan
//! is what it costs and how often. Neither is a payment, so none of this is
//! [`ChargeRequest`](kasapay_core::ChargeRequest)'s shape.

use std::fmt;

use kasapay_core::{Currency, Money, MoneyError};

/// A new product, ready to be created.
///
/// Build one with [`NewProduct::builder`]. iyzico requires a name and nothing
/// else, and the name is unique across a merchant's products.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewProduct {
    /// What the product is called. Unique across the merchant's products.
    pub name: Box<str>,
    /// What it is, for whoever reads the merchant panel.
    pub description: Option<Box<str>>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl NewProduct {
    /// Starts building a product.
    #[must_use]
    pub fn builder(name: impl Into<Box<str>>) -> NewProductBuilder {
        NewProductBuilder {
            name: name.into(),
            description: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`NewProduct`].
#[derive(Debug, Clone)]
pub struct NewProductBuilder {
    name: Box<str>,
    description: Option<Box<str>>,
    conversation_id: Option<Box<str>>,
}

impl NewProductBuilder {
    /// Says what the product is.
    #[must_use]
    pub fn describe(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the caller's own reference for this request.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Produces the product.
    ///
    /// No `Result`: iyzico documents nothing about a product that can be got
    /// wrong before it is sent. [`NewPlan::builder`] does, and its `build`
    /// answers one.
    #[must_use]
    pub fn build(self) -> NewProduct {
        NewProduct {
            name: self.name,
            description: self.description,
            conversation_id: self.conversation_id,
        }
    }
}

/// Everything an update sends, for a product that already exists.
///
/// Build one with [`ProductUpdate::builder`]. The name is required even when
/// it is not what changed: iyzico's update replaces the product's name and
/// description rather than patching them, so a description left out is a
/// description lost.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProductUpdate {
    /// What the product is called from now on.
    pub name: Box<str>,
    /// What it is, from now on.
    pub description: Option<Box<str>>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl ProductUpdate {
    /// Starts building an update.
    #[must_use]
    pub fn builder(name: impl Into<Box<str>>) -> ProductUpdateBuilder {
        ProductUpdateBuilder {
            name: name.into(),
            description: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`ProductUpdate`].
#[derive(Debug, Clone)]
pub struct ProductUpdateBuilder {
    name: Box<str>,
    description: Option<Box<str>>,
    conversation_id: Option<Box<str>>,
}

impl ProductUpdateBuilder {
    /// Says what the product is from now on.
    #[must_use]
    pub fn describe(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the caller's own reference for this request.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Produces the update.
    #[must_use]
    pub fn build(self) -> ProductUpdate {
        ProductUpdate {
            name: self.name,
            description: self.description,
            conversation_id: self.conversation_id,
        }
    }
}

/// A new pricing plan, ready to be created against a product.
///
/// Build one with [`NewPlan::builder`]. A plan says what a subscriber pays and
/// how often; the product it hangs off is named in the call rather than here.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewPlan {
    /// What the plan is called. Unique across the merchant's plans.
    pub name: Box<str>,
    /// What is charged each period.
    pub price: Money,
    /// How often that happens.
    pub interval: PaymentInterval,
    /// How many intervals apart the charges are, when it is not every one.
    pub interval_count: Option<u32>,
    /// How many charges there are in total, when the plan is not open-ended.
    pub recurrences: Option<u32>,
    /// A free trial, in days, before the first charge.
    pub trial_days: Option<u32>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl NewPlan {
    /// Starts building a plan.
    ///
    /// `interval` is how often the charge happens; the price is what is taken
    /// each time.
    #[must_use]
    pub fn builder(
        name: impl Into<Box<str>>,
        price: Money,
        interval: PaymentInterval,
    ) -> NewPlanBuilder {
        NewPlanBuilder {
            name: name.into(),
            price,
            interval,
            interval_count: None,
            recurrences: None,
            trial_days: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`NewPlan`] before it is checked.
#[derive(Debug, Clone)]
pub struct NewPlanBuilder {
    name: Box<str>,
    price: Money,
    interval: PaymentInterval,
    interval_count: Option<u32>,
    recurrences: Option<u32>,
    trial_days: Option<u32>,
    conversation_id: Option<Box<str>>,
}

impl NewPlanBuilder {
    /// Charges every `count` intervals rather than every one.
    ///
    /// `2` on a weekly plan is a charge every two weeks.
    #[must_use]
    pub const fn interval_count(mut self, count: u32) -> Self {
        self.interval_count = Some(count);
        self
    }

    /// Stops after `count` charges.
    ///
    /// Without it the plan goes on until the subscription is cancelled.
    #[must_use]
    pub const fn recurrences(mut self, count: u32) -> Self {
        self.recurrences = Some(count);
        self
    }

    /// Gives the subscriber `days` free before the first charge.
    ///
    /// The card is taken at the start of the trial and charged at the end of
    /// it; nothing is taken in between.
    #[must_use]
    pub const fn trial_days(mut self, days: u32) -> Self {
        self.trial_days = Some(days);
        self
    }

    /// Sets the caller's own reference for this request.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Checks the plan and produces it.
    pub fn build(self) -> Result<NewPlan, PlanError> {
        self.price.require_positive()?;
        plan_currency(self.price.currency())?;
        Ok(NewPlan {
            name: self.name,
            price: self.price,
            interval: self.interval,
            interval_count: self.interval_count,
            recurrences: self.recurrences,
            trial_days: self.trial_days,
            conversation_id: self.conversation_id,
        })
    }
}

/// Everything an update sends, for a plan that already exists.
///
/// Build one with [`PlanUpdate::builder`]. It carries a name and a trial
/// period and nothing else, because that is all iyzico's update takes: a plan's
/// price, currency and interval cannot be changed once subscribers are on it.
/// Charging something else means a new plan and an upgrade onto it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PlanUpdate {
    /// What the plan is called from now on.
    pub name: Box<str>,
    /// The trial period from now on, in days.
    pub trial_days: Option<u32>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl PlanUpdate {
    /// Starts building an update.
    #[must_use]
    pub fn builder(name: impl Into<Box<str>>) -> PlanUpdateBuilder {
        PlanUpdateBuilder {
            name: name.into(),
            trial_days: None,
            conversation_id: None,
        }
    }
}

/// Collects the parts of a [`PlanUpdate`].
#[derive(Debug, Clone)]
pub struct PlanUpdateBuilder {
    name: Box<str>,
    trial_days: Option<u32>,
    conversation_id: Option<Box<str>>,
}

impl PlanUpdateBuilder {
    /// Changes the trial period to `days`.
    ///
    /// iyzico says a plan's subscribers are not affected by an update, so this
    /// is what the next subscriber gets rather than what the current ones do.
    #[must_use]
    pub const fn trial_days(mut self, days: u32) -> Self {
        self.trial_days = Some(days);
        self
    }

    /// Sets the caller's own reference for this request.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Produces the update.
    #[must_use]
    pub fn build(self) -> PlanUpdate {
        PlanUpdate {
            name: self.name,
            trial_days: self.trial_days,
            conversation_id: self.conversation_id,
        }
    }
}

/// How often a plan charges.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaymentInterval {
    /// Every day.
    Daily,
    /// Every week.
    Weekly,
    /// Every month.
    Monthly,
    /// Every year.
    Yearly,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl PaymentInterval {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Yearly => "YEARLY",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for PaymentInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PaymentInterval {
    fn from(value: &str) -> Self {
        match value {
            "DAILY" => Self::Daily,
            "WEEKLY" => Self::Weekly,
            "MONTHLY" => Self::Monthly,
            "YEARLY" => Self::Yearly,
            other => Self::Other(other.into()),
        }
    }
}

/// What kind of plan this is.
///
/// iyzico documents one value and says so in as many words: *"the
/// `planPaymentType` parameter is mandatory and can currently only take the
/// value RECURRING"*. It is not on [`NewPlan`] for that reason — there is
/// nothing to choose — and exists here to read back what a plan says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlanPaymentType {
    /// Charges again every period until it stops.
    Recurring,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl PlanPaymentType {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Recurring => "RECURRING",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for PlanPaymentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PlanPaymentType {
    fn from(value: &str) -> Self {
        match value {
            "RECURRING" => Self::Recurring,
            other => Self::Other(other.into()),
        }
    }
}

/// The currencies iyzico documents a subscription plan in.
///
/// Three — `TRY`, `USD` and `EUR` — and no more, in both documentation
/// languages, which is narrower than the rest of iyzico takes. The others are
/// refused here rather than sent: a plan priced in sterling cannot be built in
/// the first place.
fn plan_currency(currency: Currency) -> Result<(), PlanError> {
    match currency {
        Currency::Try | Currency::Usd | Currency::Eur => Ok(()),
        Currency::Gbp | Currency::Jpy | Currency::Kwd => {
            Err(PlanError::UnsupportedCurrency(currency))
        }
    }
}

/// A plan was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlanError {
    /// The price was not an amount iyzico will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
    /// iyzico does not document a subscription plan in this currency.
    #[error("iyzico documents no subscription plan priced in {0}")]
    UnsupportedCurrency(Currency),
}
