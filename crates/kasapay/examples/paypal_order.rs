//! Opens a PayPal order, then captures it once the payer has approved it.
//!
//! ```sh
//! PAYPAL_CLIENT_ID=… PAYPAL_CLIENT_SECRET=… \
//!     cargo run -p kasapay --features paypal --example paypal_order
//! ```
//!
//! PayPal hosts the checkout, so no card data crosses this process. What it
//! does not do is capture on its own: every order needs the explicit
//! [`Provider::capture`] below regardless of the intent it was created with,
//! which is why this example stops and waits rather than finishing in one
//! call the way a card-first provider's example can.

use std::error::Error;
use std::io::{self, Write};

use kasapay::paypal::{Config, PayPal};
use kasapay::{ChargeRequest, Currency, Money, NextAction, OrderRef, Provider, Secret};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let paypal = PayPal::new(Config::sandbox(
        Secret::new(std::env::var("PAYPAL_CLIENT_ID")?),
        Secret::new(std::env::var("PAYPAL_CLIENT_SECRET")?),
    ))?;

    // PayPal takes neither Turkish lira nor Kuwaiti dinar, and says so before
    // a socket opens rather than sending a currency it will not read.
    let request = ChargeRequest::builder(
        OrderRef::new("ord-2026-0001"),
        Money::parse("10.00", Currency::Usd)?,
    )
    .return_url("https://webshop.example/order/12345".parse()?)
    .build()?;

    let charge = paypal.charge(&request).await?;
    match &charge.next_action {
        // `continuation` is None: PayPal wants nothing back but the payer
        // arriving at the return_url, and the order is read by its id.
        Some(NextAction::Redirect { url, .. }) => println!("send the payer to {url}"),
        Some(other) => {
            println!("this build does not know how to finish {other:?}");
            return Ok(());
        }
        None => return Err("a freshly opened order always answers an approval address".into()),
    }

    let id = charge.id.as_ref().ok_or("PayPal names every order")?;
    println!("{id} is {:?} for {}", charge.status, charge.amount);

    print!("press enter once the payer has approved it at PayPal: ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut String::new())?;

    // Every PayPal order needs this call regardless of intent — there is no
    // PayPal equivalent of a payment that captures itself on approval.
    let captured = paypal.capture(id, None).await?;
    println!("{:?}", captured.status);

    Ok(())
}
