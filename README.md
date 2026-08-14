# kasapay

One payment API in Rust, over more than one payment provider.

```toml
[dependencies]
kasapay = { version = "0.1", features = ["stripe", "iyzico"] }
```

```rust
use kasapay::{ChargeRequest, Currency, Money, NextAction, OrderRef, Provider, Status};

let request = ChargeRequest::builder(
    OrderRef::new("ord-2026-0001"),
    Money::parse("149.90", Currency::Try)?,
)
.customer("kasiyer-7")
.return_url("https://merchant.example/callback".parse()?)
.build()?;

let charge = provider.charge(&request).await?;

match charge.status {
    Status::RequiresAction => match charge.next_action {
        Some(NextAction::Redirect { url, .. }) => send_the_payer_to(url),
        Some(NextAction::ConfirmOnClient { client_secret }) => hand_to_the_browser(client_secret),
        None => unreachable!("a stalled charge always says what it is waiting for"),
    },
    Status::Captured => mark_paid(),
    _ => wait(),
}
```

`provider` there is a `Stripe` or an `Iyzico` — or an `Arc<dyn Provider>`
chosen at runtime. The calling code does not change.

## The one thing to understand first

**`charge()` returning `Ok` does not mean the money moved.** Every provider
worth supporting stalls somewhere: Stripe hands back a `client_secret` for the
browser to confirm, iyzico hands back a deep link into its own app. So `charge`
returns a `Charge` with a `Status` and, when it is waiting, a `NextAction`
saying what the payer must do. There is no method that returns "paid", because
no provider can answer that synchronously.

## What is here

| Crate | |
|---|---|
| `kasapay` | the facade — re-exports everything, providers behind features |
| `kasapay-core` | `Money`, `Charge`, `Error`, and the `Provider` trait. No network |
| `kasapay-stripe` | Stripe, over [`async-stripe`](https://github.com/arlyon/async-stripe) |
| `kasapay-iyzico` | iyzico In-Store API v3, written against its documented spec |

`kasapay-core` has no HTTP client in it and never will. A provider crate brings
its own, and a provider that lives outside this repository is a first-class one:
implement `Provider`, name yourself with `ProviderId::new`.

### What v0.1 covers

Taking a payment (`charge`) and reading it back (`charge_status`), plus
iyzico's refund as a provider-specific method. Refunds, webhooks and saved
cards are not in the shared trait yet — they are where providers disagree
most, and putting them in before the abstraction has been used in anger is how
these libraries end up leaking.

## What is deliberately not abstracted

Providers are not interchangeable, and pretending they are is the failure mode
of every library like this one. Where they differ, kasapay says so out loud:

- **`Charge::raw`** carries the provider's own response, untouched. Anything
  not modelled is still reachable.
- **`Stripe::client`** hands back the `async-stripe` client itself for calls
  kasapay does not make.
- **iyzico requires** `customer` (it is their `userId`) and `return_url` (it
  becomes the `x-callback-url` header), and settles in Turkish lira only. Ask
  it for USD and you get `ErrorKind::Unsupported` before a socket is opened.

## Amounts

`Money` counts minor units — 14990, not 149.90 — and there is no `f64` in the
crate. `Money::parse("149.905", Currency::Try)` is an error rather than a
rounding. When a provider wants a decimal on the wire, it is written from the
integer, so 149.90 goes out as `149.90` and never as `149.90000000000001`.

## Specs

`specs/` holds what each provider said its API was, dated. iyzico publishes no
OpenAPI file at all — theirs is reassembled from the fragments embedded in
their documentation page. A weekly job refetches both and opens a pull request
when anything moved. See [`specs/README.md`](specs/README.md).

## Developing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run --workspace --all-features
cargo test --workspace --all-features --doc   # nextest does not run doctests
```

The iyzico tests run against a `wiremock` server, so the suite needs no
credentials and reaches no network.

## Licence

MIT.
