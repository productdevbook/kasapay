# kasapay

One payment API in Rust, over any payment provider.

Write against one trait; the provider becomes a deployment decision. Stripe,
iyzico, PayTR and Mollie ship today — PayPal, Adyen, Param, Craftgate and the
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
| `kasapay-iyzico` | three iyzico APIs: `in_store`, `terminal` and `classic` |
| `kasapay-paytr` | PayTR's hosted form, status, refund, payment notice |
| `kasapay-mollie` | Mollie's Payments API: hosted checkout, holds, captures, refunds |

**The shared trait is `charge`, `charge_status`, `capture` and `cancel`**, with
`capabilities()` saying which of them a provider actually does — iyzico's
In-Store flow takes the money at authorisation and has no capture step, and a
caller planning a checkout needs to know that before it has a payment. Refunds,
webhooks and saved cards live on the providers for now — they enter the trait
once more than two providers have been written against them, not before.

**iyzico** is three APIs that barely resemble each other, and no two of them
authenticate the same way. `in_store` is the counter flow, three plain headers,
lira only. `terminal` is a cash register driving a physical POS device: an
OAuth2 bearer token that expires, and a call that returns when somebody has
presented a card. `classic` is everything else, signed with `IYZWSv2`: the
hosted checkout form, refunds, cancel, stored cards and charging one, BIN
lookup — with `iyzilink`, pay-by-link, `subscription`, the products and plans a
recurring charge is sold out of, and `mass`, money going out rather than coming
in, over the same client. Forty-seven of iyzico's ninety-six documented
operations — `scripts/coverage.py` says which — and none of them touches a
card number.

**Every iyzico response iyzico signs is verified.** They sign the money-moving
ones with an HMAC over selected fields of the reply, and kasapay refuses one
that does not match — or one that carries no signature at all, unless you say
otherwise. A forged callback is how a shop ships against a payment that never
happened. Where they sign nothing there is nothing to check, and the module
says so: the classic cancel, every one of iyzilink's seven, every one of
subscription's twenty-four, every one of mass payout's six, and every one of
the Terminal API's fourteen.

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
- **Mollie takes neither lira nor Kuwaiti dinar**, which is to say the home
  currency of the two Turkish providers here and the one currency with three
  decimal places. Same answer, same place: `Unsupported`, before a socket
  opens. It also requires a description and a return address on every payment,
  both of which `ChargeRequest` calls optional, and it decides at creation
  whether a payment is captured or held — so a hold is `Mollie::authorize`
  rather than `charge`, and releasing one is `release_authorization` rather
  than `cancel`, because Mollie answers that call `202` with no body for a
  trait method that has to return a `Charge`.
- The hosted checkout form does **not** go through `Provider::charge`. It needs
  a buyer's identity number, two addresses and an itemised basket, none of
  which belongs in `ChargeRequest`. The trait can express what every provider
  answers; it cannot express what each one demands. A form the payer has not
  finished has no payment id either, only its own `classic::FormToken`, and
  `classic::Client::checkout_result` is what takes one — `charge_status` reads
  a finished payment by its id. An identifier says what it names as well as who
  issued it, so handing one to the other's call does not compile.

## No card number goes through kasapay

There is no field for one on `ChargeRequest` and no type in this workspace that
can hold one. A server that touches a card number is in PCI DSS scope on the
merchant's longest self-assessment rather than its shortest, and a library that
makes it easy to put one in a struct makes it easy to end up there without
noticing.

Every provider here has a way of taking a payment that never sends a number
through the caller's process, and it is the way kasapay implements: iyzico
hosts its checkout form and PayTR its payment page; Stripe's `pm_…` is made by
Stripe.js in the payer's browser; Mollie only ever redirects.

A returning customer is `InstrumentId` — the provider keeps the card and hands
back a handle, and the handle is what the payment carries.
`iyzico::classic::Client::pay_with_saved_card` is the first of these:
`POST /payment/auth`, the same endpoint an ordinary card payment uses, with the
`cardUserKey` and `cardToken` where the number would be. `saved::Card` has no
field for a number and refuses a value that is one by shape.
`Capabilities::saved_instruments` says which providers can do this before there
is a payment to ask about.

**The card gets into the vault through the hosted form, not through an API
call.** `POST /cardstorage/card` wants `cardNumber`, `expireMonth`,
`expireYear` and `cardHolderName`, and `registerCard: 1` stores the card a
payment already carries — neither is in this crate. iyzico's checkout form
offers the payer a save-my-card box instead, and answers the `cardUserKey` and
`cardToken` on its result: `checkout::CheckoutFormBuilder::card_user_key` sends
the key so a payer's cards stay under one, and `Charge::raw` at `/cardUserKey`
and `/cardToken` is where the pair comes back. Neither field is in `specs/` —
iyzico's documentation of that request and that response mentions neither, and
their own SDKs send and read both. Stripe is the same shape from the other
side: the `pm_…` is made by Stripe.js in the browser, never on the server.

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

`specs/` records what each provider said its API was, dated. Stripe and Mollie
publish an OpenAPI file; iyzico's is reassembled from the fragments embedded in
their documentation, and PayTR's is a record of their field tables — including
which fields enter the token hash, which is what signs a request.

**Mollie's is recorded without a copy of it.** Theirs is licensed
CC-BY-NC-SA-4.0 and this repository is MIT, and a non-commercial share-alike
file inside an MIT tree is a restriction on exactly the people this licence
invites. So `specs/mollie/` holds a dated meta and two hashes, and
`scripts/fetch_mollie.py --write-document` is how you read the document itself.

A weekly job refetches all four and opens a PR when anything moved. A lost
field is the line to read first: a provider withdrawing one and the script
dropping one look identical in a diff, and only one of those is fine —
[`specs/README.md`](specs/README.md).

## Adding a provider

Implement `Provider` in a `kasapay-<name>` crate, add a spec fetcher under
`scripts/`, add a feature to `kasapay`. Tests run against `wiremock`; no
credentials, no network.

## Examples

`crates/kasapay/examples/` — a Stripe charge and refund, an iyzico hosted
checkout form end to end, an iyzico Link sold and taken down, a PayTR hosted
payment with the notice it posts back, and a Mollie payment beside a Mollie
hold. All five compile in CI, so they cannot drift from the API the way a
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
