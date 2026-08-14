//! What a caller builds before asking [`crate::softpos::Client`] for anything.

use kasapay_core::Money;

/// A sale to start on the payer's phone.
///
/// Build one with [`InitSale::new`]. `PayPOS`'s own schema marks every field of
/// `InitSaleRequest` optional, `amount` included — this crate still requires
/// one, because a sale of nothing is not a sale kasapay will send.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InitSale {
    /// What to charge. See [`InitSale::new`] for why this is lira only.
    pub amount: Money,
    /// Whether `PayPOS`'s commission is added on top of `amount`.
    pub add_commission: Option<bool>,
    /// How many instalments. `PayPOS` documents no meaning for zero versus one.
    pub instalment: Option<i32>,
    /// The payer's phone number, for `PayPOS`'s own receipt.
    pub card_holder_phone: Option<Box<str>>,
    /// The payer's email, for `PayPOS`'s own receipt.
    pub card_holder_mail: Option<Box<str>>,
    /// Free text `PayPOS` shows the payer.
    pub description: Option<Box<str>>,
    /// The caller's own reference for this attempt.
    pub reference_no: Option<Box<str>>,
    /// Which of the dealer's agents this sale is attributed to.
    pub selected_agent_id: Option<Box<str>>,
    /// Where the mobile app's deeplink returns to once the payer is done.
    pub callback_url: Option<Box<str>>,
}

impl InitSale {
    /// Starts a sale in the amount to be charged.
    ///
    /// # Why this refuses anything but `Currency::Try`
    ///
    /// `InitSaleRequest.amount` is typed `number` with no `currency` field
    /// beside it anywhere in the schema, and `specs/README.md`'s per-product
    /// currency table names no enum for `softpos` at all — the group is
    /// absent from all three rows. Nothing documents another currency working
    /// here, `PayPOS` is a Turkish domestic acceptance product, and its
    /// contactless-PIN threshold is written in TRY on the product overview
    /// page — the closest thing to currency evidence either language offers.
    /// That is inference, not a documented enum, and is said plainly rather
    /// than folded into `Currency` silently: a caller who has evidence
    /// otherwise reads this and knows exactly what was assumed.
    #[must_use]
    pub const fn new(amount: Money) -> Self {
        Self {
            amount,
            add_commission: None,
            instalment: None,
            card_holder_phone: None,
            card_holder_mail: None,
            description: None,
            reference_no: None,
            selected_agent_id: None,
            callback_url: None,
        }
    }

    /// Adds `PayPOS`'s commission on top of [`InitSale::amount`].
    #[must_use]
    pub const fn add_commission(mut self, add: bool) -> Self {
        self.add_commission = Some(add);
        self
    }

    /// Sets how many instalments.
    #[must_use]
    pub const fn instalment(mut self, instalment: i32) -> Self {
        self.instalment = Some(instalment);
        self
    }

    /// Sets the payer's phone number.
    #[must_use]
    pub fn card_holder_phone(mut self, phone: impl Into<Box<str>>) -> Self {
        self.card_holder_phone = Some(phone.into());
        self
    }

    /// Sets the payer's email.
    #[must_use]
    pub fn card_holder_mail(mut self, mail: impl Into<Box<str>>) -> Self {
        self.card_holder_mail = Some(mail.into());
        self
    }

    /// Sets the free text `PayPOS` shows the payer.
    #[must_use]
    pub fn description(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the caller's own reference for this attempt.
    #[must_use]
    pub fn reference_no(mut self, reference_no: impl Into<Box<str>>) -> Self {
        self.reference_no = Some(reference_no.into());
        self
    }

    /// Sets which of the dealer's agents this sale is attributed to.
    #[must_use]
    pub fn selected_agent_id(mut self, selected_agent_id: impl Into<Box<str>>) -> Self {
        self.selected_agent_id = Some(selected_agent_id.into());
        self
    }

    /// Sets where the mobile app's deeplink returns to.
    #[must_use]
    pub fn callback_url(mut self, callback_url: impl Into<Box<str>>) -> Self {
        self.callback_url = Some(callback_url.into());
        self
    }
}

/// A cancel or refund to start on the payer's phone.
///
/// Build one with [`InitReversal::new`]. `xact_id` is the one field `PayPOS`
/// marks required; it is `check_transaction`'s own `xact_id` on the payment
/// being reversed, not the original `payment_session_id`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InitReversal {
    /// The encrypted transaction id being cancelled or refunded, from
    /// [`crate::softpos::Transaction::xact_id`].
    pub xact_id: Box<str>,
    /// The caller's own reference for this attempt.
    pub reference_no: Option<Box<str>>,
    /// Where the mobile app's deeplink returns to once the payer is done.
    ///
    /// `PayPOS` types this nullable as well as optional; both are read as
    /// "send nothing", the same as every other optional field here.
    pub callback_url: Option<Box<str>>,
}

impl InitReversal {
    /// Names the transaction to cancel or refund.
    #[must_use]
    pub fn new(xact_id: impl Into<Box<str>>) -> Self {
        Self {
            xact_id: xact_id.into(),
            reference_no: None,
            callback_url: None,
        }
    }

    /// Sets the caller's own reference for this attempt.
    #[must_use]
    pub fn reference_no(mut self, reference_no: impl Into<Box<str>>) -> Self {
        self.reference_no = Some(reference_no.into());
        self
    }

    /// Sets where the mobile app's deeplink returns to.
    #[must_use]
    pub fn callback_url(mut self, callback_url: impl Into<Box<str>>) -> Self {
        self.callback_url = Some(callback_url.into());
        self
    }
}
