//! The person a subscription is sold to, and how a subscription is started.
//!
//! The catalogue in [`catalogue`](crate::subscription::catalogue) says what is
//! on offer. This is the other half: who is buying it, and on what terms.
//!
//! # Nothing here holds a card number
//!
//! iyzico has two ways to start a subscription and only one of them is here.
//! `POST /v2/subscription/initialize` takes the card number, expiry and CVC on
//! the request, which puts the caller's server in PCI DSS scope — the same
//! reason the classic API's own non-3-D payment is not implemented, and the
//! same reason `POST /cardstorage/card` is not.
//!
//! What is here instead is the hosted form —
//! [`Client::start_subscription_form`](crate::subscription::Client::start_subscription_form)
//! — where iyzico collects the card on its own page, and
//! [`Client::subscribe`](crate::subscription::Client::subscribe), which
//! subscribes somebody iyzico already holds a card for.

use std::fmt;

/// Somewhere to bill or ship a subscription to.
///
/// The same four fields [`classic::checkout::Address`](crate::classic::checkout::Address)
/// carries, and a separate type because iyzico's subscription API documents
/// its own schema for them — one that requires a contact name where the
/// checkout form's does not.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Address {
    /// Who to address it to.
    pub contact_name: Box<str>,
    /// The street address.
    pub address: Box<str>,
    /// City.
    pub city: Box<str>,
    /// Country.
    pub country: Box<str>,
    /// Postcode.
    pub zip_code: Option<Box<str>>,
}

impl Address {
    /// An address with the four fields iyzico requires.
    #[must_use]
    pub fn new(
        contact_name: impl Into<Box<str>>,
        address: impl Into<Box<str>>,
        city: impl Into<Box<str>>,
        country: impl Into<Box<str>>,
    ) -> Self {
        Self {
            contact_name: contact_name.into(),
            address: address.into(),
            city: city.into(),
            country: country.into(),
            zip_code: None,
        }
    }

    /// Adds the postcode iyzico documents as optional.
    #[must_use]
    pub fn zip_code(mut self, zip_code: impl Into<Box<str>>) -> Self {
        self.zip_code = Some(zip_code.into());
        self
    }
}

/// Who a subscription is sold to.
///
/// iyzico calls this a customer and this crate calls it a subscriber, because
/// [`ChargeRequest::customer`](kasapay_core::ChargeRequest::customer) already
/// means something else in this workspace: the string a provider names a payer
/// by. This is the whole person — name, contact details, national identity
/// number and an address to bill.
///
/// Every field but the shipping address is required, and that is iyzico's
/// list rather than a choice made here: a subscription is a standing
/// authority to take money, and they ask for enough to know who gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Subscriber {
    /// Given name.
    pub name: Box<str>,
    /// Family name.
    pub surname: Box<str>,
    /// Email address.
    pub email: Box<str>,
    /// Mobile number. iyzico asks for E.164 — `+905555555555` — where possible.
    pub gsm_number: Box<str>,
    /// Turkish national identity number, or the equivalent.
    pub identity_number: Box<str>,
    /// Where to bill.
    pub billing_address: Address,
    /// Where to ship, when that is somewhere else.
    pub shipping_address: Option<Address>,
}

impl Subscriber {
    /// Starts building a subscriber.
    #[must_use]
    pub fn builder(
        name: impl Into<Box<str>>,
        surname: impl Into<Box<str>>,
        email: impl Into<Box<str>>,
        gsm_number: impl Into<Box<str>>,
        identity_number: impl Into<Box<str>>,
        billing_address: Address,
    ) -> SubscriberBuilder {
        SubscriberBuilder {
            subscriber: Self {
                name: name.into(),
                surname: surname.into(),
                email: email.into(),
                gsm_number: gsm_number.into(),
                identity_number: identity_number.into(),
                billing_address,
                shipping_address: None,
            },
        }
    }
}

/// Collects the parts of a [`Subscriber`].
#[derive(Debug, Clone)]
pub struct SubscriberBuilder {
    subscriber: Subscriber,
}

impl SubscriberBuilder {
    /// Ships somewhere other than the billing address.
    #[must_use]
    pub fn shipping_address(mut self, address: Address) -> Self {
        self.subscriber.shipping_address = Some(address);
        self
    }

    /// Produces the subscriber.
    ///
    /// No `Result`: every field iyzico requires is an argument to
    /// [`Subscriber::builder`], so there is nothing left to be missing.
    #[must_use]
    pub fn build(self) -> Subscriber {
        self.subscriber
    }
}

/// Whether a subscription starts running or waits to be switched on.
///
/// iyzico's `subscriptionInitialStatus`, and it is required on both ways of
/// starting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum InitialStatus {
    /// Starts immediately, and the first payment is taken. The default.
    #[default]
    Active,
    /// Starts nothing until
    /// [`Client::activate`](crate::subscription::Client::activate) is called.
    ///
    /// What a shop uses when the subscription depends on something outside
    /// iyzico — a contract signed, a delivery made.
    Pending,
}

impl InitialStatus {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "ACTIVE",
            Self::Pending => "PENDING",
        }
    }
}

impl fmt::Display for InitialStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What starting a subscription through the hosted form asks for.
///
/// The plan, the person, where iyzico should send them back to, and whether
/// the subscription starts running.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewSubscription {
    /// The plan being subscribed to — a `pricingPlanReferenceCode` from
    /// [`Client::create_plan`](crate::subscription::Client::create_plan).
    pub plan_reference: Box<str>,
    /// Who is subscribing.
    pub subscriber: Subscriber,
    /// Where iyzico sends the result when the payer finishes the form.
    pub callback_url: Box<str>,
    /// Whether the subscription starts running or waits.
    pub initial_status: InitialStatus,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Option<Box<str>>,
}

impl NewSubscription {
    /// Starts building a subscription.
    #[must_use]
    pub fn builder(
        plan_reference: impl Into<Box<str>>,
        subscriber: Subscriber,
        callback_url: impl Into<Box<str>>,
    ) -> NewSubscriptionBuilder {
        NewSubscriptionBuilder {
            subscription: Self {
                plan_reference: plan_reference.into(),
                subscriber,
                callback_url: callback_url.into(),
                initial_status: InitialStatus::Active,
                conversation_id: None,
            },
        }
    }
}

/// Collects the parts of a [`NewSubscription`].
#[derive(Debug, Clone)]
pub struct NewSubscriptionBuilder {
    subscription: NewSubscription,
}

impl NewSubscriptionBuilder {
    /// Starts the subscription pending rather than running.
    #[must_use]
    pub const fn initial_status(mut self, status: InitialStatus) -> Self {
        self.subscription.initial_status = status;
        self
    }

    /// Sets the caller's own reference for this request.
    #[must_use]
    pub fn conversation_id(mut self, id: impl Into<Box<str>>) -> Self {
        self.subscription.conversation_id = Some(id.into());
        self
    }

    /// Produces the request.
    #[must_use]
    pub fn build(self) -> NewSubscription {
        self.subscription
    }
}

/// What upgrading a subscription changes, and when.
///
/// iyzico's `upgrade` takes four fields and three of them are decisions with
/// money in them: whether the new plan starts now or at the end of the period
/// already paid for, whether its trial applies to somebody who has already
/// been a subscriber, and whether the count of recurrences starts again.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Upgrade {
    /// The plan to move to.
    pub plan_reference: Box<str>,
    /// When the change takes effect.
    pub period: UpgradePeriod,
    /// Whether the new plan's trial days are given to this subscriber.
    ///
    /// `false` is the safer default and the one this crate uses: a subscriber
    /// who has already paid is not on trial, and a trial given by mistake is
    /// free money out.
    pub use_trial: bool,
    /// Whether the count of payments taken starts again from zero.
    pub reset_recurrence_count: bool,
}

impl Upgrade {
    /// An upgrade to a plan, taking effect now, with no trial and no reset.
    #[must_use]
    pub fn to(plan_reference: impl Into<Box<str>>) -> Self {
        Self {
            plan_reference: plan_reference.into(),
            period: UpgradePeriod::Now,
            use_trial: false,
            reset_recurrence_count: false,
        }
    }

    /// Waits until the period already paid for runs out.
    #[must_use]
    pub const fn at_period_end(mut self) -> Self {
        self.period = UpgradePeriod::PeriodEnd;
        self
    }

    /// Gives the new plan's trial days to this subscriber.
    #[must_use]
    pub const fn with_trial(mut self) -> Self {
        self.use_trial = true;
        self
    }

    /// Starts the count of payments again from zero.
    #[must_use]
    pub const fn resetting_recurrence_count(mut self) -> Self {
        self.reset_recurrence_count = true;
        self
    }
}

/// When an upgrade takes effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum UpgradePeriod {
    /// Straight away, which is what iyzico calls `NOW`. The default.
    #[default]
    Now,
    /// When the period the subscriber has already paid for runs out.
    PeriodEnd,
}

impl UpgradePeriod {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Now => "NOW",
            Self::PeriodEnd => "PERIOD_END",
        }
    }
}

impl fmt::Display for UpgradePeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{Address, InitialStatus, Subscriber, Upgrade, UpgradePeriod};

    fn subscriber() -> Subscriber {
        Subscriber::builder(
            "Ayse",
            "Yilmaz",
            "ayse@example.test",
            "+905350000000",
            "11111111111",
            Address::new("Ayse Yilmaz", "Bagdat Cad. 1", "Istanbul", "Turkey"),
        )
        .build()
    }

    #[test]
    fn a_subscriber_ships_where_it_is_billed_unless_told_otherwise() {
        assert!(subscriber().shipping_address.is_none());
        let elsewhere = Subscriber::builder(
            "Ayse",
            "Yilmaz",
            "ayse@example.test",
            "+905350000000",
            "11111111111",
            Address::new("Ayse Yilmaz", "Bagdat Cad. 1", "Istanbul", "Turkey"),
        )
        .shipping_address(
            Address::new("Ayse Yilmaz", "Is Yeri", "Ankara", "Turkey").zip_code("06000"),
        )
        .build();
        let shipping = elsewhere.shipping_address.expect("a second address");
        assert_eq!(&*shipping.city, "Ankara");
        assert_eq!(shipping.zip_code.as_deref(), Some("06000"));
    }

    /// A trial given by mistake is free money out, so it is not the default.
    #[test]
    fn an_upgrade_takes_effect_now_with_no_trial_and_no_reset() {
        let plain = Upgrade::to("plan-2");
        assert_eq!(plain.period, UpgradePeriod::Now);
        assert!(!plain.use_trial);
        assert!(!plain.reset_recurrence_count);

        let later = Upgrade::to("plan-2")
            .at_period_end()
            .with_trial()
            .resetting_recurrence_count();
        assert_eq!(later.period, UpgradePeriod::PeriodEnd);
        assert_eq!(later.period.to_string(), "PERIOD_END");
        assert!(later.use_trial);
        assert!(later.reset_recurrence_count);
    }

    #[test]
    fn a_subscription_starts_running_unless_it_is_told_to_wait() {
        assert_eq!(InitialStatus::default(), InitialStatus::Active);
        assert_eq!(InitialStatus::Pending.to_string(), "PENDING");
    }
}
