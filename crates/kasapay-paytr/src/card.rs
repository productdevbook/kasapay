//! What PayTR's BIN service knows about a card.
//!
//! Every value here is one PayTR enumerates on
//! <https://dev.paytr.com/direkt-api/bin-sorgulama-servisi>. Nothing is
//! inferred from the number itself.

/// Whether a card draws on a balance or on a credit line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    /// PayTR's `credit`.
    Credit,
    /// PayTR's `debit`.
    Debit,
}

impl CardKind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "credit" => Some(Self::Credit),
            "debit" => Some(Self::Debit),
            _ => None,
        }
    }
}

/// The network a card runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardScheme {
    /// PayTR's `VISA`.
    Visa,
    /// PayTR's `MASTERCARD`.
    Mastercard,
    /// PayTR's `AMEX`.
    Amex,
    /// PayTR's `TROY`.
    Troy,
    /// PayTR's `OTHER`, which it answers when it does not know the network.
    Other,
}

impl CardScheme {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "VISA" => Some(Self::Visa),
            "MASTERCARD" => Some(Self::Mastercard),
            "AMEX" => Some(Self::Amex),
            "TROY" => Some(Self::Troy),
            "OTHER" => Some(Self::Other),
            _ => None,
        }
    }
}

/// What PayTR knows about a BIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardDetails {
    /// PayTR's `cardType`.
    pub kind: CardKind,
    /// PayTR's `businessCard`: whether the card belongs to a company.
    pub business: bool,
    /// PayTR's `bank`, spelled as PayTR spells it — `Yapı Kredi`, not a code.
    pub bank: Box<str>,
    /// PayTR's `bankCode`, which carries leading zeros: `0010`.
    pub bank_code: Box<str>,
    /// PayTR's `brand`: the instalment programme the card belongs to, such as
    /// `axess` or `bonus`.
    ///
    /// `None` is PayTR's `none`, and it is load-bearing: **a card in no
    /// programme cannot be paid in instalments through PayTR.** It is also the
    /// value that goes into a Direkt API payment's `card_type`.
    pub programme: Option<Box<str>>,
    /// PayTR's `schema`.
    pub scheme: CardScheme,
    /// PayTR's `allow_non3d`: whether this card may be charged without 3-D
    /// Secure.
    pub non_3d_allowed: bool,
}
