//! iyzico's reporting service — a payment's status, fraud result and
//! settlement read back after the fact, rather than at the moment it was made.
//!
//! # What is here
//!
//! Both operations iyzico documents:
//!
//! | | |
//! |---|---|
//! | [`Client::payment_details`] | `GET /v2/reporting/payment/details` |
//! | [`Client::daily_transactions`] | `GET /v2/reporting/payment/transactions` |
//!
//! # Why these were not here already
//!
//! They answer the same three questions [`crate::classic`] already answers
//! about a payment it just made — did it succeed, did fraud review hold it up,
//! has it settled — and the risk in writing them was never the HTTP calls, it
//! was a second, slightly different opinion about what iyzico's own statuses
//! mean quietly drifting from the first one. [`PaymentDetail::fraud_status`]
//! is read with `classic`'s own `fraudStatus` mapping rather than a rewritten
//! copy of it — see that field's documentation for the mapping itself.
//!
//! `paymentStatus` could not get the same treatment, and says why it could
//! not on [`PaymentStatus`] rather than silently forking the mapping to make
//! it fit.
//!
//! # No worked example
//!
//! iyzico's reporting page documents every field's shape and, for the coded
//! ones, every value — but not one example request or response body, in
//! either language. [`crate::softpos`] was implemented the same way for the
//! same reason; see its module documentation for why that is a reason to
//! implement carefully rather than a reason not to. Nothing here is a fixture
//! invented to stand in for one: every field is read from iyzico's schema,
//! and every test is about a mapping this crate controls — a code value
//! round-tripping, a query string being built correctly — not about bytes
//! iyzico was never shown to actually send.
//!
//! # Currency
//!
//! `TRY`, `USD`, `EUR`, `GBP`, `CHF` and `NOK` — the same six as
//! [`crate::classic`]'s payments and card storage, per `specs/README.md`'s
//! table. Reading is the permissive direction, as
//! everywhere else in this crate: an amount is `None` only when its currency
//! is absent or is not one of these six, or when the digits beside it are not
//! an amount in it, and the bytes stay in `raw` either way.
//!
//! # Not separately signed
//!
//! The request carries the same [`IYZWSv2`](crate::Credentials) signing every
//! classic-API call does. Neither response schema documents a `signature`
//! field to check afterwards — [`crate::iyzilink`] and [`crate::mass`] answer
//! the same way about their own seven and six.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_iyzico::{Credentials, classic, reporting};
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
//!     "api-key",
//!     "secret-key",
//! )))?;
//! let reports = reporting::Client::new(iyzipay);
//!
//! let query = reporting::PaymentQuery::Id("24603222".into());
//! for payment in reports.payment_details(&query).await? {
//!     println!("{:?} {:?}", payment.payment_id, payment.payment_status);
//! }
//! # Ok(())
//! # }
//! ```

mod client;
mod wire;

#[doc(inline)]
pub use crate::reporting::client::{
    Cancel, Client, ConvertedPayout, DailyTransactionItem, DailyTransactions, ItemTransaction,
    PaymentDetail, PaymentQuery, PaymentStatus, Refund, RefundStatus, TransactionApprovalStatus,
    TransactionType,
};
