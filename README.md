# kasapay

One payment API in Rust, over any payment provider.

Write against one trait; the provider becomes a deployment decision. Stripe,
iyzico and PayTR ship today — PayPal, Adyen, Mollie, Param, Craftgate and the
rest are the same shape, and a provider outside this repository is a
first-class one: implement `Provider`, name it with `ProviderId::new`.

```toml
kasapay = { version = "0.0.1", features = ["stripe", "iyzico"] }
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

`provider` is a `Stripe`, an `in_store::Client`, or an `Arc<dyn Provider>`
picked at runtime. The calling code is the same.

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
| `kasapay-iyzico` | two iyzico APIs: `in_store` and `classic` |
| `kasapay-paytr` | PayTR's hosted form, status, refund, payment notice |

**The shared trait is `charge`, `charge_status`, `capture` and `cancel`**, with
`capabilities()` saying which of them a provider actually does — iyzico's
In-Store flow takes the money at authorisation and has no capture step, and a
caller planning a checkout needs to know that before it has a payment. Refunds,
webhooks and saved cards live on the providers for now — they enter the trait
once more than two providers have been written against them, not before.

**iyzico** is two APIs that barely resemble each other. `in_store` is the
counter flow, authenticated with plain headers, lira only. `classic` is
everything else, signed with `IYZWSv2`: the hosted checkout form, refunds,
cancel, stored cards, BIN lookup — with `iyzilink`, pay-by-link, and
`subscription`, the products and plans a recurring charge is sold out of, over
the same client. Twenty-nine of iyzico's ninety-six documented operations, and
none of them touches a card number.

**Every iyzico response iyzico signs is verified.** They sign the money-moving
ones with an HMAC over selected fields of the reply, and kasapay refuses one
that does not match — or one that carries no signature at all, unless you say
otherwise. A forged callback is how a shop ships against a payment that never
happened. Where they sign nothing there is nothing to check, and the module
says so: the classic cancel, every one of iyzilink's seven, and every one of
subscription's twenty-four.

## Where providers differ, it says so

- `Charge::id` — how the provider names the payment, and `PaymentId::source`
  says whose uniqueness that rests on. Stripe and iyzico issue their own;
  PayTR issues none and names a payment by the `merchant_oid` the merchant
  sent, so the identifier says it was derived from that field rather than
  passing a caller's own string off as the provider's. It is `None` where
  nothing has named the payment yet.
- `Charge::raw` — the provider's own answer, kept whole.
- A refusal is read where the provider reports it. PayTR's status query answers
  a payment that succeeded and nothing else, so `paytr::Notice::charge` — the
  notice PayTR posts — is where a refused payment becomes a `Status::Failed`
  charge. The query gives that payment and an order PayTR has never heard of
  the same answer, and `Status`'s own documentation carries the table of which
  provider can produce which status.
- `Stripe::client` — the `async-stripe` client itself, for calls kasapay
  does not make.
- iyzico's In-Store settles in lira only and requires `customer` and
  `return_url`. Ask it for USD and you get `ErrorKind::Unsupported` before a
  socket opens.
- The hosted checkout form does **not** go through `Provider::charge`. It needs
  a buyer's identity number, two addresses and an itemised basket, none of
  which belongs in `ChargeRequest`. The trait can express what every provider
  answers; it cannot express what each one demands.

## Amounts

`Money` counts minor units. No `f64` anywhere. `Money::parse("149.905", TRY)`
is an error, not a rounding, and a decimal on the wire is written from the
integer — `149.90`, never `149.90000000000001`. JPY has no minor unit and KWD
has three, and both are tested.

`checked_add` and `checked_sub` refuse to mix currencies; `partial_cmp`
answers `None` across them, because ten lira and ten dollars have no order.
There are no `+` and `-` operators: they would have to panic on a mismatch,
and a panic mid-checkout is worse than a `Result` somebody has to read.

## Specs

`specs/` records what each provider said its API was, dated. Neither iyzico nor
PayTR publishes an OpenAPI file: iyzico's is reassembled from the fragments
embedded in their documentation, and PayTR's is a record of their field tables —
including which fields enter the token hash, which is what signs a request.

A weekly job refetches all three and opens a PR when anything moved. A lost
field is the line to read first: a provider withdrawing one and the script
dropping one look identical in a diff, and only one of those is fine —
[`specs/README.md`](specs/README.md).

## Adding a provider

Implement `Provider` in a `kasapay-<name>` crate, add a spec fetcher under
`scripts/`, add a feature to `kasapay`. Tests run against `wiremock`; no
credentials, no network.

## Examples

`crates/kasapay/examples/` — a Stripe charge and refund, an iyzico hosted
checkout form end to end, and a PayTR hosted payment with the notice it posts
back. All three compile in CI, so they cannot drift from the API the way a
README snippet can.

```sh
STRIPE_SECRET_KEY=sk_test_… cargo run -p kasapay --features stripe --example stripe_charge
```

## Developing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run --workspace --all-features
cargo test --workspace --all-features --doc   # nextest skips doctests
```

MIT.
