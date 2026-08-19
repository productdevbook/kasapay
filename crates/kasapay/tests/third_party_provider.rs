//! A provider written against nothing but kasapay's public API.
//!
//! The README claims a provider living outside this workspace is a first-class
//! one. Nothing proved it: both bundled adapters can see `kasapay-core` as a
//! direct dependency, so neither would notice if building a `Charge` or an
//! `Error` from outside were impossible. This is that proof, and it is a test
//! rather than a crate so it costs nothing to ship.

#![allow(
    clippy::expect_used,
    reason = "a fixture that cannot be built is a failed test"
)]

use std::sync::Mutex;

use kasapay::{
    Capabilities, Charge, ChargeRequest, Currency, Error, ErrorKind, IdempotencyKey, Instrument,
    Money, NextAction, OrderRef, PaymentId, Provider, ProviderId, Raw, Refund, RefundId,
    RefundRequest, RefundStatus, Status, async_trait,
};

/// A provider that stalls on a redirect and settles when asked a second time.
#[derive(Debug, Default)]
struct Kumbara {
    /// How many times each payment has been asked about.
    seen: Mutex<Vec<String>>,
}

impl Kumbara {
    const ID: ProviderId = ProviderId::new("kumbara");
}

#[async_trait]
impl Provider for Kumbara {
    fn id(&self) -> ProviderId {
        Self::ID
    }

    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        if request.amount.currency() != Currency::Try {
            return Err(Error::new(
                ErrorKind::Unsupported,
                Self::ID,
                format!("kumbara holds lira, not {}", request.amount.currency()),
            )
            .with_code("KMB-CUR"));
        }
        Ok(Charge {
            id: Some(PaymentId::issued(format!("kmb_{}", request.order))),
            order: Some(request.order.clone()),
            amount: request.amount,
            order_amount: None,
            status: Status::RequiresAction,
            next_action: Some(NextAction::Redirect {
                url: "https://kumbara.test/approve".parse().expect("valid url"),
                continuation: Some("kmb-session".into()),
            }),
            provider: Self::ID,
            raw: Raw::from_text(r#"{"provider":"kumbara"}"#),
        })
    }

    async fn charge_status(&self, id: &PaymentId) -> Result<Charge, Error> {
        let mut seen = self.seen.lock().expect("the mutex is not poisoned");
        let settled = seen.iter().any(|s| s == id.as_str());
        seen.push(id.as_str().to_owned());
        Ok(Charge {
            id: Some(id.clone()),
            order: None,
            amount: Money::from_minor_units(1000, Currency::Try),
            order_amount: None,
            status: if settled {
                Status::Captured
            } else {
                Status::Pending
            },
            next_action: None,
            provider: Self::ID,
            raw: Raw::default(),
        })
    }

    async fn capture(
        &self,
        id: &PaymentId,
        amount: Option<Money>,
        _idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        Ok(Charge {
            id: Some(id.clone()),
            order: None,
            amount: amount.unwrap_or_else(|| Money::from_minor_units(1000, Currency::Try)),
            order_amount: None,
            status: Status::Captured,
            next_action: None,
            provider: Self::ID,
            raw: Raw::default(),
        })
    }

    async fn cancel(&self, id: &PaymentId) -> Result<Charge, Error> {
        Ok(Charge {
            id: Some(id.clone()),
            order: None,
            amount: Money::from_minor_units(1000, Currency::Try),
            order_amount: None,
            status: Status::Canceled,
            next_action: None,
            provider: Self::ID,
            raw: Raw::default(),
        })
    }

    async fn refund(&self, request: &RefundRequest) -> Result<Refund, Error> {
        Ok(Refund {
            id: Some(RefundId::issued(format!("kmb_re_{}", request.payment))),
            payment: request.payment.clone(),
            amount: request
                .amount
                .unwrap_or_else(|| Money::from_minor_units(1000, Currency::Try)),
            status: RefundStatus::Succeeded,
            next_action: None,
            provider: Self::ID,
            raw: Raw::default(),
        })
    }

    async fn instruments(&self, _customer: &str) -> Result<Vec<Instrument>, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            Self::ID,
            "kumbara keeps no vault",
        ))
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            separate_capture: true,
            partial_capture: true,
            ..Capabilities::default()
        }
    }
}

fn ten_lira() -> Money {
    Money::parse("10.00", Currency::Try).expect("valid amount")
}

#[tokio::test]
async fn a_provider_outside_the_workspace_can_be_written() {
    let kumbara = Kumbara::default();
    let request = ChargeRequest::builder(OrderRef::new("ord-1"), ten_lira())
        .build()
        .expect("valid request");

    let charge = kumbara.charge(&request).await.expect("kumbara takes lira");
    assert_eq!(charge.provider, Kumbara::ID);
    assert_eq!(charge.provider.as_str(), "kumbara");
    assert_eq!(charge.status, Status::RequiresAction);
    assert!(charge.status.is_open());
    assert!(matches!(
        charge.next_action,
        Some(NextAction::Redirect { .. })
    ));
}

#[tokio::test]
async fn it_reports_its_own_failures_through_the_shared_error() {
    let kumbara = Kumbara::default();
    let request = ChargeRequest::builder(
        OrderRef::new("ord-1"),
        Money::parse("10.00", Currency::Usd).expect("valid amount"),
    )
    .build()
    .expect("valid request");

    let error = kumbara
        .charge(&request)
        .await
        .expect_err("kumbara holds lira only");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert_eq!(error.provider(), Kumbara::ID);
    assert_eq!(error.code(), Some("KMB-CUR"));
    assert!(error.to_string().contains("kumbara"));
}

#[tokio::test]
async fn a_caller_can_hold_one_behind_the_trait_object() {
    let providers: Vec<Box<dyn Provider>> = vec![Box::new(Kumbara::default())];
    let request = ChargeRequest::builder(OrderRef::new("ord-1"), ten_lira())
        .build()
        .expect("valid request");

    for provider in &providers {
        let charge = provider.charge(&request).await.expect("charge succeeds");
        let id = charge.id.expect("kumbara names every payment it opens");
        let first = provider.charge_status(&id).await.expect("status reads");
        assert_eq!(first.status, Status::Pending);
        let second = provider.charge_status(&id).await.expect("status reads");
        assert_eq!(second.status, Status::Captured);
        assert!(!second.status.is_open());
    }
}

#[tokio::test]
async fn a_provider_outside_the_workspace_can_hold_funds_and_take_part_of_them() {
    let kumbara = Kumbara::default();
    assert!(kumbara.capabilities().separate_capture);
    assert!(!kumbara.capabilities().partial_refund);

    let id = PaymentId::issued("kmb_ord-1");
    let captured = kumbara
        .capture(
            &id,
            Some(Money::parse("4.00", Currency::Try).expect("valid amount")),
            None,
        )
        .await
        .expect("part of the authorisation is taken");
    assert_eq!(captured.status, Status::Captured);
    assert_eq!(captured.amount.minor_units(), 400);

    let canceled = kumbara.cancel(&id).await.expect("the hold is released");
    assert_eq!(canceled.status, Status::Canceled);
    assert!(!canceled.status.is_open());
}

#[tokio::test]
async fn a_provider_outside_the_workspace_can_give_money_back() {
    let providers: Vec<Box<dyn Provider>> = vec![Box::new(Kumbara::default())];
    let request = RefundRequest::builder(PaymentId::issued("kmb_ord-1"))
        .amount(Money::parse("4.00", Currency::Try).expect("valid amount"))
        .reason(kasapay::RefundReason::RequestedByCustomer)
        .build()
        .expect("valid request");

    for provider in &providers {
        let refund = provider
            .refund(&request)
            .await
            .expect("the money goes back");
        assert_eq!(refund.amount.minor_units(), 400);
        assert_eq!(refund.status, RefundStatus::Succeeded);
        assert!(!refund.status.is_open());
        assert_eq!(
            refund.id.as_ref().map(RefundId::as_str),
            Some("kmb_re_kmb_ord-1")
        );
    }
}

/// A provider that names a payment by nothing at all.
///
/// Cash on delivery: no identifier of its own, and nothing sent to it that
/// could stand in for one. `Charge::id` is `None` and there is no reading a
/// payment back — the order reference is all anybody has.
#[derive(Debug)]
struct Yastik;

impl Yastik {
    const ID: ProviderId = ProviderId::new("yastik");

    fn unnamed() -> Error {
        Error::new(
            ErrorKind::Unsupported,
            Self::ID,
            "yastik names a payment by nothing; reconcile against the order reference",
        )
    }
}

#[async_trait]
impl Provider for Yastik {
    fn id(&self) -> ProviderId {
        Self::ID
    }

    async fn charge(&self, request: &ChargeRequest) -> Result<Charge, Error> {
        Ok(Charge {
            id: None,
            order: Some(request.order.clone()),
            amount: request.amount,
            order_amount: None,
            status: Status::Pending,
            next_action: None,
            provider: Self::ID,
            raw: Raw::default(),
        })
    }

    async fn charge_status(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Self::unnamed())
    }

    async fn capture(
        &self,
        _id: &PaymentId,
        _amount: Option<Money>,
        _idempotency: Option<&IdempotencyKey>,
    ) -> Result<Charge, Error> {
        Err(Self::unnamed())
    }

    async fn cancel(&self, _id: &PaymentId) -> Result<Charge, Error> {
        Err(Self::unnamed())
    }

    async fn refund(&self, _request: &RefundRequest) -> Result<Refund, Error> {
        Err(Self::unnamed())
    }

    async fn instruments(&self, _customer: &str) -> Result<Vec<Instrument>, Error> {
        Err(Error::new(
            ErrorKind::Unsupported,
            Self::ID,
            "yastik keeps no vault",
        ))
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

#[tokio::test]
async fn a_provider_that_names_a_payment_by_nothing_can_be_written() {
    let request = ChargeRequest::builder(OrderRef::new("ord-1"), ten_lira())
        .build()
        .expect("valid request");

    let charge = Yastik.charge(&request).await.expect("the cash is taken");
    assert_eq!(charge.id, None);
    assert_eq!(charge.order.as_ref().map(OrderRef::as_str), Some("ord-1"));

    let error = Yastik
        .charge_status(&PaymentId::issued("ord-1"))
        .await
        .expect_err("there is no payment to read back");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}
