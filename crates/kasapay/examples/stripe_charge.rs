//! Takes a payment with Stripe, then gives half of it back.
//!
//! ```sh
//! STRIPE_SECRET_KEY=sk_test_… cargo run -p kasapay --features stripe --example stripe_charge
//! ```
//!
//! Nothing here is Stripe-shaped except building the client. The charge, the
//! stalled state and the amounts are kasapay's vocabulary, and the same code
//! reads a payment from any provider.

use std::error::Error;

use kasapay::stripe::Stripe;
use kasapay::{
    ChargeRequest, Currency, IdempotencyKey, Money, NextAction, OrderRef, PaymentId, Provider,
    Secret, Status,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let key = std::env::var("STRIPE_SECRET_KEY")?;
    let stripe = Stripe::new(&Secret::new(key));

    let request = ChargeRequest::builder(
        OrderRef::new("ord-2026-0001"),
        Money::parse("19.99", Currency::Usd)?,
    )
    .description("one coffee")
    // Sent as Stripe's Idempotency-Key. Replaying this request with the same
    // key takes the money once.
    .idempotency_key(IdempotencyKey::new("ord-2026-0001-attempt-1"))
    .build()?;

    let charge = stripe.charge(&request).await?;
    let id = charge.id.as_ref().map_or("unnamed", PaymentId::as_str);
    println!("{} {id} — {:?}", charge.provider, charge.status);

    // `Ok` is not a payment. Stripe stalls until the browser confirms.
    match &charge.next_action {
        Some(NextAction::ConfirmOnClient { client_secret }) => {
            println!("hand this to Stripe.js: {client_secret}");
            return Ok(());
        }
        Some(NextAction::Redirect { url, .. }) => {
            println!("send the payer to {url}");
            return Ok(());
        }
        // NextAction is non-exhaustive, so a caller has to say what it does
        // with one kasapay learns about after they build. Refusing to guess is
        // the right answer for money.
        Some(other) => {
            println!("this build does not know how to finish {other:?}");
            return Ok(());
        }
        None if charge.status == Status::Captured => println!("paid"),
        None => {
            println!("nothing to do yet, and not captured either");
            return Ok(());
        }
    }

    let half = Money::from_minor_units(charge.amount.minor_units() / 2, charge.amount.currency());
    let id = charge.id.ok_or("Stripe names every payment it takes")?;
    let refund = stripe.refund(&id, Some(half)).await?;
    println!("refunded {} — {:?}", refund.amount, refund.status);

    Ok(())
}
