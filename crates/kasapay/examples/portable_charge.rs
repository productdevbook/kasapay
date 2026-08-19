//! One `ChargeRequest`, two providers, no branch that names either of them.
//!
//! ```sh
//! KASAPAY_PROVIDER=iyzico IYZICO_API_KEY=… IYZICO_SECRET_KEY=… \
//!   cargo run -p kasapay --features iyzico,paytr --example portable_charge
//! KASAPAY_PROVIDER=paytr PAYTR_MERCHANT_ID=… PAYTR_MERCHANT_KEY=… \
//!   PAYTR_MERCHANT_SALT=… \
//!   cargo run -p kasapay --features iyzico,paytr --example portable_charge
//! ```
//!
//! This is the claim the library rests on, so it is worth one example that
//! does nothing else: the request is built once, the provider is a
//! `Box<dyn Provider>` chosen from an environment variable, and swapping one
//! for the other changes no line of what is being asked for.
//!
//! It is also the request that works on both, which is the request carrying
//! what the *stricter* of them asks. iyzico wants a surname, an identity
//! number, a phone number, an address and a category on every basket line;
//! PayTR wants an email, a phone number, an address and the IP the payer's own
//! request came from. Neither minds being handed the other's fields. Leave one
//! out and the adapter that needs it says so by name, before a socket opens —
//! which is what the second half of this example shows.

use std::error::Error;

use kasapay::iyzico::classic::{Client as Iyzipay, Config as IyzicoConfig};
use kasapay::paytr::{Config as PaytrConfig, Credentials as PaytrCredentials, PayTr};
use kasapay::{
    Address, BasketItem, Buyer, ChargeRequest, Currency, ErrorKind, Money, NextAction, OrderRef,
    Provider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let provider: Box<dyn Provider> = match std::env::var("KASAPAY_PROVIDER")?.as_str() {
        "iyzico" => Box::new(Iyzipay::new(IyzicoConfig::sandbox(
            kasapay::iyzico::Credentials::new(
                std::env::var("IYZICO_API_KEY")?,
                std::env::var("IYZICO_SECRET_KEY")?,
            ),
        ))?),
        "paytr" => Box::new(PayTr::new(
            PaytrConfig::new(PaytrCredentials::new(
                std::env::var("PAYTR_MERCHANT_ID")?,
                std::env::var("PAYTR_MERCHANT_KEY")?,
                std::env::var("PAYTR_MERCHANT_SALT")?,
            ))
            .test_mode(),
        )?),
        other => return Err(format!("no provider called {other}").into()),
    };

    let coffee = Money::parse("74.95", Currency::Try)?;
    let request = ChargeRequest::builder(
        OrderRef::new("ord-2026-0007"),
        // Two coffees. The amount is what the card is charged, which is the
        // basket here because nothing has been added for instalments.
        Money::parse("149.90", Currency::Try)?,
    )
    .return_url("https://merchant.example/paid".parse()?)
    .failure_url("https://merchant.example/failed".parse()?)
    .buyer(
        Buyer::new("Ayse", "ayse@example.test")
            .surname("Yilmaz")
            .identity_number("11111111111")
            .phone("+905350000000")
            .ip("203.0.113.7")
            .address(Address::new("Bagdat Cad. 1", "Istanbul", "Turkey").zip_code("34000")),
    )
    .item(
        BasketItem::new("sku-kahve", "Kahve", coffee)
            .category("Icecek")
            .quantity(2),
    )
    .build()?;

    let charge = provider.charge(&request).await?;
    println!("{} says: {:?}", provider.id(), charge.status);
    match &charge.next_action {
        Some(NextAction::Redirect { url, .. }) => println!("send the payer to {url}"),
        // NextAction is non-exhaustive, so a caller has to say what it does
        // with one kasapay learns about after they build.
        Some(other) => println!("this build does not know how to finish {other:?}"),
        None => println!("no redirect: the payment is already decided"),
    }

    // The same request with the buyer left out. Both providers refuse it
    // before opening a socket, and the message names the field rather than
    // handing back a numbered code from Ankara.
    let incomplete = ChargeRequest::builder(
        OrderRef::new("ord-2026-0008"),
        Money::parse("149.90", Currency::Try)?,
    )
    .return_url("https://merchant.example/paid".parse()?)
    .build()?;
    match provider.charge(&incomplete).await {
        Ok(_) => return Err("a payment with no buyer should not have opened".into()),
        Err(error) => {
            assert_eq!(error.kind(), ErrorKind::InvalidRequest);
            println!("refused before the network: {error}");
        }
    }

    Ok(())
}
