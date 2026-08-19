//! What a cash register asks the terminal to do, on the way out.
//!
//! Every Terminal Host request names three things: the conversation, so the
//! answer can be matched to the question; the device, because a merchant may
//! have many; and the transaction, which is the caller's own reference for
//! this attempt. [`Reference`] carries all three, and every request here is
//! built from one.

use std::fmt;

use kasapay_core::{Currency, Money, MoneyError};

/// What language iyzico answers in, and what a cashier reads.
///
/// The Terminal API's error messages are shown at the till, so this is a
/// choice worth making rather than a default worth inheriting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Locale {
    /// Turkish. The default, as this is a Turkish fiscal product.
    #[default]
    Turkish,
    /// English.
    English,
}

impl Locale {
    /// The word iyzico expects on the wire.
    ///
    /// Lowercase, which is what the OpenAPI fragment's `enum` says, although
    /// iyzico's own sample request sends `"TR"`. See the module docs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Turkish => "tr",
            Self::English => "en",
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What every Terminal Host request names itself, its terminal and its attempt by.
///
/// None of the three is optional. iyzico marks `conversationId` required on
/// all four operations and this crate has no way to invent one — there is no
/// UUID generator in this dependency tree, and a conversation id a caller did
/// not choose is one they cannot match an answer to later.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(
    clippy::struct_field_names,
    reason = "the three are iyzico's own field names and all three end in Id; \
              renaming them to drop a suffix would hide which field is being sent"
)]
pub struct Reference {
    /// The caller's id for this request, echoed back on the answer.
    pub conversation_id: Box<str>,
    /// The terminal the transaction happens on. iyzico's `deviceUniqueId`.
    pub device_unique_id: Box<str>,
    /// The caller's own unique reference for this attempt.
    ///
    /// A sale, a refund of that sale and a void of it each carry their own:
    /// iyzico documents this as "a unique reference number generated for the
    /// void transaction", not the sale's.
    pub transaction_reference_id: Box<str>,
}

impl Reference {
    /// Names a request, a terminal and an attempt.
    #[must_use]
    pub fn new(
        conversation_id: impl Into<Box<str>>,
        device_unique_id: impl Into<Box<str>>,
        transaction_reference_id: impl Into<Box<str>>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            device_unique_id: device_unique_id.into(),
            transaction_reference_id: transaction_reference_id.into(),
        }
    }
}

/// What kind of sale is being started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SalesType {
    /// Take the money now.
    #[default]
    Sale,
    /// Hold the money without taking it.
    PreAuth,
    /// Take money a [`SalesType::PreAuth`] is holding.
    ///
    /// The only kind that needs [`SaleBuilder::payment_id`], and
    /// [`SaleBuilder::build`] refuses one without it.
    PostAuth,
}

impl SalesType {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sale => "SALE",
            Self::PreAuth => "PRE_AUTH",
            Self::PostAuth => "POST_AUTH",
        }
    }
}

impl fmt::Display for SalesType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A sale to start on the terminal.
///
/// Build one with [`Sale::builder`]. Sending it puts the terminal into its
/// card-reading state; the payer completes the payment there, and the answer
/// comes back on the same call rather than through a callback.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sale {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// What to charge.
    pub amount: Money,
    /// Which kind of sale.
    pub sales_type: SalesType,
    /// How many instalments. Zero and one both mean a single payment.
    pub installments: u8,
    /// The payment a [`SalesType::PostAuth`] is closing.
    pub payment_id: Option<Box<str>>,
}

impl Sale {
    /// Starts building a sale, in the amount to be charged.
    #[must_use]
    pub fn builder(reference: Reference, amount: Money) -> SaleBuilder {
        SaleBuilder {
            reference,
            amount,
            sales_type: SalesType::Sale,
            installments: 0,
            payment_id: None,
        }
    }
}

/// Collects the parts of a [`Sale`] before they are checked.
#[derive(Debug, Clone)]
pub struct SaleBuilder {
    reference: Reference,
    amount: Money,
    sales_type: SalesType,
    installments: u8,
    payment_id: Option<Box<str>>,
}

impl SaleBuilder {
    /// Sets what kind of sale this is. [`SalesType::Sale`] without this.
    #[must_use]
    pub const fn sales_type(mut self, sales_type: SalesType) -> Self {
        self.sales_type = sales_type;
        self
    }

    /// Splits the amount over `count` instalments.
    ///
    /// iyzico takes 0 to 12 and nothing else. Both 0 and 1 are a single
    /// payment: the response says so — "returns 0 or 1 for single-payment
    /// transactions" — and iyzico's own sample sends 0.
    #[must_use]
    pub const fn installments(mut self, count: u8) -> Self {
        self.installments = count;
        self
    }

    /// Names the payment a provision-closing sale is closing.
    ///
    /// Required for [`SalesType::PostAuth`] and meaningless without it.
    #[must_use]
    pub fn payment_id(mut self, payment_id: impl Into<Box<str>>) -> Self {
        self.payment_id = Some(payment_id.into());
        self
    }

    /// Checks the sale and produces it.
    pub fn build(self) -> Result<Sale, RequestError> {
        self.amount.require_positive()?;
        terminal_currency(self.amount.currency())?;
        if self.installments > 12 {
            return Err(RequestError::Installments(self.installments));
        }
        if self.sales_type == SalesType::PostAuth && self.payment_id.is_none() {
            return Err(RequestError::PostAuthWithoutPayment);
        }
        Ok(Sale {
            reference: self.reference,
            amount: self.amount,
            sales_type: self.sales_type,
            installments: self.installments,
            payment_id: self.payment_id,
        })
    }
}

/// A refund of a payment, in whole or in part.
///
/// Build one with [`Refund::builder`]. Like a sale, the payer finishes it at
/// the terminal by presenting the card.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Refund {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// The payment being refunded, as iyzico named it.
    pub payment_id: Box<str>,
    /// The day that payment was posted, `YYYYMMDD`.
    pub payment_date: Box<str>,
    /// How much to give back.
    pub amount: Money,
    /// Why, for whoever reads it later.
    pub reason: Option<Box<str>>,
    /// Anything more to say about it.
    pub description: Option<Box<str>>,
}

impl Refund {
    /// Starts building a refund of `amount` against a payment.
    ///
    /// `payment_date` is the `paymentDate` the sale answered — eight digits,
    /// `YYYYMMDD`, and not the date the refund is being made.
    #[must_use]
    pub fn builder(
        reference: Reference,
        payment_id: impl Into<Box<str>>,
        payment_date: impl Into<Box<str>>,
        amount: Money,
    ) -> RefundBuilder {
        RefundBuilder {
            reference,
            payment_id: payment_id.into(),
            payment_date: payment_date.into(),
            amount,
            reason: None,
            description: None,
        }
    }
}

/// Collects the parts of a [`Refund`] before they are checked.
#[derive(Debug, Clone)]
pub struct RefundBuilder {
    reference: Reference,
    payment_id: Box<str>,
    payment_date: Box<str>,
    amount: Money,
    reason: Option<Box<str>>,
    description: Option<Box<str>>,
}

impl RefundBuilder {
    /// Says why the money is going back.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<Box<str>>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Says anything more about it.
    #[must_use]
    pub fn description(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Checks the refund and produces it.
    pub fn build(self) -> Result<Refund, RequestError> {
        self.amount.require_positive()?;
        terminal_currency(self.amount.currency())?;
        payment_date(&self.payment_date)?;
        Ok(Refund {
            reference: self.reference,
            payment_id: self.payment_id,
            payment_date: self.payment_date,
            amount: self.amount,
            reason: self.reason,
            description: self.description,
        })
    }
}

/// A payment to withdraw before it settles.
///
/// Build one with [`Void::builder`]. iyzico's overview says a void is "only
/// valid for transactions that have not yet been included in end-of-day
/// closing"; after that the way back is a [`Refund`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Void {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// The payment being withdrawn, as iyzico named it.
    pub payment_id: Box<str>,
    /// The day that payment was posted, `YYYYMMDD`.
    pub payment_date: Box<str>,
    /// Why, for whoever reads it later.
    pub reason: Option<Box<str>>,
    /// Anything more to say about it.
    pub description: Option<Box<str>>,
}

impl Void {
    /// Starts building a void of a payment.
    #[must_use]
    pub fn builder(
        reference: Reference,
        payment_id: impl Into<Box<str>>,
        payment_date: impl Into<Box<str>>,
    ) -> VoidBuilder {
        VoidBuilder {
            reference,
            payment_id: payment_id.into(),
            payment_date: payment_date.into(),
            reason: None,
            description: None,
        }
    }
}

/// Collects the parts of a [`Void`] before they are checked.
#[derive(Debug, Clone)]
pub struct VoidBuilder {
    reference: Reference,
    payment_id: Box<str>,
    payment_date: Box<str>,
    reason: Option<Box<str>>,
    description: Option<Box<str>>,
}

impl VoidBuilder {
    /// Says why the payment is being withdrawn.
    #[must_use]
    pub fn reason(mut self, reason: impl Into<Box<str>>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Says anything more about it.
    #[must_use]
    pub fn description(mut self, description: impl Into<Box<str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Checks the void and produces it.
    pub fn build(self) -> Result<Void, RequestError> {
        payment_date(&self.payment_date)?;
        Ok(Void {
            reference: self.reference,
            payment_id: self.payment_id,
            payment_date: self.payment_date,
            reason: self.reason,
            description: self.description,
        })
    }
}

/// Which transaction to read back, in one of the three ways iyzico allows.
///
/// The schema marks `paymentId`, `deviceUniqueId` and `transactionReferenceId`
/// all required, and the note printed beside it says the opposite: *"the
/// fields `paymentId`, `transactionReferenceId` and `deviceUniqueId` are not
/// all mandatory at the same time"*, followed by the three combinations that
/// work. Error `380111` — "either `transactionReferenceId` or `paymentId` must be
/// provided" — is what a fourth gets.
///
/// So this is an enum of exactly those three, and the fourth cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Query {
    /// By payment alone: answers the sale.
    Payment {
        /// The caller's id for this request.
        conversation_id: Box<str>,
        /// The payment to read.
        payment_id: Box<str>,
    },
    /// By transaction and terminal: answers the payment, void or refund that
    /// reference belongs to.
    Transaction {
        /// The caller's id for this request.
        conversation_id: Box<str>,
        /// The terminal the transaction happened on.
        device_unique_id: Box<str>,
        /// The caller's own reference for the transaction to read.
        transaction_reference_id: Box<str>,
    },
    /// By both: answers the sale together with the void and refund on it.
    PaymentAndTransaction {
        /// The caller's id for this request.
        conversation_id: Box<str>,
        /// The payment to read.
        payment_id: Box<str>,
        /// The caller's own reference for the transaction to read.
        transaction_reference_id: Box<str>,
    },
}

impl Query {
    /// Reads a sale back by the `paymentId` iyzico issued for it.
    #[must_use]
    pub fn payment(conversation_id: impl Into<Box<str>>, payment_id: impl Into<Box<str>>) -> Self {
        Self::Payment {
            conversation_id: conversation_id.into(),
            payment_id: payment_id.into(),
        }
    }

    /// Reads a transaction back by the caller's own reference and its terminal.
    #[must_use]
    pub fn transaction(
        conversation_id: impl Into<Box<str>>,
        device_unique_id: impl Into<Box<str>>,
        transaction_reference_id: impl Into<Box<str>>,
    ) -> Self {
        Self::Transaction {
            conversation_id: conversation_id.into(),
            device_unique_id: device_unique_id.into(),
            transaction_reference_id: transaction_reference_id.into(),
        }
    }

    /// Reads a sale back with the void and refund that hang off it.
    #[must_use]
    pub fn payment_and_transaction(
        conversation_id: impl Into<Box<str>>,
        payment_id: impl Into<Box<str>>,
        transaction_reference_id: impl Into<Box<str>>,
    ) -> Self {
        Self::PaymentAndTransaction {
            conversation_id: conversation_id.into(),
            payment_id: payment_id.into(),
            transaction_reference_id: transaction_reference_id.into(),
        }
    }

    /// The caller's id for this request, whichever way it was built.
    #[must_use]
    pub fn conversation_id(&self) -> &str {
        match self {
            Self::Payment {
                conversation_id, ..
            }
            | Self::Transaction {
                conversation_id, ..
            }
            | Self::PaymentAndTransaction {
                conversation_id, ..
            } => conversation_id,
        }
    }
}

/// A request was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RequestError {
    /// The amount was not one iyzico will take.
    #[error(transparent)]
    Amount(#[from] MoneyError),
    /// iyzico documents no Terminal API transaction in this currency.
    #[error("iyzico documents no terminal transaction in {0}")]
    UnsupportedCurrency(Currency),
    /// More instalments than iyzico's list has.
    #[error("iyzico takes 0 to 12 instalments, was asked for {0}")]
    Installments(u8),
    /// The payment date was not eight digits.
    #[error("a payment date is eight digits, YYYYMMDD; got `{0}`")]
    PaymentDate(Box<str>),
    /// A provision-closing sale did not say which provision it closes.
    #[error("a POST_AUTH sale needs the paymentId of the PRE_AUTH it closes")]
    PostAuthWithoutPayment,
}

/// The currencies iyzico documents a Terminal API transaction in.
///
/// Three — `TRY`, `USD` and `EUR` — the same three [`subscription`] takes and
/// four fewer than an [`iyzilink`] link.
///
/// Establishing that took reading the whole group, because the VUK 509 request
/// this module sends types `currency` as a bare string with no `enum` at all:
/// the three are what every `currency` in `terminal-host` that carries an
/// `enum` carries — the VUK 507 sale request and both its response shapes — and
/// `TRY` is the only value iyzico's own examples ever show. Sending sterling
/// would be sending a currency nothing in the group documents, so it is refused
/// here rather than tried.
///
/// [`subscription`]: crate::subscription
/// [`iyzilink`]: crate::iyzilink
fn terminal_currency(currency: Currency) -> Result<(), RequestError> {
    match currency {
        Currency::Try | Currency::Usd | Currency::Eur => Ok(()),
        _ => Err(RequestError::UnsupportedCurrency(currency)),
    }
}

/// Refuses anything that is not the eight digits iyzico asks for.
///
/// `2026-08-14` is the shape a caller reaches for and `20260814` is the one
/// iyzico documents. The field is required on a refund and on a void, so
/// getting it wrong is a refusal at best.
fn payment_date(value: &str) -> Result<(), RequestError> {
    if value.len() == 8 && value.bytes().all(|b| b.is_ascii_digit()) {
        Ok(())
    } else {
        Err(RequestError::PaymentDate(value.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Locale, Query, Reference, Refund, RequestError, Sale, SalesType, Void, payment_date,
    };
    use kasapay_core::{Currency, Money};

    fn reference() -> Reference {
        Reference::new("conv-1", "PAV860047264", "txn-1")
    }

    fn lira(amount: &str) -> Money {
        Money::parse(amount, Currency::Try).expect("a valid amount")
    }

    #[test]
    fn a_currency_iyzico_does_not_document_here_is_refused_before_a_socket_opens() {
        for currency in [Currency::Try, Currency::Usd, Currency::Eur] {
            let amount = Money::from_minor_units(10_000, currency);
            assert!(Sale::builder(reference(), amount).build().is_ok());
        }
        for currency in [
            Currency::Gbp,
            Currency::Jpy,
            Currency::Kwd,
            Currency::Rub,
            Currency::Chf,
            Currency::Nok,
        ] {
            let amount = Money::from_minor_units(10_000, currency);
            assert_eq!(
                Sale::builder(reference(), amount).build().unwrap_err(),
                RequestError::UnsupportedCurrency(currency)
            );
        }
    }

    #[test]
    fn a_provision_closing_sale_has_to_name_the_provision() {
        let sale = Sale::builder(reference(), lira("100.00"))
            .sales_type(SalesType::PostAuth)
            .build();
        assert_eq!(sale.unwrap_err(), RequestError::PostAuthWithoutPayment);

        let sale = Sale::builder(reference(), lira("100.00"))
            .sales_type(SalesType::PostAuth)
            .payment_id("30001")
            .build()
            .expect("a post-auth that names its provision");
        assert_eq!(sale.payment_id.as_deref(), Some("30001"));
    }

    #[test]
    fn more_instalments_than_iyzico_lists_are_refused() {
        assert!(
            Sale::builder(reference(), lira("100.00"))
                .installments(12)
                .build()
                .is_ok()
        );
        assert_eq!(
            Sale::builder(reference(), lira("100.00"))
                .installments(13)
                .build()
                .unwrap_err(),
            RequestError::Installments(13)
        );
    }

    #[test]
    fn a_payment_date_is_the_eight_digits_iyzico_documents() {
        assert!(payment_date("20260814").is_ok());
        // The shape a caller reaches for, and the one iyzico does not take.
        for wrong in ["2026-08-14", "2026081", "202608145", "2026081a", ""] {
            assert!(payment_date(wrong).is_err(), "{wrong} was let through");
        }
    }

    #[test]
    fn a_refund_and_a_void_check_the_date_they_carry() {
        assert!(
            Refund::builder(reference(), "30001", "20260814", lira("50.00"))
                .build()
                .is_ok()
        );
        assert!(
            Refund::builder(reference(), "30001", "2026-08-14", lira("50.00"))
                .build()
                .is_err()
        );
        assert!(
            Void::builder(reference(), "30001", "20260814")
                .build()
                .is_ok()
        );
        assert!(
            Void::builder(reference(), "30001", "14/08/2026")
                .build()
                .is_err()
        );
    }

    #[test]
    fn a_refund_of_nothing_is_refused() {
        let none = Money::from_minor_units(0, Currency::Try);
        assert!(
            Refund::builder(reference(), "30001", "20260814", none)
                .build()
                .is_err()
        );
    }

    #[test]
    fn the_three_documented_query_combinations_all_carry_a_conversation() {
        assert_eq!(Query::payment("c", "30001").conversation_id(), "c");
        assert_eq!(Query::transaction("c", "dev", "txn").conversation_id(), "c");
        assert_eq!(
            Query::payment_and_transaction("c", "30001", "txn").conversation_id(),
            "c"
        );
    }

    #[test]
    fn the_words_iyzico_expects_are_the_ones_sent() {
        assert_eq!(SalesType::default(), SalesType::Sale);
        assert_eq!(SalesType::Sale.as_str(), "SALE");
        assert_eq!(SalesType::PreAuth.as_str(), "PRE_AUTH");
        assert_eq!(SalesType::PostAuth.as_str(), "POST_AUTH");
        // Lowercase, per the fragment's enum, not the "TR" of iyzico's sample.
        assert_eq!(Locale::default().as_str(), "tr");
        assert_eq!(Locale::English.as_str(), "en");
    }
}
