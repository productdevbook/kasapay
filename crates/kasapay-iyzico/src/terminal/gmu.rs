//! VUK 507 — the Terminal API's other integration, where a sale is a receipt.
//!
//! [`crate::terminal`] speaks VUK 509: a payment carries an amount, and what
//! is being sold is the caller's own business. VUK 507 is the model where the
//! device issues the fiscal document, so the sale carries the **lines** — each
//! with its unit code, VAT group and prices — the document type, and the
//! buyer's tax details when the buyer is not a consumer.
//!
//! A merchant chooses one integration or the other with iyzico. This is not a
//! fallback within VUK 509 and nothing here mixes with it: the paths differ,
//! the request differs, and a `paymentId` from one is not one the other knows.
//!
//! # What is here
//!
//! All nine operations iyzico documents under `gmu`:
//!
//! | | |
//! |---|---|
//! | [`Client::pay`] | `POST /v2/terminal-host/gmu/payment` |
//! | [`Client::payment`] | `POST /v2/terminal-host/gmu/payment/query-transaction-status` |
//! | [`Client::refund`] | `POST /v2/terminal-host/gmu/payment/refund` |
//! | [`Client::void`] | `POST /v2/terminal-host/gmu/payment/void` |
//! | [`Client::refundable_sale`] | `POST /v2/terminal-host/gmu/payment/refundable-sale-info` |
//! | [`Client::end_of_day`] | `POST /v2/terminal-host/gmu/eod` |
//! | [`Client::start_partial_payment`], [`Client::add_partial_payment`], [`Client::complete_partial_payment`] | one sale settled by several instruments |
//!
//! # A refund names the lines, not just the amount
//!
//! Which is the whole difference. [`Client::refundable_sale`] answers what is
//! still returnable — per line, with a quantity and an amount — and a refund
//! carries the lines being returned. Refunding "fifty lira" of a receipt is
//! not a thing this model has: two of three items came back, and the document
//! has to say which two.
//!
//! # Partial payment holds a sale open
//!
//! [`Client::start_partial_payment`] answers a `saleNumber` and what is left
//! to pay. Each [`Client::add_partial_payment`] settles part of it with one
//! instrument, and [`Client::complete_partial_payment`] closes the sale.
//! **A sale left open is a sale nobody has been charged for and the device is
//! still holding** — the completion is not optional bookkeeping.
//!
//! # Nothing here is checked against a live till
//!
//! The same as the rest of [`crate::terminal`], and for the same reason: there
//! is no Terminal API sandbox without a merchant agreement and a Pavo device.
//! Every shape here is what iyzico documents. Their VUK 507 pages are Turkish
//! only, which the dated index records.

use kasapay_core::{Currency, Error, ErrorKind, Money};

use crate::terminal::client::{Client as Terminal, EndOfDay, EndOfDayRequest};
use crate::terminal::request::{Reference, is_payment_date};
use crate::terminal::{PROVIDER, wire};

/// Talks VUK 507 over a Terminal API client.
///
/// Built over [`terminal::Client`](crate::terminal::Client) because that is
/// what this is: the same host, the same bearer token, the same failure
/// envelope. Cloning shares the one connection pool and the one token.
#[derive(Debug, Clone)]
pub struct Client {
    terminal: Terminal,
}

impl Client {
    /// Speaks VUK 507 over a Terminal API client.
    #[must_use]
    pub const fn new(terminal: Terminal) -> Self {
        Self { terminal }
    }

    /// The client underneath, for the VUK 509 operations.
    #[must_use]
    pub const fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// Takes a payment, and issues the fiscal document for it.
    ///
    /// `POST /v2/terminal-host/gmu/payment`. Like VUK 509's own sale, this
    /// returns when the payer has presented a card and, if the bank asks,
    /// typed a PIN — so the client's ninety-second timeout is what bounds it
    /// rather than a socket.
    pub async fn pay(&self, sale: &Sale) -> Result<Payment, Error> {
        let body = sale.body(self.terminal.locale())?;
        self.call("v2/terminal-host/gmu/payment", &body).await
    }

    /// Reads a payment back.
    ///
    /// `POST /v2/terminal-host/gmu/payment/query-transaction-status`. iyzico
    /// marks every field of this request optional and means it: the payment's
    /// own id, the device, or the reference the sale was sent with will each
    /// find it. Naming none of the three is refused here rather than sent.
    pub async fn payment(&self, query: &Query) -> Result<Payment, Error> {
        if query.payment_id.is_none()
            && query.device_unique_id.is_none()
            && query.transaction_reference_id.is_none()
        {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a query names the payment, the device or the sale's own reference, \
                 and this named none of them",
            ));
        }
        let body = wire::GmuQueryRequest {
            locale: self.terminal.locale(),
            conversation_id: query.conversation_id.as_deref(),
            payment_id: query.payment_id.as_deref(),
            device_unique_id: query.device_unique_id.as_deref(),
            transaction_reference_id: query.transaction_reference_id.as_deref(),
        };
        self.call(
            "v2/terminal-host/gmu/payment/query-transaction-status",
            &body,
        )
        .await
    }

    /// Gives money back, line by line.
    ///
    /// `POST /v2/terminal-host/gmu/payment/refund`. The lines are the point:
    /// this model's document says what was returned, not only how much.
    /// [`Client::refundable_sale`] is what says which lines still can be.
    pub async fn refund(&self, refund: &Refund) -> Result<Payment, Error> {
        // The first line is what the rest are read against: a refund request
        // carries no currency field, so there is nothing on the wire for a
        // line to disagree with. iyzico reads them all as one currency, so two
        // that differ is a document that cannot be right whichever it means.
        let Some(first) = refund.items.first() else {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a VUK 507 refund names the lines being returned, and this named none",
            ));
        };
        posting_date(&refund.payment_date)?;
        let currency = first.unit_price.currency();
        let body = wire::GmuRefundRequest {
            locale: self.terminal.locale(),
            conversation_id: Some(&refund.reference.conversation_id),
            device_unique_id: &refund.reference.device_unique_id,
            transaction_reference_id: &refund.reference.transaction_reference_id,
            payment_id: &refund.payment_id,
            payment_date: &refund.payment_date,
            sale_app_name: &refund.sale_app.name,
            sale_app_version: &refund.sale_app.version,
            notification_phone: refund.notify_phone.as_deref(),
            notification_email: refund.notify_email.as_deref(),
            sale_items: refund
                .items
                .iter()
                .map(|item| item.body(currency))
                .collect::<Result<Vec<_>, Error>>()?,
        };
        self.call("v2/terminal-host/gmu/payment/refund", &body)
            .await
    }

    /// Withdraws a payment before the day's batch closes.
    ///
    /// `POST /v2/terminal-host/gmu/payment/void`. After the batch, the way
    /// back is [`Client::refund`] — and a refund here names lines where a void
    /// takes the whole sale.
    pub async fn void(&self, void: &Void) -> Result<Payment, Error> {
        posting_date(&void.payment_date)?;
        let body = wire::GmuVoidRequest {
            locale: self.terminal.locale(),
            conversation_id: Some(&void.reference.conversation_id),
            device_unique_id: &void.reference.device_unique_id,
            transaction_reference_id: &void.reference.transaction_reference_id,
            payment_id: &void.payment_id,
            payment_date: &void.payment_date,
            reason: void.reason.as_deref(),
            description: void.description.as_deref(),
        };
        self.call("v2/terminal-host/gmu/payment/void", &body).await
    }

    /// Asks what of a sale can still be given back.
    ///
    /// `POST /v2/terminal-host/gmu/payment/refundable-sale-info`, by the
    /// reference the original sale was sent with. The answer carries the
    /// `saleUid` a refund quotes and, per line, what quantity and amount are
    /// still returnable — so a second partial refund cannot exceed what a
    /// first one left.
    pub async fn refundable_sale(
        &self,
        reference: &str,
        device_unique_id: Option<&str>,
        conversation_id: Option<&str>,
    ) -> Result<RefundableSale, Error> {
        let body = wire::GmuRefundableSaleRequest {
            locale: self.terminal.locale(),
            conversation_id,
            device_unique_id,
            transaction_reference_id: reference,
        };
        let (response, raw) = self
            .terminal
            .post::<wire::GmuRefundableSaleResponse>(
                "v2/terminal-host/gmu/payment/refundable-sale-info",
                &body,
            )
            .await?;
        Ok(RefundableSale {
            sale_number: response.sale_number.map(String::into_boxed_str),
            sale_uid: response.sale_uid.map(String::into_boxed_str),
            total_returnable: response.total_returnable_amount.map(String::into_boxed_str),
            currency: response.currency.map(String::into_boxed_str),
            raw,
        })
    }

    /// Closes the day's batch for this integration.
    ///
    /// `POST /v2/terminal-host/gmu/eod`. The same request and the same answer
    /// as VUK 509's own end of day —
    /// [`terminal::Client::end_of_day`](crate::terminal::Client::end_of_day) —
    /// at the path this integration uses. A device runs one or the other, not
    /// both.
    pub async fn end_of_day(&self, request: &EndOfDayRequest) -> Result<EndOfDay, Error> {
        let body = wire::EndOfDayRequest {
            conversation_id: &request.conversation_id,
            locale: self.terminal.locale(),
            device_unique_id: &request.device_unique_id,
            use_summary: request.summary_on_slip,
        };
        let (response, raw) = self
            .terminal
            .post::<wire::EndOfDayResponse>("v2/terminal-host/gmu/eod", &body)
            .await?;
        Ok(EndOfDay::read(response, raw))
    }

    /// Opens a sale that will be paid with more than one instrument.
    ///
    /// `POST /v2/terminal-host/gmu/partial-payment/start`. The same [`Sale`] a
    /// whole payment takes — the lines, the document type, the buyer — and
    /// what comes back is a [`PartialPayment`] carrying the `saleNumber` the
    /// other two steps quote and what is left to pay.
    ///
    /// **The sale is open until [`Client::complete_partial_payment`].** Until
    /// then nobody has been charged for it and the device is holding it.
    pub async fn start_partial_payment(&self, sale: &Sale) -> Result<PartialPayment, Error> {
        let body = sale.body(self.terminal.locale())?;
        let (response, raw) = self
            .terminal
            .post::<wire::GmuPartialPaymentResponse>(
                "v2/terminal-host/gmu/partial-payment/start",
                &body,
            )
            .await?;
        Ok(PartialPayment::read(response, raw))
    }

    /// Settles part of an open sale with one instrument.
    ///
    /// `POST /v2/terminal-host/gmu/partial-payment/add-payment`. `amount` must
    /// not exceed what [`PartialPayment::remaining`] said was left; iyzico
    /// refuses one that does.
    pub async fn add_partial_payment(
        &self,
        sale_number: &str,
        reference: &Reference,
        amount: Money,
        installments: u8,
    ) -> Result<PartialPayment, Error> {
        let amount = amount.require_positive().map_err(|e| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a part payment takes an amount above zero",
            )
            .with_source(e)
        })?;
        let body = wire::GmuPartialAddRequest {
            locale: self.terminal.locale(),
            conversation_id: Some(&reference.conversation_id),
            device_unique_id: Some(&reference.device_unique_id),
            transaction_reference_id: Some(&reference.transaction_reference_id),
            sale_number,
            price: amount.to_decimal_string(),
            installment: installments,
            currency: currency_code(amount.currency())?,
        };
        let (response, raw) = self
            .terminal
            .post::<wire::GmuPartialPaymentResponse>(
                "v2/terminal-host/gmu/partial-payment/add-payment",
                &body,
            )
            .await?;
        Ok(PartialPayment::read(response, raw))
    }

    /// Closes an open sale once it has been paid for.
    ///
    /// `POST /v2/terminal-host/gmu/partial-payment/complete`. This is what
    /// issues the document; a sale that is never completed is one the device
    /// is still holding.
    pub async fn complete_partial_payment(
        &self,
        sale_number: &str,
        reference: &Reference,
    ) -> Result<Payment, Error> {
        let body = wire::GmuPartialCompleteRequest {
            locale: self.terminal.locale(),
            conversation_id: Some(&reference.conversation_id),
            device_unique_id: &reference.device_unique_id,
            transaction_reference_id: &reference.transaction_reference_id,
            sale_number,
        };
        self.call("v2/terminal-host/gmu/partial-payment/complete", &body)
            .await
    }

    /// Every VUK 507 call that answers a payment.
    async fn call<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<Payment, Error> {
        let (response, raw) = self
            .terminal
            .post::<wire::GmuPaymentResponse>(path, body)
            .await?;
        Ok(Payment::read(response, raw))
    }
}

impl From<Terminal> for Client {
    fn from(terminal: Terminal) -> Self {
        Self::new(terminal)
    }
}

/// The sale application, which iyzico requires on every VUK 507 sale.
///
/// Its own name and version, recorded on the fiscal document: the till is
/// identifying the software that produced the receipt, not the merchant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaleApp {
    /// What the sale application is called.
    pub name: Box<str>,
    /// Which version of it.
    pub version: Box<str>,
}

impl SaleApp {
    /// Names the sale application and its version.
    #[must_use]
    pub fn new(name: impl Into<Box<str>>, version: impl Into<Box<str>>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// What kind of document the device issues for a sale.
///
/// iyzico documents two values and nothing between them: `1` is an e-invoice
/// and `9` is a `gider pusulası`, the document a business issues when buying
/// from somebody who cannot invoice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DocumentType {
    /// An e-invoice — iyzico's `1`. The default.
    #[default]
    Invoice,
    /// A `gider pusulası` — iyzico's `9`.
    ExpenseVoucher,
}

impl DocumentType {
    /// The code iyzico expects on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Invoice => 1,
            Self::ExpenseVoucher => 9,
        }
    }
}

/// One line of a sale, as the fiscal document carries it.
///
/// Every amount is a [`Money`] here and a decimal string on the wire, which is
/// how iyzico types them for this integration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SaleItem {
    /// What is being sold.
    pub name: Box<str>,
    /// The unit iyzico measures it in — `C62` is "each".
    pub unit_code: Box<str>,
    /// The VAT group the line falls in.
    pub tax_group_code: Box<str>,
    /// How many.
    pub quantity: i64,
    /// What one costs.
    pub unit_price: Money,
    /// What the line comes to before tax.
    pub gross_price: Money,
    /// What the line comes to altogether.
    pub total_price: Money,
    /// The line of the original sale this one returns, on a refund.
    ///
    /// iyzico's `relatedSaleItemId`, which
    /// [`Client::refundable_sale`] answers. `None` on a sale.
    pub returns: Option<Box<str>>,
    /// How much of it is being returned, on a refund.
    pub return_amount: Option<Money>,
    /// Whether iyzico should treat this as a generic line rather than a named
    /// product.
    pub generic: bool,
}

impl SaleItem {
    /// One line, with the four figures iyzico requires.
    #[must_use]
    pub fn new(
        name: impl Into<Box<str>>,
        unit_code: impl Into<Box<str>>,
        tax_group_code: impl Into<Box<str>>,
        quantity: i64,
        unit_price: Money,
        gross_price: Money,
        total_price: Money,
    ) -> Self {
        Self {
            name: name.into(),
            unit_code: unit_code.into(),
            tax_group_code: tax_group_code.into(),
            quantity,
            unit_price,
            gross_price,
            total_price,
            returns: None,
            return_amount: None,
            generic: false,
        }
    }

    /// Marks this line as returning part of an earlier sale's line.
    #[must_use]
    pub fn returning(mut self, sale_item_id: impl Into<Box<str>>, amount: Money) -> Self {
        self.returns = Some(sale_item_id.into());
        self.return_amount = Some(amount);
        self
    }

    /// Marks the line generic.
    #[must_use]
    pub const fn generic(mut self) -> Self {
        self.generic = true;
        self
    }

    /// The line as iyzico wants it, in the currency the document is in.
    ///
    /// Fallible because the request names one currency for the whole document:
    /// iyzico reads every amount on it as being in that one, so a line
    /// denominated in another is not a rejected request but a different
    /// number. A `Money` whose exponent differs is that number out by a
    /// factor.
    ///
    /// `expected` rather than a membership test, because the Terminal API
    /// settles in three currencies and a euro line on a lira sale is inside
    /// that set.
    fn body(&self, expected: Currency) -> Result<wire::GmuSaleItem<'_>, Error> {
        for amount in [
            Some(self.unit_price),
            Some(self.gross_price),
            Some(self.total_price),
            self.return_amount,
        ]
        .into_iter()
        .flatten()
        {
            in_the_documents_currency(amount, expected)?;
        }
        if let Some(returning) = self.return_amount {
            returning.require_positive().map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    PROVIDER,
                    "a returned line gives back an amount above zero",
                )
                .with_source(e)
            })?;
        }
        Ok(wire::GmuSaleItem {
            name: &self.name,
            generic: self.generic,
            unit_code: &self.unit_code,
            tax_group_code: &self.tax_group_code,
            item_quantity: self.quantity,
            unit_price_amount: self.unit_price.to_decimal_string(),
            gross_price_amount: self.gross_price.to_decimal_string(),
            total_price_amount: self.total_price.to_decimal_string(),
            related_sale_item_id: self.returns.as_deref(),
            return_amount: self.return_amount.map(Money::to_decimal_string),
        })
    }
}

/// Who the buyer is, where they are not a consumer.
///
/// iyzico's `buyerInfo` is required exactly when the buyer is a business: the
/// document has to carry their tax office and number. A sale to a consumer
/// leaves it out.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Buyer {
    /// Individual or company — iyzico's `customerType`, `1` or `2`.
    pub kind: BuyerKind,
    /// Given name.
    pub first_name: Option<Box<str>>,
    /// Middle name.
    pub middle_name: Option<Box<str>>,
    /// Family name.
    pub family_name: Option<Box<str>>,
    /// The company's registered name.
    pub company_name: Option<Box<str>>,
    /// The tax office they are registered with.
    pub tax_office_code: Option<Box<str>>,
    /// Their tax number, or a national identity number for an individual.
    pub tax_number: Option<Box<str>>,
    /// Country.
    pub country: Option<Box<str>>,
    /// City.
    pub city: Option<Box<str>>,
    /// District.
    pub district: Option<Box<str>>,
}

impl Buyer {
    /// A buyer of one kind, with nothing said about them yet.
    #[must_use]
    pub const fn new(kind: BuyerKind) -> Self {
        Self {
            kind,
            first_name: None,
            middle_name: None,
            family_name: None,
            company_name: None,
            tax_office_code: None,
            tax_number: None,
            country: None,
            city: None,
            district: None,
        }
    }

    /// Names a person.
    #[must_use]
    pub fn named(
        mut self,
        first_name: impl Into<Box<str>>,
        family_name: impl Into<Box<str>>,
    ) -> Self {
        self.first_name = Some(first_name.into());
        self.family_name = Some(family_name.into());
        self
    }

    /// Names a company.
    #[must_use]
    pub fn company(mut self, name: impl Into<Box<str>>) -> Self {
        self.company_name = Some(name.into());
        self
    }

    /// Sets the tax office and number the document carries.
    #[must_use]
    pub fn tax(mut self, office_code: impl Into<Box<str>>, number: impl Into<Box<str>>) -> Self {
        self.tax_office_code = Some(office_code.into());
        self.tax_number = Some(number.into());
        self
    }

    /// Sets where they are.
    #[must_use]
    pub fn at(
        mut self,
        country: impl Into<Box<str>>,
        city: impl Into<Box<str>>,
        district: impl Into<Box<str>>,
    ) -> Self {
        self.country = Some(country.into());
        self.city = Some(city.into());
        self.district = Some(district.into());
        self
    }

    fn body(&self) -> wire::GmuBuyer<'_> {
        wire::GmuBuyer {
            customer_type: self.kind.code(),
            first_name: self.first_name.as_deref(),
            middle_name: self.middle_name.as_deref(),
            family_name: self.family_name.as_deref(),
            company_name: self.company_name.as_deref(),
            tax_office_code: self.tax_office_code.as_deref(),
            tax_number: self.tax_number.as_deref(),
            country: self.country.as_deref(),
            city: self.city.as_deref(),
            district: self.district.as_deref(),
        }
    }
}

/// Whether the buyer is a person or a business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuyerKind {
    /// iyzico's `1`.
    Individual,
    /// iyzico's `2`.
    Company,
}

impl BuyerKind {
    /// The code iyzico expects on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Individual => 1,
            Self::Company => 2,
        }
    }
}

/// A VUK 507 sale: what is being paid for, line by line.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Sale {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// What the goods come to.
    pub price: Money,
    /// What the payer is charged, which an instalment surcharge can raise.
    pub paid_price: Money,
    /// How iyzico is being paid — their `paymentType`, e.g. card or cash.
    pub payment_type: Box<str>,
    /// The software issuing the document.
    pub sale_app: SaleApp,
    /// Which document the device issues.
    pub document_type: DocumentType,
    /// How many instalments. One is a single payment.
    pub installments: u8,
    /// The lines. iyzico requires at least one.
    pub items: Vec<SaleItem>,
    /// The buyer, where they are not a consumer.
    pub buyer: Option<Buyer>,
    /// A number to send the receipt to.
    pub notify_phone: Option<Box<str>>,
    /// An address to send it to.
    pub notify_email: Option<Box<str>>,
}

impl Sale {
    /// Starts building a sale.
    #[must_use]
    pub fn builder(
        reference: Reference,
        price: Money,
        payment_type: impl Into<Box<str>>,
        sale_app: SaleApp,
    ) -> SaleBuilder {
        SaleBuilder {
            sale: Self {
                reference,
                price,
                paid_price: price,
                payment_type: payment_type.into(),
                sale_app,
                document_type: DocumentType::Invoice,
                installments: 1,
                items: Vec::new(),
                buyer: None,
                notify_phone: None,
                notify_email: None,
            },
        }
    }

    fn body<'a>(&'a self, locale: &'a str) -> Result<wire::GmuPaymentRequest<'a>, Error> {
        if self.items.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "a VUK 507 sale is made of lines, and this one has none",
            ));
        }
        // `paidPrice` is iyzico's "Ödenecek nihai tutar" — the figure the payer
        // is actually charged, which `SaleBuilder::paid_price` exists to make
        // larger than the basket. It is the one amount on this document that
        // must be right, and it was the one nothing checked.
        for amount in [self.price, self.paid_price] {
            amount.require_positive().map_err(|e| {
                Error::new(
                    ErrorKind::InvalidRequest,
                    PROVIDER,
                    "a sale takes an amount above zero",
                )
                .with_source(e)
            })?;
        }
        let currency = self.price.currency();
        in_the_documents_currency(self.paid_price, currency)?;
        Ok(wire::GmuPaymentRequest {
            locale,
            conversation_id: Some(&self.reference.conversation_id),
            device_unique_id: &self.reference.device_unique_id,
            transaction_reference_id: &self.reference.transaction_reference_id,
            price: self.price.to_decimal_string(),
            paid_price: self.paid_price.to_decimal_string(),
            payment_type: &self.payment_type,
            currency: currency_code(self.price.currency())?,
            installment: self.installments,
            sale_app_name: &self.sale_app.name,
            sale_app_version: &self.sale_app.version,
            sale_document_type: self.document_type.code(),
            notification_phone: self.notify_phone.as_deref(),
            notification_email: self.notify_email.as_deref(),
            sale_items: self
                .items
                .iter()
                .map(|item| item.body(currency))
                .collect::<Result<Vec<_>, Error>>()?,
            buyer_info: self.buyer.as_ref().map(Buyer::body),
        })
    }
}

/// Collects the parts of a [`Sale`].
#[derive(Debug, Clone)]
pub struct SaleBuilder {
    sale: Sale,
}

impl SaleBuilder {
    /// Adds a line.
    #[must_use]
    pub fn item(mut self, item: SaleItem) -> Self {
        self.sale.items.push(item);
        self
    }

    /// Charges more than the goods came to — an instalment surcharge.
    #[must_use]
    pub const fn paid_price(mut self, paid_price: Money) -> Self {
        self.sale.paid_price = paid_price;
        self
    }

    /// Splits the payment over instalments.
    #[must_use]
    pub const fn installments(mut self, installments: u8) -> Self {
        self.sale.installments = installments;
        self
    }

    /// Issues a document other than an e-invoice.
    #[must_use]
    pub const fn document_type(mut self, document_type: DocumentType) -> Self {
        self.sale.document_type = document_type;
        self
    }

    /// Names the buyer, which iyzico requires when they are not a consumer.
    #[must_use]
    pub fn buyer(mut self, buyer: Buyer) -> Self {
        self.sale.buyer = Some(buyer);
        self
    }

    /// Sends the receipt somewhere.
    #[must_use]
    pub fn notify(
        mut self,
        phone: Option<impl Into<Box<str>>>,
        email: Option<impl Into<Box<str>>>,
    ) -> Self {
        self.sale.notify_phone = phone.map(Into::into);
        self.sale.notify_email = email.map(Into::into);
        self
    }

    /// Produces the sale.
    ///
    /// No `Result`: a sale with no lines is refused where it is sent, because
    /// that is where the same check has to happen for a refund too.
    #[must_use]
    pub fn build(self) -> Sale {
        self.sale
    }
}

/// Giving part of a VUK 507 sale back.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Refund {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// The payment being refunded.
    pub payment_id: Box<str>,
    /// The day it was posted, `YYYYMMDD`, as iyzico wrote it.
    pub payment_date: Box<str>,
    /// The software issuing the document.
    pub sale_app: SaleApp,
    /// The lines being returned, each naming what it returns.
    pub items: Vec<SaleItem>,
    /// A number to send the receipt to.
    pub notify_phone: Option<Box<str>>,
    /// An address to send it to.
    pub notify_email: Option<Box<str>>,
}

impl Refund {
    /// Names a refund of one payment.
    #[must_use]
    pub fn new(
        reference: Reference,
        payment_id: impl Into<Box<str>>,
        payment_date: impl Into<Box<str>>,
        sale_app: SaleApp,
        items: Vec<SaleItem>,
    ) -> Self {
        Self {
            reference,
            payment_id: payment_id.into(),
            payment_date: payment_date.into(),
            sale_app,
            items,
            notify_phone: None,
            notify_email: None,
        }
    }
}

/// Withdrawing a VUK 507 sale before the batch closes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Void {
    /// Who is asking, of which device, for which attempt.
    pub reference: Reference,
    /// The payment being withdrawn.
    pub payment_id: Box<str>,
    /// The day it was posted, `YYYYMMDD`.
    pub payment_date: Box<str>,
    /// Why, in iyzico's own field.
    pub reason: Option<Box<str>>,
    /// Anything more to say about it.
    pub description: Option<Box<str>>,
}

impl Void {
    /// Names a void of one payment.
    #[must_use]
    pub fn new(
        reference: Reference,
        payment_id: impl Into<Box<str>>,
        payment_date: impl Into<Box<str>>,
    ) -> Self {
        Self {
            reference,
            payment_id: payment_id.into(),
            payment_date: payment_date.into(),
            reason: None,
            description: None,
        }
    }

    /// Says why.
    #[must_use]
    pub fn because(
        mut self,
        reason: impl Into<Box<str>>,
        description: Option<impl Into<Box<str>>>,
    ) -> Self {
        self.reason = Some(reason.into());
        self.description = description.map(Into::into);
        self
    }
}

/// Which payment a query is about.
///
/// Every field is optional in iyzico's own schema, and naming none of them is
/// refused by [`Client::payment`] rather than sent.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Query {
    /// The caller's id for this request.
    pub conversation_id: Option<Box<str>>,
    /// iyzico's own id for the payment.
    pub payment_id: Option<Box<str>>,
    /// The device it happened on.
    pub device_unique_id: Option<Box<str>>,
    /// The reference the sale was sent with.
    pub transaction_reference_id: Option<Box<str>>,
}

impl Query {
    /// Asks about one payment by iyzico's own id for it.
    #[must_use]
    pub fn by_payment(payment_id: impl Into<Box<str>>) -> Self {
        Self {
            payment_id: Some(payment_id.into()),
            ..Self::default()
        }
    }

    /// Asks about one by the reference the sale was sent with.
    #[must_use]
    pub fn by_reference(transaction_reference_id: impl Into<Box<str>>) -> Self {
        Self {
            transaction_reference_id: Some(transaction_reference_id.into()),
            ..Self::default()
        }
    }

    /// Narrows the question to one device.
    #[must_use]
    pub fn on_device(mut self, device_unique_id: impl Into<Box<str>>) -> Self {
        self.device_unique_id = Some(device_unique_id.into());
        self
    }
}

/// One VUK 507 transaction, as every operation here reports it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Payment {
    /// iyzico's own id for the payment. What a refund and a void name it by.
    pub payment_id: Option<Box<str>>,
    /// The day it was posted, `YYYYMMDD`, as iyzico wrote it.
    pub payment_date: Option<Box<str>>,
    /// What moved, as iyzico wrote it.
    ///
    /// Text rather than [`Money`]: iyzico types it as a decimal string here
    /// and the currency is a separate field that a query does not always
    /// carry, so pairing the two would sometimes mean inventing one.
    pub price: Option<Box<str>>,
    /// The currency, where iyzico named one.
    pub currency: Option<Box<str>>,
    /// The sale a partial payment belongs to.
    pub sale_number: Option<Box<str>>,
    /// The bank's approval code.
    pub auth_code: Option<Box<str>>,
    /// The batch it will settle in.
    pub batch_no: Option<Box<str>>,
    /// The last four digits of the card.
    pub last_four_digits: Option<Box<str>>,
    /// iyzico's own answer, untouched. The lines are in here.
    pub raw: kasapay_core::Raw,
}

impl Payment {
    fn read(response: wire::GmuPaymentResponse, raw: kasapay_core::Raw) -> Self {
        Self {
            payment_id: response.payment_id.map(String::into_boxed_str),
            payment_date: response.payment_date.map(|date| date.to_string().into()),
            price: response.price.map(String::into_boxed_str),
            currency: response.currency.map(String::into_boxed_str),
            sale_number: response.sale_number.map(String::into_boxed_str),
            auth_code: response.auth_code.map(String::into_boxed_str),
            batch_no: response.batch_no.map(String::into_boxed_str),
            last_four_digits: response.last_four_digits.map(String::into_boxed_str),
            raw,
        }
    }
}

/// A sale being paid for with more than one instrument.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PartialPayment {
    /// What the other two steps quote.
    pub sale_number: Option<Box<str>>,
    /// What is left to pay, as iyzico wrote it.
    pub remaining: Option<Box<str>>,
    /// iyzico's own answer, untouched.
    pub raw: kasapay_core::Raw,
}

impl PartialPayment {
    fn read(response: wire::GmuPartialPaymentResponse, raw: kasapay_core::Raw) -> Self {
        Self {
            sale_number: response.sale_number.map(String::into_boxed_str),
            remaining: response
                .remaining_payment_amount
                .map(String::into_boxed_str),
            raw,
        }
    }
}

/// What of a sale can still be given back.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RefundableSale {
    /// The sale's number.
    pub sale_number: Option<Box<str>>,
    /// The id a refund quotes as `relatedSaleId`.
    pub sale_uid: Option<Box<str>>,
    /// What is still returnable altogether, as iyzico wrote it.
    pub total_returnable: Option<Box<str>>,
    /// The currency it is in.
    pub currency: Option<Box<str>>,
    /// iyzico's own answer, untouched. The per-line returnable quantities and
    /// amounts are in here.
    pub raw: kasapay_core::Raw,
}

/// Refuses an amount that is not in the currency the document is in.
///
/// `currency_code` answers whether the Terminal API settles in a currency at
/// all. That is not the question a document asks: it names **one** currency and
/// iyzico reads every figure on it as being in that one, so a euro line on a
/// lira sale passes a membership test and is still a different number.
fn in_the_documents_currency(amount: Money, expected: Currency) -> Result<(), Error> {
    currency_code(expected)?;
    if amount.currency() == expected {
        return Ok(());
    }
    Err(Error::new(
        ErrorKind::Unsupported,
        PROVIDER,
        format!(
            "this document is in {expected} and carries an amount in {}; \
             iyzico reads every figure on it as the first",
            amount.currency()
        ),
    ))
}

/// Refuses a posting date iyzico's own schemas cannot read.
///
/// The 509 path checks this in [`crate::terminal::request`]; the GMU path sends
/// the same field to the same host and did not.
fn posting_date(value: &str) -> Result<(), Error> {
    if is_payment_date(value) {
        Ok(())
    } else {
        Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            format!("a posting date is eight digits, `YYYYMMDD`, and this is `{value}`"),
        ))
    }
}

/// The three currencies this API's own schemas name.
///
/// The same three [`crate::terminal`] allows, and refused here before a socket
/// opens rather than sent to find out.
fn currency_code(currency: Currency) -> Result<&'static str, Error> {
    match currency {
        Currency::Try => Ok("TRY"),
        Currency::Usd => Ok("USD"),
        Currency::Eur => Ok("EUR"),
        other => Err(Error::new(
            ErrorKind::Unsupported,
            PROVIDER,
            format!("the Terminal API's own schemas name TRY, USD and EUR, not {other}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{BuyerKind, DocumentType};

    #[test]
    fn the_two_document_types_iyzico_names_are_one_and_nine() {
        assert_eq!(DocumentType::default(), DocumentType::Invoice);
        assert_eq!(DocumentType::Invoice.code(), 1);
        assert_eq!(DocumentType::ExpenseVoucher.code(), 9);
    }

    #[test]
    fn a_buyer_is_a_person_or_a_business() {
        assert_eq!(BuyerKind::Individual.code(), 1);
        assert_eq!(BuyerKind::Company.code(), 2);
    }
}
