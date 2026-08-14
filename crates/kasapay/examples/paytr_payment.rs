//! Opens a PayTR hosted payment, answers the notice it posts back, and reads
//! the payment.
//!
//! ```sh
//! PAYTR_MERCHANT_ID=… PAYTR_MERCHANT_KEY=… PAYTR_MERCHANT_SALT=… \
//!   cargo run -p kasapay --features paytr --example paytr_payment
//! ```
//!
//! PayTR hosts the form, so no card data crosses this process. It also has no
//! payment id: the merchant's own order reference names the payment on every
//! later call, which is why one is never reused and why `paytr::payment_id`
//! rather than a bare identifier is what reads it back.
//!
//! The half nobody gets right is the payment notice. PayTR does not wait for
//! the payer to come back — it posts the outcome to the merchant and retries
//! until the reply body is exactly `OK`. It is also the only place a refusal
//! is reported. The three notices below stand in for that POST: one that PayTR
//! would have signed, one refusing the payment, and one forged.

use std::error::Error;

use kasapay::paytr::{Config, Credentials, Notice, PayTr, payment, payment_id};
use kasapay::{Currency, ErrorKind, Money, NextAction, OrderRef, Provider, Status};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let paytr = PayTr::new(
        Config::new(Credentials::new(
            std::env::var("PAYTR_MERCHANT_ID")?,
            std::env::var("PAYTR_MERCHANT_KEY")?,
            std::env::var("PAYTR_MERCHANT_SALT")?,
        ))
        .test_mode(),
    )?;

    let order = OrderRef::new("ord-2026-0001");
    let price = Money::parse("149.90", Currency::Try)?;
    let payment = payment::Payment::builder(
        order.clone(),
        price,
        payment::Payer {
            email: "ayse@example.test".into(),
            // PayTR refuses a token without the address the payer's request
            // came from, and refuses a private one.
            ip: "203.0.113.7".into(),
            name: "Ayse Yilmaz".into(),
            address: "Bagdat Cad. 1".into(),
            phone: "+905350000000".into(),
            success_url: "https://merchant.example/paytr/ok".parse()?,
            failure_url: "https://merchant.example/paytr/no".parse()?,
        },
    )
    .item(payment::BasketItem {
        name: "Kahve".into(),
        price,
        quantity: 1,
    })
    .build()?;

    let charge = paytr.start_payment(&payment).await?;
    match &charge.next_action {
        Some(NextAction::Redirect { url, .. }) => println!("send the payer to {url}"),
        // NextAction is non-exhaustive, so a caller has to say what it does
        // with one kasapay learns about after they build.
        Some(other) => {
            println!("this build does not know how to finish {other:?}");
            return Ok(());
        }
        None => return Err("an opened payment always answers a redirect".into()),
    }

    let total = charge.amount.minor_units().to_string();
    let notice = |outcome: &str| Notice {
        merchant_oid: order.as_str().into(),
        status: outcome.into(),
        total_amount: total.as_str().into(),
        hash: paytr
            .credentials()
            .callback_hash(order.as_str(), outcome, &total)
            .into(),
        failed_reason_code: None,
        failed_reason_msg: None,
        payment_amount: None,
        currency: Some("TL".into()),
        payment_type: Some("card".into()),
        test_mode: Some("1".into()),
    };
    let refused = Notice {
        failed_reason_code: Some("0".into()),
        failed_reason_msg: Some("Kartin limiti yetersiz".into()),
        ..notice("failed")
    };
    let forged = Notice {
        hash: "not the hash PayTR would have sent".into(),
        ..notice("success")
    };

    for posted in [notice("success"), refused, forged] {
        println!("answered {}", answer_notice(paytr.credentials(), &posted));
    }

    // The status query answers a payment that succeeded. A payment PayTR
    // refused and an order it never heard of are the same answer here, which
    // is why the refusal above came off the notice instead.
    match paytr.charge_status(&payment_id(&order)).await {
        Ok(settled) => println!("paid {}", settled.amount),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            println!("PayTR reports no successful payment for {order}");
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

/// What the handler writes back, which is `OK` whatever the notice said.
fn answer_notice(credentials: &Credentials, notice: &Notice) -> &'static str {
    // The currency is not covered by PayTR's hash, so it comes from what the
    // payment was opened in rather than from the notice.
    match notice.charge(credentials, Currency::Try) {
        Ok(charge) if charge.status == Status::Failed => println!(
            "{} was refused: {}",
            notice.merchant_oid,
            notice
                .failed_reason_msg
                .as_deref()
                .unwrap_or("no reason given")
        ),
        Ok(charge) => println!("{} paid {}", notice.merchant_oid, charge.amount),
        // Answering anything else makes PayTR retry it for days, and acting on
        // it is how a shop ships against a payment nobody made.
        Err(e) => println!("{} was not acted on: {e}", notice.merchant_oid),
    }
    "OK"
}
