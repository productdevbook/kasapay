//! What a mass payout is made of, on the way out.
//!
//! Money leaving for somebody else's bank account, not money arriving, so none
//! of this is [`ChargeRequest`](kasapay_core::ChargeRequest)'s shape and none
//! of it can be undone once [`Client::authorize`](crate::mass::Client::authorize)
//! has been called.

use std::fmt;

use kasapay_core::{Currency, Money, MoneyError};

/// A mass payout, built and not yet sent.
///
/// Build one with [`NewPayout::builder`]. Sending it creates the payout in
/// `INIT`; nothing moves until it is authorized.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewPayout {
    /// The caller's own identifier for the whole payout.
    ///
    /// iyzico calls this the idempotency key and does not say what a repeat of
    /// one does — see the module documentation.
    pub external_id: Box<str>,
    /// The caller's own reference, echoed back on the answer.
    pub conversation_id: Box<str>,
    /// What the money is for.
    pub purpose: Option<Purpose>,
    /// The lines. At least one, which is iyzico's own minimum.
    pub items: Vec<NewPayoutItem>,
}

impl NewPayout {
    /// Starts building a payout.
    ///
    /// Both identifiers are the caller's own. `conversation_id` is required
    /// here because iyzico's English page requires it and their Turkish page
    /// does not: sending it satisfies both readings.
    #[must_use]
    pub fn builder(
        external_id: impl Into<Box<str>>,
        conversation_id: impl Into<Box<str>>,
    ) -> NewPayoutBuilder {
        NewPayoutBuilder {
            external_id: external_id.into(),
            conversation_id: conversation_id.into(),
            purpose: None,
            items: Vec::new(),
        }
    }

    /// The currencies the lines are in, in the order they first appear.
    ///
    /// More than one is a payout iyzico's own answer cannot describe: the
    /// summary that comes back carries a single `currency` for the whole
    /// payout. Nothing here refuses it, because iyzico does not.
    #[must_use]
    pub fn currencies(&self) -> Vec<Currency> {
        let mut seen: Vec<Currency> = Vec::new();
        for item in &self.items {
            let currency = item.amount.currency();
            if !seen.contains(&currency) {
                seen.push(currency);
            }
        }
        seen
    }
}

/// Collects the parts of a [`NewPayout`] before it is checked.
#[derive(Debug, Clone)]
pub struct NewPayoutBuilder {
    external_id: Box<str>,
    conversation_id: Box<str>,
    purpose: Option<Purpose>,
    items: Vec<NewPayoutItem>,
}

impl NewPayoutBuilder {
    /// Says what the money is for.
    #[must_use]
    pub fn purpose(mut self, purpose: Purpose) -> Self {
        self.purpose = Some(purpose);
        self
    }

    /// Adds one line.
    #[must_use]
    pub fn item(mut self, item: NewPayoutItem) -> Self {
        self.items.push(item);
        self
    }

    /// Adds several lines.
    #[must_use]
    pub fn items(mut self, items: impl IntoIterator<Item = NewPayoutItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Checks the payout and produces it.
    pub fn build(self) -> Result<NewPayout, PayoutError> {
        blank("externalId", &self.external_id)?;
        blank("conversationId", &self.conversation_id)?;
        if self.items.is_empty() {
            return Err(PayoutError::NoItems);
        }
        Ok(NewPayout {
            external_id: self.external_id,
            conversation_id: self.conversation_id,
            purpose: self.purpose,
            items: self.items,
        })
    }
}

/// One line of a mass payout: who is paid, and how much.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NewPayoutItem {
    /// The caller's own identifier for this line.
    pub external_id: Box<str>,
    /// Who is paid.
    pub recipient: Recipient,
    /// What they are paid.
    pub amount: Money,
    /// What the line is for. iyzico requires it.
    pub description: Box<str>,
    /// Whether the money, once in an iyzico wallet, may be withdrawn.
    pub non_withdrawable: Option<bool>,
}

impl NewPayoutItem {
    /// Starts building a line.
    ///
    /// Every argument is one iyzico requires. The recipient carries their name
    /// with them when it is an IBAN, because that is the one case iyzico makes
    /// the name mandatory in — see [`Recipient::Iban`].
    #[must_use]
    pub fn builder(
        external_id: impl Into<Box<str>>,
        recipient: Recipient,
        amount: Money,
        description: impl Into<Box<str>>,
    ) -> NewPayoutItemBuilder {
        NewPayoutItemBuilder {
            external_id: external_id.into(),
            recipient,
            amount,
            description: description.into(),
            non_withdrawable: None,
        }
    }
}

/// Collects the parts of a [`NewPayoutItem`] before it is checked.
#[derive(Debug, Clone)]
pub struct NewPayoutItemBuilder {
    external_id: Box<str>,
    recipient: Recipient,
    amount: Money,
    description: Box<str>,
    non_withdrawable: Option<bool>,
}

impl NewPayoutItemBuilder {
    /// Stops the recipient withdrawing what lands in their iyzico wallet.
    ///
    /// iyzico documents this in one sentence — *"iyzico ensures that the fee
    /// sent to your wallet cannot be withdrawn"* — and does not say what it
    /// means for a line paid to an IBAN, which never reaches a wallet, nor
    /// what lifts the restriction afterwards.
    #[must_use]
    pub const fn non_withdrawable(mut self, non_withdrawable: bool) -> Self {
        self.non_withdrawable = Some(non_withdrawable);
        self
    }

    /// Checks the line and produces it.
    pub fn build(self) -> Result<NewPayoutItem, PayoutError> {
        blank("itemExternalId", &self.external_id)?;
        blank("description", &self.description)?;
        blank("recipientInfo", self.recipient.info())?;
        if let Some(name) = self.recipient.name() {
            blank("recipientName", name)?;
        }
        self.amount.require_positive()?;
        payout_currency(self.amount.currency())?;
        Ok(NewPayoutItem {
            external_id: self.external_id,
            recipient: self.recipient,
            amount: self.amount,
            description: self.description,
            non_withdrawable: self.non_withdrawable,
        })
    }
}

/// Who a line is paid to.
///
/// iyzico carries this as a `recipientType` and a `recipientInfo` that has to
/// match it, plus a `recipientName` that is *mandatory when the type is IBAN*
/// and unmentioned otherwise. Those three fields have exactly one valid
/// combination each, so they are one value here: a [`Recipient::Iban`] cannot
/// be built without the name, and a phone number cannot be labelled an
/// identity number by mistake.
///
/// **Nothing here checks that the identifier belongs to whoever is meant to be
/// paid.** No IBAN checksum is computed — iyzico documents no format for the
/// field — and no name is matched against an account holder. A well-formed
/// identifier for the wrong person is paid, not refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Recipient {
    /// A mobile number, in the `+90…` form iyzico's example uses.
    Phone(Box<str>),
    /// A bank account, with the name of whoever holds it.
    Iban {
        /// The account number.
        iban: Box<str>,
        /// Whose account it is. iyzico requires it for an IBAN and for
        /// nothing else.
        holder: Box<str>,
    },
    /// A Turkish national identity number.
    IdentityNumber(Box<str>),
    /// An iyzico member id, for money staying inside iyzico.
    MemberId(Box<str>),
}

impl Recipient {
    /// Which of iyzico's four kinds this is.
    #[must_use]
    pub fn kind(&self) -> RecipientKind {
        match self {
            Self::Phone(_) => RecipientKind::Phone,
            Self::Iban { .. } => RecipientKind::Iban,
            Self::IdentityNumber(_) => RecipientKind::IdentityNumber,
            Self::MemberId(_) => RecipientKind::MemberId,
        }
    }

    /// The identifier itself, whatever kind it is.
    #[must_use]
    pub fn info(&self) -> &str {
        match self {
            Self::Phone(value) | Self::IdentityNumber(value) | Self::MemberId(value) => value,
            Self::Iban { iban, .. } => iban,
        }
    }

    /// The name that travels with the identifier, when iyzico wants one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Iban { holder, .. } => Some(holder),
            Self::Phone(_) | Self::IdentityNumber(_) | Self::MemberId(_) => None,
        }
    }

    /// The `recipientType` iyzico expects on the wire.
    ///
    /// [`Recipient::kind`] says the same thing as a value; this is the word
    /// itself, and unlike the value it outlives what it came from.
    #[must_use]
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Phone(_) => "PHONE",
            Self::Iban { .. } => "IBAN",
            Self::IdentityNumber(_) => "IDENTITY_NUMBER",
            Self::MemberId(_) => "MEMBER_ID",
        }
    }
}

/// What kind of identifier names a recipient.
///
/// Open, because this is also what a line read back says it was, and iyzico
/// may name a kind that did not exist when this was written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecipientKind {
    /// A mobile number.
    Phone,
    /// A bank account.
    Iban,
    /// A Turkish national identity number.
    IdentityNumber,
    /// An iyzico member id.
    MemberId,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl RecipientKind {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Phone => "PHONE",
            Self::Iban => "IBAN",
            Self::IdentityNumber => "IDENTITY_NUMBER",
            Self::MemberId => "MEMBER_ID",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for RecipientKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for RecipientKind {
    fn from(value: &str) -> Self {
        match value {
            "PHONE" => Self::Phone,
            "IBAN" => Self::Iban,
            "IDENTITY_NUMBER" => Self::IdentityNumber,
            "MEMBER_ID" => Self::MemberId,
            other => Self::Other(other.into()),
        }
    }
}

/// What a payout is for.
///
/// iyzico's own list, and the two documentation languages give different ones:
/// the English page names eight, the Turkish page the first five. `Cashback`,
/// `Rebates` and `Donations` are therefore English-only, and
/// [`Purpose::Other`] is there for a word iyzico adds later.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Purpose {
    /// Wages.
    Salary,
    /// A bonus on top of wages.
    Bonus,
    /// Settling up — a marketplace paying its sellers.
    Settlement,
    /// Goods bought.
    Goods,
    /// Services rendered.
    Services,
    /// Money given back to a customer. English pages only.
    Cashback,
    /// A rebate. English pages only.
    Rebates,
    /// A donation. English pages only.
    Donations,
    /// Something iyzico has started taking since this was written.
    Other(Box<str>),
}

impl Purpose {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Salary => "SALARY",
            Self::Bonus => "BONUS",
            Self::Settlement => "SETTLEMENT",
            Self::Goods => "GOODS",
            Self::Services => "SERVICES",
            Self::Cashback => "CASHBACK",
            Self::Rebates => "REBATES",
            Self::Donations => "DONATIONS",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for Purpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for Purpose {
    fn from(value: &str) -> Self {
        match value {
            "SALARY" => Self::Salary,
            "BONUS" => Self::Bonus,
            "SETTLEMENT" => Self::Settlement,
            "GOODS" => Self::Goods,
            "SERVICES" => Self::Services,
            "CASHBACK" => Self::Cashback,
            "REBATES" => Self::Rebates,
            "DONATIONS" => Self::Donations,
            other => Self::Other(other.into()),
        }
    }
}

/// Which lines of a payout to read back.
///
/// Documented in Turkish, where it is required, and absent from the English
/// page altogether. [`Client::payout`](crate::mass::Client::payout) makes the
/// caller choose rather than guessing a default neither page states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemFilter {
    /// The lines iyzico accepted.
    Valid,
    /// The lines it refused.
    Invalid,
}

impl ItemFilter {
    /// The word iyzico expects on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "VALID",
            Self::Invalid => "INVALID",
        }
    }
}

impl fmt::Display for ItemFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which payout to read back.
///
/// iyzico takes `requestId`, `externalMassPayoutId`, or — says the English
/// page — both. What happens when both are sent and they name different
/// payouts is not documented, so this names one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayoutRef {
    /// The `requestId` iyzico answered the initialization with.
    Request(Box<str>),
    /// The `externalId` the caller chose for it.
    External(Box<str>),
}

/// The currencies iyzico documents a mass payout in.
///
/// Three — `TRY`, `USD` and `EUR` — and that list comes from the Turkish page
/// alone: the English one types the field `string` and names no currencies at
/// all. Where one page constrains and the other is silent, the constraint is
/// the only thing either of them actually says, so the other six are refused
/// here rather than sent out to a bank.
fn payout_currency(currency: Currency) -> Result<(), PayoutError> {
    match currency {
        Currency::Try | Currency::Usd | Currency::Eur => Ok(()),
        Currency::Gbp
        | Currency::Jpy
        | Currency::Kwd
        | Currency::Rub
        | Currency::Chf
        | Currency::Nok => Err(PayoutError::UnsupportedCurrency(currency)),
    }
}

/// Refuses a field iyzico requires and that carries nothing.
fn blank(field: &'static str, value: &str) -> Result<(), PayoutError> {
    if value.trim().is_empty() {
        return Err(PayoutError::Blank(field));
    }
    Ok(())
}

/// A payout was built out of parts iyzico will not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PayoutError {
    /// The amount was not one iyzico will send.
    #[error(transparent)]
    Amount(#[from] MoneyError),
    /// iyzico does not document a mass payout in this currency.
    #[error("iyzico documents no mass payout in {0}")]
    UnsupportedCurrency(Currency),
    /// A field iyzico requires carried nothing.
    #[error("a mass payout's `{0}` cannot be blank")]
    Blank(&'static str),
    /// A payout with no lines in it.
    #[error("iyzico documents a mass payout of at least one item")]
    NoItems,
}
