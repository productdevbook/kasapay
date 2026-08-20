<div align="center">

# kasapay

**One payment API in Rust, over any payment provider.**

[![crates.io](https://img.shields.io/crates/v/kasapay.svg?style=flat-square)](https://crates.io/crates/kasapay)
[![docs.rs](https://img.shields.io/docsrs/kasapay?style=flat-square)](https://docs.rs/kasapay)
[![CI](https://img.shields.io/github/actions/workflow/status/productdevbook/kasapay/ci.yml?branch=main&style=flat-square)](https://github.com/productdevbook/kasapay/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue?style=flat-square)](https://github.com/productdevbook/kasapay)
[![licence](https://img.shields.io/crates/l/kasapay.svg?style=flat-square)](LICENSE)

Stripe · iyzico · PayTR · Mollie · PayPal

</div>

---

Write against one trait and the provider becomes a deployment decision. A
provider outside this repository is a first-class one: implement `Provider`,
name it with `ProviderId::new`.

## Install

```toml
[dependencies]
kasapay = { version = "0.0.5", features = ["stripe", "iyzico"] }
```

One feature per provider, and they are additive: `stripe`, `iyzico`, `paytr`,
`mollie`, `paypal`.

## Quick start

```rust
use kasapay::{ChargeRequest, Currency, Money, NextAction, OrderRef, Provider, Status};

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

`provider` is a `Stripe`, an `iyzico::classic::Client`, or an
`Arc<dyn Provider>` chosen at runtime. The calling code does not change.

**`charge()` returning `Ok` is not a payment.** Every provider stalls
somewhere — Stripe wants the browser to confirm a `client_secret`, iyzico
wants the payer in its own app — so `charge` answers a `Status` and a
`NextAction`, never "paid".

## The traits

| `Provider` | |
|---|---|
| `charge` | Starts a payment, optionally against a saved instrument. |
| `resume` | Finishes a redirect flow from the token it handed over. |
| `charge_status` | Reads a payment back by the provider's own identifier. |
| `capture` | Takes funds an authorisation is holding. |
| `cancel` | Releases an authorisation that will never be taken, answering a `Release`. |
| `refund` | Gives money back, answering a `Refund` rather than a status. |
| `lookup` | Finds a payment by the caller's own order reference. |
| `instruments` | Lists what a customer has saved. |
| `capabilities` | What this provider will do, before there is a payment. |

`Webhook` is the second trait: `verify(&Delivery) -> Result<Event, Error>`. It
is `async` because verification is not one mechanism — Stripe signs the bytes,
PayTR signs three fields of them, Mollie signs nothing and posts an identifier
to read back, and PayPal verifies a delivery over its own API.

## Support

| | Stripe | iyzico `classic` | iyzico `in_store` | PayTR | Mollie | PayPal |
|---|:-:|:-:|:-:|:-:|:-:|:-:|
| Separate capture | ● | ● | ○ | ○ | ● | ● |
| Partial capture | ● | ● | ○ | ○ | ● | ○ |
| Partial refund | ● | ● | ● | ● | ● | ● |
| Repeated refund | ● | ● | ○ | ● | ● | ● |
| Lookup by order | ○ | ● | ○ | ● | ○ | ○ |
| Resume by token | ○ | ● | ○ | ○ | ○ | ○ |
| Saved instruments | ● | ● | ○ | ○ | ● | ○ |
| Webhook | ● | ○ | ○ | ● | ● | ● |

`capabilities()` answers this at runtime, and
[`crates/kasapay/tests/conformance.rs`](crates/kasapay/tests/conformance.rs)
asserts every cell against the behaviour underneath it: a `false` pairs with
`ErrorKind::Unsupported` raised before a socket opens, a `true` pairs with a
request on the wire.

iyzico has no `Webhook` implementation because iyzico documents no delivery it
posts and no signature over one. Verifying a body means knowing the mechanism.

## Crates

| Crate | |
|---|---|
| [`kasapay`](crates/kasapay) | Facade. Providers behind features. |
| [`kasapay-core`](crates/kasapay-core) | `Money`, `Charge`, `Error`, `Provider`, `Webhook`. No network, no HTTP client. |
| [`kasapay-stripe`](crates/kasapay-stripe) | Over [`async-stripe`](https://github.com/arlyon/async-stripe). |
| [`kasapay-iyzico`](crates/kasapay-iyzico) | Three iyzico APIs — `classic`, `in_store`, `terminal` — plus Link, Subscription, Mass Payout and Onboarding. |
| [`kasapay-paytr`](crates/kasapay-paytr) | Hosted form, status, refund, instalment rates, payment notice. |
| [`kasapay-mollie`](crates/kasapay-mollie) | Payments API: hosted checkout, holds, captures, refunds, mandates. |
| [`kasapay-paypal`](crates/kasapay-paypal) | Orders v2 and Payments v2: create, read, capture, hold, release, refund. |

## Design principles

**No card number goes through kasapay.** There is no field for one on
`ChargeRequest` and no type in this workspace that can hold one. Every
provider here has a way of taking a payment that never sends a number through
the caller's process, and that is the way each adapter implements. A returning
customer is an `InstrumentId`: the provider keeps the card and hands back a
handle.

**Amounts are integers.** `Money` counts minor units and there is no `f64`
anywhere. `Money::parse("149.905", Currency::Try)` is an error rather than a
rounding, and a decimal on the wire is written from the integer. JPY has no
minor unit, KWD has three, and both are tested. `checked_add` and
`checked_sub` refuse to mix currencies, and `partial_cmp` answers `None`
across them. There are no `+` and `-` operators, because they would have to
panic on a mismatch.

**A provider that cannot do something says so before the wire.** An
unsupported currency, an idempotency key the provider has no mechanism for, a
saved instrument it cannot charge: each is `ErrorKind::Unsupported` raised
before a socket opens, never a field quietly dropped.

**`Charge::raw` keeps the provider's own answer whole**, so everything kasapay
does not model is still reachable.

**A release is its own act, not a payment state.** `cancel` answers a
`Release` rather than a `Charge`, because three of the four providers that
hold funds answer no payment at all — Mollie `202 Accepted` with no body,
PayPal `204` with no body, iyzico a reversal carrying the bank's own
reference. `ReleaseState` says whether the money is gone or the provider has
only taken the request, which is the difference between telling a payer their
hold is released and being wrong about it.

**Differences are stated rather than smoothed over.** `Status`, `Capabilities`
and each adapter's own documentation carry the per-provider tables: which
statuses a provider can produce, which currencies it settles in, what its
`lookup` can and cannot find.

## Currencies

`Currency` names 119 currencies. One is named when ISO 4217 defines it, its
minor unit is exactly two decimal places, and some provider here settles in
it — plus the nine the library shipped with, whatever their exponent.

The two-decimal rule is a safety rule. Zero- and three-decimal currencies are
where a provider's reading and ISO's diverge, and being wrong about one is a
payment out by a factor of a hundred. A currency `match` may carry a wildcard
arm only where that arm refuses.

## Specs

[`specs/`](specs) records what each provider said its API was, dated. Stripe,
Mollie and PayPal publish an OpenAPI document; iyzico's is reassembled from
the fragments embedded in their documentation, and PayTR's is a record of
their field tables, including which fields enter the token hash.

Mollie's is recorded without a copy of it: theirs is CC-BY-NC-SA-4.0 and this
repository is MIT, so `specs/mollie/` holds a dated meta and two hashes, and
`scripts/fetch_mollie.py --write-document` writes the document itself to a
gitignored path.

A weekly job refetches all five and opens a pull request when anything moved —
[`specs/README.md`](specs/README.md).

## Not verified against a live account

Nothing here has been run against live credentials.
[`UNVERIFIED.md`](UNVERIFIED.md) lists every reading taken from a document
rather than observed, grouped by the account that would settle it, each with
the one call that settles it.

## Examples

Nine, under [`crates/kasapay/examples`](crates/kasapay/examples), all compiled
in CI so they cannot drift.

| Example | |
|---|---|
| `portable_charge` | Two providers, one `ChargeRequest`, no branch naming either. |
| `stripe_charge` | A Stripe charge and refund. |
| `iyzico_checkout` | A hosted checkout form, end to end. |
| `iyzico_link` | An iyzico Link sold and taken down. |
| `iyzico_subscription` | Product, plan, and the form that signs somebody up. |
| `paytr_payment` | A hosted payment and the notice PayTR posts back. |
| `mollie_payment` | A payment beside a hold. |
| `paypal_order` | An order opened, then captured once approved. |
| `webhook` | Three deliveries: signed, unsigned, and an undocumented status. |

`webhook` needs no credentials and talks to nobody:

```sh
cargo run -p kasapay --features paytr --example webhook
```

The rest take the provider's own keys:

```sh
STRIPE_SECRET_KEY=sk_test_… cargo run -p kasapay --features stripe --example stripe_charge
```

## Adding a provider

Implement `Provider` in a `kasapay-<name>` crate, add a spec fetcher under
`scripts/`, add a feature to `kasapay`. Tests run against `wiremock` — no
credentials, no network.

## Developing

```sh
cargo fmt --all
cargo clippy --workspace --all-targets --all-features
cargo nextest run --workspace --all-features
cargo test --workspace --all-features --doc   # nextest skips doctests
```

Rust 1.88 or later. Open an issue before anything larger than a fix.

## Licence

MIT — see [LICENSE](LICENSE).
