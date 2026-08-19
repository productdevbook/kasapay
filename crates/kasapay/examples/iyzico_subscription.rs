//! Sells a subscription: a product, a plan, and somebody signed up to it.
//!
//! ```sh
//! IYZICO_API_KEY=… IYZICO_SECRET_KEY=… \
//!   cargo run -p kasapay --features iyzico --example iyzico_subscription
//! ```
//!
//! Runs against iyzico's sandbox. It creates a product and a monthly plan —
//! both of which stay in the merchant's catalogue afterwards — and then opens
//! the hosted form that subscribes somebody to it.
//!
//! # No card number goes near this process
//!
//! iyzico has two ways to start a subscription. The other one,
//! `POST /v2/subscription/initialize`, takes the card number, expiry and CVC
//! on the request, and this crate does not implement it: a server that touches
//! a card number is in PCI DSS scope on the merchant's longest
//! self-assessment. What is here instead is the form iyzico hosts, and the
//! second subscription for the same person costs no card either — iyzico
//! already holds it.
//!
//! # A subscription is a standing authority to take money
//!
//! Which is why the last thing this prints is how to stop one.

use std::error::Error;

use kasapay::iyzico::subscription::{
    Address, Client, InitialStatus, NewPlan, NewProduct, NewSubscription, PaymentInterval,
    Subscriber,
};
use kasapay::iyzico::{Credentials, classic};
use kasapay::{Currency, Money};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let credentials = Credentials::new(
        std::env::var("IYZICO_API_KEY")?,
        std::env::var("IYZICO_SECRET_KEY")?,
    );
    let iyzipay = classic::Client::new(classic::Config::sandbox(credentials))?;
    let subscriptions = Client::new(iyzipay);

    // The catalogue: what is sold, and what it costs.
    let magazine = subscriptions
        .create_product(
            &NewProduct::builder("A Dergisi")
                .describe("Aylık dergi aboneliği")
                .build(),
        )
        .await?;
    println!("product {}", magazine.reference_code);

    let monthly = subscriptions
        .create_plan(
            &magazine.reference_code,
            &NewPlan::builder(
                "A Dergisi aylık",
                Money::parse("50.00", Currency::Try)?,
                PaymentInterval::Monthly,
            )
            .trial_days(14)
            .build()?,
        )
        .await?;
    // `price` is an Option: iyzico answers a plan without one for a plan
    // whose payment type is not a fixed recurring amount.
    println!("plan {} — {:?}", monthly.reference_code, monthly.price);

    // The subscriber. Every field but the shipping address is iyzico's own
    // requirement: a subscription is a standing authority to take money, and
    // they ask for enough to know who gave it.
    let subscriber = Subscriber::builder(
        "Ayşe",
        "Yılmaz",
        "ayse@example.test",
        "+905350000000",
        "11111111111",
        Address::new("Ayşe Yılmaz", "Bağdat Cad. 1", "İstanbul", "Turkey"),
    )
    .build();

    let form = subscriptions
        .start_subscription_form(
            &NewSubscription::builder(
                &*monthly.reference_code,
                subscriber,
                "https://merchant.test/subscribed",
            )
            // ACTIVE starts it and takes the first payment. PENDING waits for
            // `activate`, which is what a shop uses when the subscription
            // depends on something outside iyzico.
            .initial_status(InitialStatus::Active)
            .build(),
        )
        .await?;

    // Unlike the classic checkout form, iyzico answers no page URL here: their
    // schema documents the form's HTML and nothing to redirect to. So this is
    // embedded rather than redirected to.
    println!(
        "form token {} — valid for {:?} seconds",
        form.token, form.expires_in_seconds
    );
    println!(
        "embed {} bytes of iyzico's own HTML",
        form.content.as_deref().map_or(0, str::len)
    );

    // Afterwards, at the callback address: `subscription_form_result(token)`
    // says what became of it, and `subscriptions(1, 20)` lists what is running.
    //
    // And the way out, because a subscription that cannot be stopped is not
    // one to sell:
    println!("stop it with subscriptions.cancel(reference)");

    Ok(())
}
