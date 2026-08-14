//! Opens a Mollie payment, then a second one Mollie will hold rather than
//! take, and captures part of it.
//!
//! ```sh
//! MOLLIE_API_KEY=test_… cargo run -p kasapay --features mollie --example mollie_payment
//! ```
//!
//! Mollie hosts the checkout, so no card data crosses this process. What it
//! does not host is the decision between taking the money and holding it:
//! that is fixed when the payment is created, which is why there are two calls
//! here and only one of them is `Provider::charge`.
//!
//! Nothing below waits for the payer. Mollie's webhook is what says a payment
//! moved, and it carries one field — the payment's id — and no signature, so
//! the only safe response to one is to read the payment back the way
//! `charge_status` does here.

use std::error::Error;

use kasapay::mollie::{Config, Mollie};
use kasapay::{ChargeRequest, Currency, ErrorKind, Money, NextAction, OrderRef, Provider, Secret};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mollie = Mollie::new(Config::new(Secret::new(std::env::var("MOLLIE_API_KEY")?)))?;

    // Mollie takes neither lira nor Kuwaiti dinar, and says so before a socket
    // opens rather than sending a currency it will not read.
    let request = |order: &str| -> Result<ChargeRequest, Box<dyn Error>> {
        Ok(
            ChargeRequest::builder(OrderRef::new(order), Money::parse("10.00", Currency::Eur)?)
                // Mollie requires both of these; `ChargeRequest` calls both optional.
                .description("Order #12345")
                .return_url("https://webshop.example/order/12345".parse()?)
                .build()?,
        )
    };

    let charge = mollie.charge(&request("ord-2026-0001")?).await?;
    match &charge.next_action {
        // `continuation` is None: Mollie wants nothing back but the id.
        Some(NextAction::Redirect { url, .. }) => println!("send the payer to {url}"),
        // NextAction is non-exhaustive, so a caller has to say what it does
        // with one kasapay learns about after they build.
        Some(other) => {
            println!("this build does not know how to finish {other:?}");
            return Ok(());
        }
        None => return Err("an opened payment always answers a checkout address".into()),
    }

    let id = charge.id.as_ref().ok_or("Mollie names every payment")?;
    println!("{id} is {:?} for {}", charge.status, charge.amount);

    // A hold is a different payment, not a different call afterwards.
    let held = mollie.authorize(&request("ord-2026-0002")?).await?;
    let held_id = held.id.as_ref().ok_or("Mollie names every payment")?;

    // Which will refuse until the payer has authorised it — the point being
    // that the failure is Mollie's own 422 rather than something invented.
    match mollie
        .capture_payment(held_id, Some(Money::parse("4.00", Currency::Eur)?))
        .await
    {
        // The capture is its own object with its own `cpt_…` identifier, and
        // the compiler will not let it be passed where a payment id goes.
        Ok(capture) => println!("captured {} as {}", capture.amount, capture.id),
        Err(e) if e.kind() == ErrorKind::InvalidRequest => {
            println!("nothing to capture yet: {e}");
        }
        Err(e) => return Err(e.into()),
    }

    // Releasing a hold answers 202 and no body at all, so there is no charge
    // to read; the payment is what says what became of it.
    mollie.release_authorization(held_id).await?;
    println!(
        "released: {:?}",
        mollie.charge_status(held_id).await?.status
    );

    Ok(())
}
