//! The mass payout client.

use std::fmt;

use kasapay_core::{Currency, Error, ErrorKind, Money, MoneyError, ProviderId, Raw};
use reqwest::Method;
use serde_json::value::RawValue;

use crate::classic;
use crate::mass::payout::{ItemFilter, NewPayout, PayoutRef, Purpose, RecipientKind};
use crate::mass::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// Where a payout is created.
const INIT: &str = "/v1/mass/payout/init";
/// Where it is set going.
const AUTH: &str = "/v1/mass/payout/auth";
/// Where one still in `INIT` is stopped. The tracking id goes on the end.
const CANCEL: &str = "/v1/mass/payout/cancel";
/// What is left to pay out with.
const BALANCE: &str = "/v1/mass/payout/balance";
/// A payout and its lines.
const RETRIEVE: &str = "/v1/mass/payout/retrieve";
/// One line of one. The reference code goes on the end.
const ITEMS: &str = "/v1/mass/payout/retrieve/items";
/// What language iyzico answers in, on the query string of every call.
///
/// Documented as a **required query parameter** on all six operations, which
/// is not where the rest of the classic API puts it — [`iyzilink`](crate::iyzilink)
/// and [`subscription`](crate::subscription) send `locale` in the body of
/// anything that has one. Mass payout's request bodies document no `locale`
/// field at all, so it travels in the query even on a `POST`.
const LOCALE_QUERY: &str = "?locale=tr";

/// Talks to iyzico's mass payout API.
///
/// Built over a [`classic::Client`], because that is what mass payout is: the
/// same host, the same [`IYZWSv2`](crate::Credentials) signing, the same
/// `status: "failure"` envelope. Cloning shares the one connection pool.
#[derive(Debug, Clone)]
pub struct Client {
    classic: classic::Client,
}

impl Client {
    /// Speaks mass payout over a classic client.
    #[must_use]
    pub const fn new(classic: classic::Client) -> Self {
        Self { classic }
    }

    /// The classic client underneath, for everything that is not a payout.
    #[must_use]
    pub const fn classic(&self) -> &classic::Client {
        &self.classic
    }

    /// Creates a payout, without sending any money.
    ///
    /// Every valid line is stored in `INIT` and nothing leaves until
    /// [`Client::authorize`] is called with the `requestId` that comes back.
    ///
    /// # A success is not an acceptance of every line
    ///
    /// iyzico answers `status: "success"` and puts whatever it would not take
    /// in [`Started::invalid_items`]. Those lines are stored `INVALID`, are
    /// never paid, and are not mentioned again by the authorization. Read the
    /// list before authorizing, or a payout of a hundred people quietly
    /// becomes a payout of ninety-seven.
    pub async fn start(&self, payout: &NewPayout) -> Result<Started, Error> {
        let mut items = Vec::with_capacity(payout.items.len());
        for item in &payout.items {
            items.push(wire::Item {
                external_id: &item.external_id,
                recipient_type: item.recipient.kind_str(),
                recipient_info: item.recipient.info(),
                amount: wire::Amount {
                    value: wire::amount(item.amount)?,
                    currency: item.amount.currency().code(),
                },
                description: &item.description,
                recipient_name: item.recipient.name(),
                non_withdrawable: item.non_withdrawable,
            });
        }
        let body = wire::InitRequest {
            external_id: &payout.external_id,
            conversation_id: &payout.conversation_id,
            purpose: payout.purpose.as_ref().map(Purpose::as_str),
            items,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::InitResponse>(Method::POST, INIT, LOCALE_QUERY, Some(&body))
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to create the mass payout",
        ) {
            return Err(error);
        }
        Ok(Started {
            request_id: response.request_id.unwrap_or_default().into_boxed_str(),
            conversation_id: response.conversation_id.map(String::into_boxed_str),
            invalid_items: response
                .invalid_items
                .unwrap_or_default()
                .into_iter()
                .map(|item| InvalidItem {
                    external_id: item.external_id.map(String::into_boxed_str),
                    error_code: item.error_code.map(String::into_boxed_str),
                    error_message: item.error_message.map(String::into_boxed_str),
                })
                .collect(),
            raw,
        })
    }

    /// Sets a payout going. **This is the one that spends the money.**
    ///
    /// Every line still in `INIT` moves to `PROCESSING` and is queued for the
    /// bank. iyzico documents no way back from here: [`Client::cancel`] only
    /// takes a payout that has *not* been authorized.
    ///
    /// The answer carries nothing but `status`, `systemTime` and `locale` —
    /// no count of what was queued, no total — so `Ok(())` means iyzico
    /// accepted the instruction, not that any particular line was paid.
    /// [`Client::payout`] is what says what became of them.
    pub async fn authorize(&self, request_id: &str) -> Result<(), Error> {
        let body = wire::AuthRequest {
            request_id: required_id(request_id)?,
        };
        let (response, _) = self
            .classic
            .request::<_, wire::Ack>(Method::POST, AUTH, LOCALE_QUERY, Some(&body))
            .await?;
        acknowledged(response, "iyzico refused to authorize the mass payout")
    }

    /// Stops a payout that has not been authorized.
    ///
    /// The payout and every line of it still in `INIT` go to `CANCELED` and
    /// `MASS_PAYOUT_CANCELED`. iyzico says nothing about what this does to a
    /// payout already authorized, and nothing here asks first.
    pub async fn cancel(&self, request_id: &str) -> Result<(), Error> {
        let path = format!("{CANCEL}/{}", path_segment(request_id)?);
        let (response, _) = self
            .classic
            .request::<(), wire::Ack>(Method::POST, &path, LOCALE_QUERY, None)
            .await?;
        acknowledged(response, "iyzico refused to cancel the mass payout")
    }

    /// What is left in the merchant's mass payout balance.
    ///
    /// A payout is paid out of this rather than out of the merchant's ordinary
    /// iyzico balance, and a payout larger than it comes back
    /// `INSUFFICIENT_BALANCE`.
    pub async fn balance(&self) -> Result<Balance, Error> {
        let (response, raw) = self
            .classic
            .request::<(), wire::BalanceResponse>(Method::GET, BALANCE, LOCALE_QUERY, None)
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused the mass payout balance",
        ) {
            return Err(error);
        }
        Ok(Balance {
            amount: response
                .balance
                .as_deref()
                .map(|value| wire::text(value).into()),
            raw,
        })
    }

    /// Reads a payout back, one page of its lines at a time.
    ///
    /// `page` counts from **zero** — iyzico's own worked example asks for
    /// `"page": 0` — which is not how [`subscription`](crate::subscription)
    /// pages, and neither page of the documentation states a maximum `size`.
    /// Both are sent as given.
    ///
    /// `items` decides whether the lines that come back are the ones iyzico
    /// accepted or the ones it refused; there is no way to ask for both.
    pub async fn payout(
        &self,
        which: &PayoutRef,
        items: ItemFilter,
        page: u32,
        size: u32,
    ) -> Result<Payout, Error> {
        let (request_id, external_id) = match which {
            PayoutRef::Request(id) => (Some(&**id), None),
            PayoutRef::External(id) => (None, Some(&**id)),
        };
        let body = wire::RetrieveRequest {
            request_id,
            external_mass_payout_id: external_id,
            item_type: items.as_str(),
            page,
            size,
        };
        let (response, raw) = self
            .classic
            .request::<_, wire::RetrieveResponse>(Method::POST, RETRIEVE, LOCALE_QUERY, Some(&body))
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to read the mass payout",
        ) {
            return Err(error);
        }
        let page_of_items = response.mass_payout_items.unwrap_or_default();
        let lines = page_of_items.items.unwrap_or_default();
        let mut read_lines = Vec::with_capacity(lines.len());
        for line in &lines {
            read_lines.push(PayoutItem::read(line)?);
        }
        let summary = match response.mass_payout.as_deref() {
            Some(value) => Some(Summary::read(value)?),
            None => None,
        };
        Ok(Payout {
            summary,
            items: read_lines,
            page: response.page.or(page_of_items.page),
            size: response.size.or(page_of_items.size),
            total: response.total.or(page_of_items.total),
            raw,
        })
    }

    /// Reads one line of a payout back, by the reference code iyzico gave it.
    pub async fn item(&self, reference_code: &str) -> Result<PayoutItem, Error> {
        let path = format!("{ITEMS}/{}", path_segment(reference_code)?);
        let (response, _) = self
            .classic
            .request::<(), wire::ItemResponse>(Method::GET, &path, LOCALE_QUERY, None)
            .await?;
        if let Some(error) = classic::refused(
            response.status.as_deref(),
            response.error_message,
            response.error_code,
            "iyzico refused to read the mass payout item",
        ) {
            return Err(error);
        }
        let item = response.item.ok_or_else(|| {
            Error::new(ErrorKind::Malformed, PROVIDER, "the answer carried no item")
        })?;
        PayoutItem::read(&item)
    }
}

impl From<classic::Client> for Client {
    fn from(classic: classic::Client) -> Self {
        Self::new(classic)
    }
}

/// A mass payout that exists and has not been paid.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Started {
    /// What iyzico calls this payout. Authorizing, cancelling and reading it
    /// back all name it by this.
    pub request_id: Box<str>,
    /// The caller's own reference, back again.
    pub conversation_id: Option<Box<str>>,
    /// The lines iyzico would not take. **Empty is the only safe reading of
    /// "everything went in"** — see [`Client::start`].
    pub invalid_items: Vec<InvalidItem>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// A line iyzico refused while accepting the payout around it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InvalidItem {
    /// Which line. iyzico calls this `externalId` here and `itemExternalId`
    /// on the way in; it is the same value.
    pub external_id: Option<Box<str>>,
    /// iyzico's code for what was wrong.
    pub error_code: Option<Box<str>>,
    /// iyzico's words for it, in the language the call asked for.
    pub error_message: Option<Box<str>>,
}

/// What is left to pay out with.
///
/// **iyzico documents no currency on this answer** — one number, and nothing
/// saying what it counts. Their Turkish pages document a payout in three
/// currencies, so a single scalar cannot be the whole story, and this crate
/// will not decide it is lira. The digits are here as iyzico wrote them;
/// [`Balance::in_currency`] turns them into [`Money`] for a caller who knows
/// from their own account which currency it is.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Balance {
    /// The number iyzico sent, exactly as it wrote it.
    pub amount: Option<Box<str>>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

impl Balance {
    /// Reads the balance as an amount in a currency the caller names.
    ///
    /// # Errors
    ///
    /// [`MoneyError::NotDecimal`] when iyzico sent no balance at all, and
    /// whatever [`Money::parse`] says otherwise.
    pub fn in_currency(&self, currency: Currency) -> Result<Money, MoneyError> {
        match self.amount.as_deref() {
            Some(amount) => Money::parse(amount, currency),
            None => Err(MoneyError::NotDecimal(String::new())),
        }
    }
}

/// A mass payout, read back with one page of its lines.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payout {
    /// The payout itself.
    pub summary: Option<Summary>,
    /// The lines on this page, of whichever [`ItemFilter`] was asked for.
    pub items: Vec<PayoutItem>,
    /// Which page this is, counting from zero.
    pub page: Option<i64>,
    /// How many lines were asked for.
    pub size: Option<i64>,
    /// How many lines there are in total.
    pub total: Option<i64>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// The payout itself, without its lines.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Summary {
    /// The identifier the caller chose for it.
    pub external_id: Option<Box<str>>,
    /// Which merchant it belongs to.
    pub merchant_id: Option<i64>,
    /// Where it has got to.
    pub status: Option<PayoutStatus>,
    /// What the payout comes to.
    ///
    /// `None` when iyzico named a currency [`Currency`] cannot, or wrote an
    /// amount that is not one in that currency. The digits stay in
    /// [`Summary::raw`] either way.
    pub total_amount: Option<Money>,
    /// What has actually been paid so far.
    pub total_successful_amount: Option<Money>,
    /// What iyzico charges for the payout.
    pub total_commission_amount: Option<Money>,
    /// What all three are counted in.
    pub currency: Option<Currency>,
    /// iyzico's own answer for the payout, untouched.
    pub raw: Raw,
}

impl Summary {
    /// Reads the payout out of the bytes iyzico sent for it.
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::SummaryItem = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a mass payout was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        let currency = item
            .currency
            .as_deref()
            .and_then(|code| code.parse::<Currency>().ok());
        Ok(Self {
            external_id: item.external_id.map(String::into_boxed_str),
            merchant_id: item.merchant_id.as_deref().and_then(wire::integer),
            status: item.mass_payout_status.as_deref().map(PayoutStatus::from),
            total_amount: money(item.total_amount.as_deref(), currency),
            total_successful_amount: money(item.total_successful_amount.as_deref(), currency),
            total_commission_amount: money(item.total_commission_amount.as_deref(), currency),
            currency,
            raw: Raw::from_text(value.get()),
        })
    }
}

/// One line of a mass payout, read back.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PayoutItem {
    /// The identifier the caller chose for this line.
    pub external_id: Option<Box<str>>,
    /// What iyzico calls it, and what [`Client::item`] reads it by.
    pub reference_code: Box<str>,
    /// What kind of identifier names the recipient.
    pub recipient_kind: Option<RecipientKind>,
    /// The identifier itself.
    pub recipient_info: Option<Box<str>>,
    /// Whoever is being paid, when iyzico carries a name.
    pub recipient_name: Option<Box<str>>,
    /// What the line was for.
    pub description: Option<Box<str>>,
    /// Where the line has got to.
    pub status: Option<ItemStatus>,
    /// What went wrong, when something did.
    pub error_messages: Vec<Box<str>>,
    /// What the line comes to.
    ///
    /// iyzico never says whether this is what the caller asked to send or
    /// that plus the commission — see the module documentation. `None` when
    /// the currency or the digits could not be read; both stay in
    /// [`PayoutItem::raw`].
    pub total_amount: Option<Money>,
    /// What iyzico charges for the line.
    pub commission_amount: Option<Money>,
    /// What both are counted in.
    pub currency: Option<Currency>,
    /// iyzico's own answer for this line, untouched.
    pub raw: Raw,
}

impl PayoutItem {
    /// Reads one line out of the bytes iyzico sent for it.
    fn read(value: &RawValue) -> Result<Self, Error> {
        let item: wire::ItemDetail = serde_json::from_str(value.get()).map_err(|e| {
            Error::new(
                ErrorKind::Malformed,
                PROVIDER,
                "a mass payout item was not the JSON this endpoint documents",
            )
            .with_source(e)
        })?;
        let currency = item
            .currency_code
            .as_deref()
            .and_then(|code| code.parse::<Currency>().ok());
        Ok(Self {
            external_id: item.item_external_id.map(String::into_boxed_str),
            reference_code: item.reference_code.unwrap_or_default().into_boxed_str(),
            recipient_kind: item.recipient_type.as_deref().map(RecipientKind::from),
            recipient_info: item.recipient_info.map(String::into_boxed_str),
            recipient_name: item.recipient_name.map(String::into_boxed_str),
            description: item.description.map(String::into_boxed_str),
            status: item.item_status.as_deref().map(ItemStatus::from),
            error_messages: item
                .error_messages
                .unwrap_or_default()
                .into_iter()
                .map(String::into_boxed_str)
                .collect(),
            total_amount: money(item.total_amount.as_deref(), currency),
            commission_amount: money(item.commission_amount.as_deref(), currency),
            currency,
            raw: Raw::from_text(value.get()),
        })
    }
}

/// Where a whole mass payout has got to.
///
/// iyzico's seven, with their own descriptions. Open, because a payment system
/// adds states.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PayoutStatus {
    /// Received, and not yet authorized. Nothing has been spent.
    Init,
    /// Authorized, and being worked through.
    InProgress,
    /// On iyzico's queue; at least one line has reached a bank.
    PublishedToQueue,
    /// Every line was processed successfully.
    Completed,
    /// Every line failed at the bank.
    Fail,
    /// The mass payout balance was not enough to run it.
    InsufficientBalance,
    /// Cancelled before it was authorized.
    Canceled,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl PayoutStatus {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Init => "INIT",
            Self::InProgress => "IN_PROGRESS",
            Self::PublishedToQueue => "PUBLISHED_TO_QUEUE",
            Self::Completed => "COMPLETED",
            Self::Fail => "FAIL",
            Self::InsufficientBalance => "INSUFFICIENT_BALANCE",
            Self::Canceled => "CANCELED",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for PayoutStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for PayoutStatus {
    fn from(value: &str) -> Self {
        match value {
            "INIT" => Self::Init,
            "IN_PROGRESS" => Self::InProgress,
            "PUBLISHED_TO_QUEUE" => Self::PublishedToQueue,
            "COMPLETED" => Self::Completed,
            "FAIL" => Self::Fail,
            "INSUFFICIENT_BALANCE" => Self::InsufficientBalance,
            "CANCELED" => Self::Canceled,
            other => Self::Other(other.into()),
        }
    }
}

/// Where one line of a mass payout has got to.
///
/// iyzico's nine. Note that a line ending `DEPOSIT_SUCCESS` is a line that
/// **failed**: the money and its commission came back to the merchant's mass
/// payout balance, and nobody was paid.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemStatus {
    /// Sent to the bank, and refused there.
    Failed,
    /// Sent to the bank, and paid.
    Success,
    /// Stored, waiting for the payout to be authorized.
    Init,
    /// Refused by iyzico before anything was sent: a parameter was wrong,
    /// missing, or inconsistent with another.
    Invalid,
    /// At the bank, still going.
    Processing,
    /// Cancelled because the payout around it was.
    MassPayoutCanceled,
    /// Queued, for a bank transfer or for a wallet.
    Queued,
    /// It failed, and the money and commission went back to the merchant's
    /// mass payout balance.
    DepositSuccess,
    /// It failed, and the money did **not** go back. Somebody has to chase it.
    DepositFail,
    /// Something iyzico has started returning since this was written.
    Other(Box<str>),
}

impl ItemStatus {
    /// The word iyzico uses on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Failed => "FAILED",
            Self::Success => "SUCCESS",
            Self::Init => "INIT",
            Self::Invalid => "INVALID",
            Self::Processing => "PROCESSING",
            Self::MassPayoutCanceled => "MASS_PAYOUT_CANCELED",
            Self::Queued => "QUEUED",
            Self::DepositSuccess => "DEPOSIT_SUCCESS",
            Self::DepositFail => "DEPOSIT_FAIL",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for ItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<&str> for ItemStatus {
    fn from(value: &str) -> Self {
        match value {
            "FAILED" => Self::Failed,
            "SUCCESS" => Self::Success,
            "INIT" => Self::Init,
            "INVALID" => Self::Invalid,
            "PROCESSING" => Self::Processing,
            "MASS_PAYOUT_CANCELED" => Self::MassPayoutCanceled,
            "QUEUED" => Self::Queued,
            "DEPOSIT_SUCCESS" => Self::DepositSuccess,
            "DEPOSIT_FAIL" => Self::DepositFail,
            other => Self::Other(other.into()),
        }
    }
}

/// Reads one of iyzico's amounts, when there is a currency to read it in.
///
/// `None` for an amount with no currency beside it, a currency
/// [`Currency`] has no name for, or digits that are not an amount in it. The
/// bytes stay in the `raw` of whatever carried them, so nothing is lost by
/// answering `None` here.
fn money(field: Option<&RawValue>, currency: Option<Currency>) -> Option<Money> {
    wire::decimal(field?, currency?)
}

/// Reads the answer to anything that reports only whether it worked.
fn acknowledged(response: wire::Ack, fallback: &str) -> Result<(), Error> {
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

/// Refuses an identifier that would change the path rather than sit in it.
///
/// The path is what gets signed as well as what gets requested, so a tracking
/// id carrying `/` or `?` would sign one endpoint and call another. Only the
/// unreserved characters of RFC 3986 are let through — iyzico's tracking ids
/// and reference codes are hyphenated hex, and anything else did not come from
/// them.
fn path_segment(value: &str) -> Result<&str, Error> {
    let unreserved = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
    if value.is_empty() || !value.chars().all(unreserved) {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "a mass payout identifier goes into the signed request path, so it may \
             hold only letters, digits and -._~",
        ));
    }
    Ok(value)
}

/// Refuses an empty identifier that travels in a body rather than a path.
fn required_id(value: &str) -> Result<&str, Error> {
    if value.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            "a mass payout is authorized by its requestId, and none was given",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use kasapay_core::{Currency, Money};
    use serde_json::value::RawValue;

    use super::{ItemStatus, PayoutStatus, path_segment, required_id};
    use crate::mass::wire;

    fn raw(text: &str) -> Box<RawValue> {
        RawValue::from_string(text.to_owned()).expect("valid JSON")
    }

    #[test]
    fn an_identifier_that_would_change_the_path_is_refused() {
        assert!(path_segment("79cdefc1-3ce6-4f51-bdd9-0c050274db64").is_ok());
        assert!(path_segment("").is_err());
        // Each of these signs one path and calls another.
        for hostile in ["../auth", "id/../..", "id?locale=en", "a b", "a#b"] {
            assert!(path_segment(hostile).is_err(), "{hostile} was let through");
        }
        assert!(required_id("79cdefc1").is_ok());
        assert!(required_id("  ").is_err());
    }

    #[test]
    fn trailing_zeros_beyond_the_minor_unit_are_dropped_and_nothing_else_is() {
        let lira = |text: &str| wire::decimal(&raw(text), Currency::Try);
        // iyzico's own example writes a commission this way.
        assert_eq!(
            lira("20.00000000"),
            Some(Money::parse("20.00", Currency::Try).expect("valid"))
        );
        assert_eq!(
            lira("100.50"),
            Some(Money::parse("100.50", Currency::Try).expect("valid"))
        );
        // Not zeros out there: a number that is not an amount in lira, and it
        // is answered as such rather than rounded into one.
        assert_eq!(lira("20.00000001"), None);
        assert_eq!(lira("20.005"), None);
        // A whole number, and one iyzico quoted.
        assert_eq!(
            lira("100"),
            Some(Money::parse("100.00", Currency::Try).expect("valid"))
        );
        assert_eq!(
            wire::decimal(&raw("\"100.50\""), Currency::Try),
            Some(Money::parse("100.50", Currency::Try).expect("valid"))
        );
        // Yen has no minor unit at all, so the zeros are all there is to drop.
        assert_eq!(
            wire::decimal(&raw("20.0000"), Currency::Jpy),
            Some(Money::parse("20", Currency::Jpy).expect("valid"))
        );
    }

    #[test]
    fn the_words_iyzico_uses_round_trip_and_the_rest_are_kept() {
        for name in [
            "INIT",
            "IN_PROGRESS",
            "PUBLISHED_TO_QUEUE",
            "COMPLETED",
            "FAIL",
            "INSUFFICIENT_BALANCE",
            "CANCELED",
        ] {
            assert_eq!(PayoutStatus::from(name).to_string(), name);
        }
        for name in [
            "FAILED",
            "SUCCESS",
            "INIT",
            "INVALID",
            "PROCESSING",
            "MASS_PAYOUT_CANCELED",
            "QUEUED",
            "DEPOSIT_SUCCESS",
            "DEPOSIT_FAIL",
        ] {
            assert_eq!(ItemStatus::from(name).to_string(), name);
        }
        assert_eq!(
            PayoutStatus::from("REFUNDED"),
            PayoutStatus::Other("REFUNDED".into())
        );
        assert_eq!(ItemStatus::from("HELD").to_string(), "HELD");
    }
}
