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
//! until the reply body is exactly `OK`. The two notices below stand in for
//! that POST, one signed the way PayTR signs it and one not.

use std::error::Error;

use kasapay::paytr::{Config, Credentials, PayTr, payment, payment_id};
use kasapay::{Currency, ErrorKind, Money, NextAction, OrderRef, Provider};

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
    let signed = Notice {
        merchant_oid: order.as_str().to_owned(),
        status: "success".to_owned(),
        total_amount: total.clone(),
        hash: paytr
            .credentials()
            .callback_hash(order.as_str(), "success", &total),
        failed_reason: None,
    };
    let forged = Notice {
        hash: "not the hash PayTR would have sent".to_owned(),
        ..signed.clone()
    };

    println!("answered {}", answer_notice(paytr.credentials(), &signed));
    println!("answered {}", answer_notice(paytr.credentials(), &forged));

    // PayTR's status query answers a payment that happened, or an error saying
    // it does not know one — a refusal is reported on the notice, not here.
    match paytr.charge_status(&payment_id(&order)).await {
        Ok(settled) => println!("paid {}", settled.amount),
        Err(e) if e.kind() == ErrorKind::NotFound => println!("PayTR knows no such payment"),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

/// The fields of a payment notice a web handler would read off the form POST.
#[derive(Clone)]
struct Notice {
    merchant_oid: String,
    status: String,
    total_amount: String,
    hash: String,
    failed_reason: Option<String>,
}

/// What the handler writes back, which is `OK` whatever the notice said.
fn answer_notice(credentials: &Credentials, notice: &Notice) -> &'static str {
    if !credentials.verify_callback(
        &notice.hash,
        &notice.merchant_oid,
        &notice.status,
        &notice.total_amount,
    ) {
        // Answering anything else makes PayTR retry it for days, and acting on
        // it is how a shop ships against a payment nobody made.
        println!("a notice for {} did not verify", notice.merchant_oid);
        return "OK";
    }

    match notice.status.as_str() {
        "success" => println!(
            "{} paid {} minor units",
            notice.merchant_oid, notice.total_amount
        ),
        "failed" => println!(
            "{} was refused: {}",
            notice.merchant_oid,
            notice.failed_reason.as_deref().unwrap_or("no reason given")
        ),
        other => println!("{} carried an unknown status {other}", notice.merchant_oid),
    }
    "OK"
}
