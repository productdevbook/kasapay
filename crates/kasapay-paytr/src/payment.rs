//! What PayTR needs to open a payment.
//!
//! All of it required: an email, the payer's own IP address, a name, an
//! address, a phone number, two return URLs and an itemised basket.
//!
//! Most callers do not build one.
//! [`Provider::charge`](kasapay_core::Provider::charge) assembles it from a
//! [`ChargeRequest`](kasapay_core::ChargeRequest) carrying a
//! [`Buyer`](kasapay_core::Buyer) and a basket, which is the portable way in.
//! This is what to build by hand for the two things the shared request has no
//! word for: refusing instalments altogether, and capping how many are
//! offered.

use kasapay_core::{Money, MoneyError, OrderRef};
use url::Url;

/// One line of the basket PayTR shows the payer.
#[derive(Debug, Clone)]
pub struct BasketItem {
    /// What it is called.
    pub name: Box<str>,
    /// What it costs.
    pub price: Money,
    /// How many.
    pub quantity: u32,
}

/// A payment to open.
///
/// Build one with [`Payment::builder`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payment {
    /// The merchant's own order reference. PayTR has no id of its own, so this
    /// is what reads the payment back later.
    pub order: OrderRef,
    /// What to charge.
    pub amount: Money,
    /// The payer's email.
    pub email: Box<str>,
    /// The address the payer's request came from. PayTR refuses a token
    /// without it, and refuses a private one.
    pub payer_ip: Box<str>,
    /// The payer's name.
    pub name: Box<str>,
    /// The payer's address.
    pub address: Box<str>,
    /// The payer's phone number.
    pub phone: Box<str>,
    /// Where to send them when it works.
    pub success_url: Url,
    /// Where to send them when it does not.
    pub failure_url: Url,
    /// What they are buying.
    pub basket: Vec<BasketItem>,
    /// The largest instalment count to offer; zero for whatever the merchant
    /// is allowed.
    pub max_instalments: u8,
    /// Whether to offer instalments at all.
    pub instalments: bool,
}

impl Payment {
    /// Starts building a payment.
    #[must_use]
    pub fn builder(order: OrderRef, amount: Money, payer: Payer) -> PaymentBuilder {
        PaymentBuilder {
            order,
            amount,
            payer,
            basket: Vec::new(),
            max_instalments: 0,
            instalments: true,
        }
    }
}

/// Who is paying, and where to send them afterwards.
#[derive(Debug, Clone)]
pub struct Payer {
    /// Email.
    pub email: Box<str>,
    /// The address their request came from.
    pub ip: Box<str>,
    /// Name.
    pub name: Box<str>,
    /// Address.
    pub address: Box<str>,
    /// Phone.
    pub phone: Box<str>,
    /// Where to send them when it works.
    pub success_url: Url,
    /// Where to send them when it does not.
    pub failure_url: Url,
}

/// Collects the parts of a [`Payment`] before it is checked.
#[derive(Debug, Clone)]
pub struct PaymentBuilder {
    order: OrderRef,
    amount: Money,
    payer: Payer,
    basket: Vec<BasketItem>,
    max_instalments: u8,
    instalments: bool,
}

impl PaymentBuilder {
    /// Adds one line to the basket.
    #[must_use]
    pub fn item(mut self, item: BasketItem) -> Self {
        self.basket.push(item);
        self
    }

    /// Offers no instalments at all — required for some goods.
    #[must_use]
    pub const fn without_instalments(mut self) -> Self {
        self.instalments = false;
        self
    }

    /// Offers no more than this many instalments.
    #[must_use]
    pub const fn max_instalments(mut self, count: u8) -> Self {
        self.max_instalments = count;
        self
    }

    /// Checks the payment and produces it.
    pub fn build(self) -> Result<Payment, PaymentError> {
        self.amount.require_positive()?;
        if self.basket.is_empty() {
            return Err(PaymentError::EmptyBasket);
        }
        if self.payer.email.is_empty() {
            return Err(PaymentError::NoEmail);
        }
        if self.payer.ip.is_empty() {
            return Err(PaymentError::NoPayerIp);
        }
        Ok(Payment {
            order: self.order,
            amount: self.amount,
            email: self.payer.email,
            payer_ip: self.payer.ip,
            name: self.payer.name,
            address: self.payer.address,
            phone: self.payer.phone,
            success_url: self.payer.success_url,
            failure_url: self.payer.failure_url,
            basket: self.basket,
            max_instalments: self.max_instalments,
            instalments: self.instalments,
        })
    }
}

/// A [`Payment`] was built out of parts PayTR will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PaymentError {
    /// The basket had no lines.
    #[error("the basket is empty")]
    EmptyBasket,
    /// No email, which PayTR requires.
    #[error("PayTR requires the payer's email")]
    NoEmail,
    /// No payer IP, which PayTR requires and refuses a token without.
    #[error("PayTR requires the address the payer's request came from")]
    NoPayerIp,
    /// The amount was not one that can be charged.
    #[error(transparent)]
    Amount(#[from] MoneyError),
}
