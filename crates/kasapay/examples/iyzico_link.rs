//! Sells one thing over a link iyzico hosts, then takes the link down.
//!
//! ```sh
//! IYZICO_API_KEY=… IYZICO_SECRET_KEY=… \
//!   cargo run -p kasapay --features iyzico --example iyzico_link
//! ```
//!
//! There is no integration behind an iyzico Link: what comes back is a URL,
//! and sharing it is the whole checkout. No card data crosses this process and
//! no callback has to be handled to make the sale.
//!
//! What it does cost is written out below — nothing iyzilink answers is signed,
//! so nothing it answers may settle money.

use std::error::Error;

use kasapay::iyzico::Credentials;
use kasapay::iyzico::classic;
use kasapay::iyzico::iyzilink::{Category, Client, LinkStatus, NewLink};
use kasapay::{Currency, Money};

/// A one-pixel PNG. iyzico wants the picture as base64 and will not take a URL.
const PICTURE: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // The same client, credentials and connection pool as the rest of the
    // classic API: iyzilink is part of it rather than beside it.
    let iyzipay = classic::Client::new(classic::Config::sandbox(Credentials::new(
        std::env::var("IYZICO_API_KEY")?,
        std::env::var("IYZICO_SECRET_KEY")?,
    )))?;
    let links = Client::new(iyzipay);

    let coffee = NewLink::builder(
        "Filtre kahve",
        "250g, haftalık kavrum",
        Money::parse("149.90", Currency::Try)?,
        PICTURE,
    )
    .category(Category::Food)
    .stock(40)
    .build()?;

    let link = links.create(&coffee).await?;
    match link.url.as_ref() {
        Some(url) => println!("share this: {url}"),
        // iyzico documents the URL as always present. Believing that would put
        // an `expect` in a caller's path over a field nothing verifies.
        None => println!("created {}, but iyzico sent no URL back", link.token),
    }

    let details = links.get(&link.token).await?;
    println!(
        "{} sold {} so far",
        details.name.as_deref().unwrap_or("the link"),
        details.sold_count.unwrap_or(0)
    );

    // **Do not settle anything against that number.** iyzilink responses carry
    // no signature — iyzico's response-signature page lists no iyzilink
    // endpoint — so `sold_count` and `price` are what the connection said, not
    // what iyzico can be shown to have said. Read the payment itself through
    // `classic::Client::payment`, which is signed, before shipping anything.

    // Selling stops without the link or its history going anywhere. `delete`
    // is the other choice, and it does not come back.
    links.set_status(&link.token, &LinkStatus::Passive).await?;
    println!("{} is off", link.token);

    Ok(())
}
