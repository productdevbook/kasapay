//! Opens an iyzico hosted checkout form and reads what became of it.
//!
//! ```sh
//! IYZICO_API_KEY=… IYZICO_SECRET_KEY=… \
//!   cargo run -p kasapay --features iyzico --example iyzico_checkout
//! ```
//!
//! The form is the pan-free way to take a payment here: iyzico collects the
//! card, so no card data crosses this process. What goes in is iyzico-shaped —
//! a buyer, two addresses, an itemised basket — and what comes back is
//! kasapay's `Charge`.

use std::error::Error;

use kasapay::iyzico::Credentials;
use kasapay::iyzico::classic::{self, checkout};
use kasapay::{Currency, Money, NextAction, OrderRef, PaymentId, Provider, Status};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
        std::env::var("IYZICO_API_KEY")?,
        std::env::var("IYZICO_SECRET_KEY")?,
    )))?;

    let price = Money::parse("149.90", Currency::Try)?;
    let form = checkout::CheckoutForm::builder(
        OrderRef::new("ord-2026-0001"),
        price,
        "https://merchant.example/iyzico/callback".parse()?,
        checkout::Buyer {
            id: "buyer-1".into(),
            name: "Ayse".into(),
            surname: "Yilmaz".into(),
            // iyzico requires this. It is their compliance, not a payment
            // concept, which is why it is not on ChargeRequest.
            identity_number: "11111111111".into(),
            email: "ayse@example.test".into(),
            phone: "+905350000000".into(),
            registration_address: "Bagdat Cad. 1".into(),
            city: "Istanbul".into(),
            country: "Turkey".into(),
            zip_code: None,
            ip: None,
        },
    )
    .billing_address(checkout::Address {
        contact_name: "Ayse Yilmaz".into(),
        address: "Bagdat Cad. 1".into(),
        city: "Istanbul".into(),
        country: "Turkey".into(),
        zip_code: None,
    })
    .item(checkout::BasketItem {
        id: "item-1".into(),
        name: "Kahve".into(),
        category: "Icecek".into(),
        kind: checkout::ItemKind::Physical,
        // The basket has to add up to the price, and the builder says so
        // before iyzico does.
        price,
    })
    .build()?;

    let charge = iyzipay.start_checkout_form(&form).await?;

    let Some(NextAction::Redirect { url, continuation }) = charge.next_action else {
        return Err("an opened form always answers a redirect".into());
    };
    let token = continuation.ok_or("the form's token is what reads it back later")?;
    println!("send the payer to {url}");
    println!("keep this token: {token}");

    // Later, when the payer comes back. `charge_status` takes the token,
    // because until the form is finished there is no payment id — the finished
    // charge is the first thing to carry one.
    let settled = iyzipay.charge_status(&PaymentId::issued(&*token)).await?;
    match settled.status {
        Status::Captured => println!("paid {}", settled.amount),
        Status::Failed => println!("refused"),
        other => println!("not finished: {other:?}"),
    }

    Ok(())
}
