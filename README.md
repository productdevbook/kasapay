# kasapay

One payment API in Rust, over any payment provider.

Write against one trait; the provider becomes a deployment decision. Stripe and
iyzico ship today — PayPal, Adyen, Mollie, PayTR, Param, Craftgate and the rest
are the same shape, and a provider outside this repository is a first-class one:
implement `Provider`, name it with `ProviderId::new`.

```toml
kasapay = { version = "0.1", features = ["stripe", "iyzico"] }
```

```rust
let charge = provider.charge(
    &ChargeRequest::builder(OrderRef::new("ord-1"), Money::parse("149.90", Currency::Try)?)
        .customer("kasiyer-7")
        .return_url("https://merchant.example/callback".parse()?)
        .build()?,
).await?;

match charge.next_action {
    Some(NextAction::Redirect { url, .. }) => send_payer_to(url),
    Some(NextAction::ConfirmOnClient { client_secret }) => hand_to_browser(client_secret),
    None if charge.status == Status::Captured => mark_paid(),
    None => wait(),
}
```

`provider` is a `Stripe`, an `Iyzico`, or an `Arc<dyn Provider>` picked at
runtime. The calling code is the same.

## `charge()` returning `Ok` is not a payment

Every provider stalls somewhere: Stripe wants the browser to confirm a
`client_secret`, iyzico wants the payer in its own app. So `charge` returns a
`Status` and a `NextAction`, never "paid". No provider can answer that
synchronously, so kasapay does not pretend to.

## Crates

| | |
|---|---|
| `kasapay` | facade; providers behind features |
| `kasapay-core` | `Money`, `Charge`, `Error`, `Provider`. No network, no HTTP client |
| `kasapay-stripe` | over [`async-stripe`](https://github.com/arlyon/async-stripe) |
| `kasapay-iyzico` | In-Store API v3 |

v0.1 covers `charge` and `charge_status`. Refunds, webhooks and saved cards are
where providers disagree most; they enter the shared trait once more than two
of them have been written, not before.

## Where providers differ, it says so

- `Charge::raw` — the provider's own response, untouched.
- `Stripe::client` — the `async-stripe` client itself, for calls kasapay
  does not make.
- iyzico settles in lira only and requires `customer` and `return_url`. Ask it
  for USD and you get `ErrorKind::Unsupported` before a socket opens.

## Amounts

`Money` counts minor units. No `f64` anywhere. `Money::parse("149.905", TRY)`
is an error, not a rounding, and a decimal on the wire is written from the
integer — `149.90`, never `149.90000000000001`.

## Specs

`specs/` records what each provider said its API was, dated. iyzico publishes
no OpenAPI file; theirs is reassembled from their documentation page. A weekly
job refetches and opens a PR when anything moved — [`specs/README.md`](specs/README.md).

## Adding a provider

Implement `Provider` in a `kasapay-<name>` crate, add a spec fetcher under
`scripts/`, add a feature to `kasapay`. Tests run against `wiremock`; no
credentials, no network.

## Developing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run --workspace --all-features
cargo test --workspace --all-features --doc   # nextest skips doctests
```

MIT.
