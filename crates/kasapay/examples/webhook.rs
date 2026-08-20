//! Answers a payment notice the way a provider needs it answered.
//!
//! ```sh
//! cargo run -p kasapay --features paytr --example webhook
//! ```
//!
//! Nothing here talks to PayTR. The three deliveries below are built in this
//! file — one PayTR signed, one nobody signed, one carrying a status PayTR does
//! not document — because what this example is about is the handler, and a
//! handler is only interesting when the delivery is wrong.
//!
//! # The line that looks like a missing error path
//!
//! `answer` returns `"OK"` for all three. That is not a shortcut: **PayTR
//! retries any reply that is not exactly `OK`**, for days. A handler that
//! answers a 500 for a forged notice has arranged for the forgery to be
//! delivered again every hour, and a handler that answers one for an event
//! type it has not heard of does the same for something nobody wanted.
//!
//! So the two questions are separate, and the type system keeps them apart:
//! `verify` says whether this may be **acted on**, and the reply body says
//! whether the provider should **stop sending it**. Only the first is a
//! `Result`.
//!
//! # Why this handler can answer `OK` to every error, and one that cannot
//!
//! PayTR's `verify` is an HMAC over bytes already in hand. It reaches nothing,
//! so it cannot fail transiently: every `Err` it produces means the delivery
//! is not worth acting on, and acknowledging it is right.
//!
//! `kasapay_mollie::Mollie` is the exception in this workspace. Mollie signs
//! nothing, so its `verify` reads the payment back over the network — and an
//! `Err` there may mean the check did not finish rather than that the delivery
//! was bad. Copied as written, this catch-all would acknowledge a delivery
//! nobody read. `Error::is_retryable` is what separates them; `kasapay-mollie`'s
//! own crate documentation says which way to answer.

use std::error::Error;

use kasapay::paytr::{Config, Credentials, PayTr};
use kasapay::{Delivery, ErrorKind, Event, EventKind, IdSource, Webhook};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let credentials = Credentials::new("merchant-1", "merchant-key", "merchant-salt");
    // The client is the verifier: PayTR signs its notice with the same key and
    // salt every request carries, unlike Stripe's separate endpoint secret.
    let paytr = PayTr::new(Config::at("https://www.paytr.com", credentials.clone())?)?;
    let handler: &dyn Webhook = &paytr;

    // What PayTR posts: a form body, and a hash over three of its fields.
    let notice = |order: &str, status: &str, total: &str, hash: &str| {
        format!(
            "merchant_oid={order}&status={status}&total_amount={total}\
             &hash={}&payment_type=card&currency=TL",
            urlencoded(hash)
        )
    };
    let signed = |order: &str, status: &str, total: &str| {
        notice(
            order,
            status,
            total,
            &credentials.callback_hash(order, status, total),
        )
    };

    let deliveries = [
        // One PayTR signed, for a payment that went through.
        signed("ord-1", "success", "14990"),
        // One signed for a status PayTR does not document. A provider adding a
        // word is normal; refusing it is how a shop earns a week of retries.
        signed("ord-2", "pending", "14990"),
        // One claiming ten times the amount, with the hash left as it was.
        notice(
            "ord-3",
            "success",
            "149900",
            &credentials.callback_hash("ord-3", "success", "14990"),
        ),
    ];

    for body in &deliveries {
        // Headers and bytes, nothing parsed: a signature is over what arrived.
        let delivery = Delivery::new(&[], body.as_bytes());
        let reply = answer(handler.verify(&delivery).await);
        println!("answered {reply}\n");
    }

    Ok(())
}

/// What the handler writes back, which is `OK` whatever it decided.
fn answer(verified: Result<Event, kasapay::Error>) -> &'static str {
    match verified {
        Ok(event) => {
            match &event.kind {
                // What a shop acts on. The identifier goes into a unique index
                // first, so the second delivery of this event collides instead
                // of shipping the order twice.
                EventKind::Captured => println!("ship {}", show(&event)),
                EventKind::Failed => println!("do not ship {}", show(&event)),
                // Not an error. Something happened that this build has no word
                // for, and the provider is owed an acknowledgement for it.
                other => println!("noted {other:?} for {}", show(&event)),
            }
            // PayTR signs three fields and the currency is not one of them, so
            // the figure it sent is a number with no unit anybody can vouch
            // for. It is on `Event::raw`; `Notice::charge` is the call that
            // turns it into money, and it takes the currency from the caller.
            if event.amount.is_none()
                && let Some(total) = event.raw.text_at("/total_amount")
            {
                println!("  the notice says {total}, in a currency it did not sign");
            }
            "OK"
        }
        // Nothing was read out of this body, and nothing should be.
        Err(error) if error.kind() == ErrorKind::Untrusted => {
            println!("not acted on: {error}");
            "OK"
        }
        Err(error) => {
            println!("not read: {error}");
            "OK"
        }
    }
}

/// The payment, and whose uniqueness the caller's index would be resting on.
fn show(event: &Event) -> String {
    let payment = event
        .payment
        .as_ref()
        .map_or_else(|| "an unnamed payment".to_owned(), ToString::to_string);
    match event.id.source() {
        // Stripe and PayPal name the delivery itself.
        IdSource::Provider => format!("{payment} (event {}, the provider's own)", event.id),
        // PayTR and Mollie name nothing, so kasapay composes a key and says
        // which fields it rests on — which is what a unique index rests on too.
        IdSource::Derived(fields) => {
            format!("{payment} (event {}, composed from {fields:?})", event.id)
        }
    }
}

/// The hash is base64 and a form body is not, so `+`, `/` and `=` travel
/// escaped. A web framework does this on the way in; this example is the way
/// out.
fn urlencoded(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}
