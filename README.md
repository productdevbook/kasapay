# kasapay

One payment API in Rust, over any payment provider.

Write against one trait; the provider becomes a deployment decision. Stripe,
iyzico, PayTR, Mollie and PayPal ship today — Adyen, Param, Craftgate and the
rest are the same shape, and a provider outside this repository is a
first-class one: implement `Provider`, name it with `ProviderId::new`.

```toml
kasapay = { version = "0.0.4", features = ["stripe", "iyzico"] }
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
| `kasapay-paytr` | PayTR's hosted form, status, refund, instalment rates, payment notice |
| `kasapay-mollie` | Mollie's Payments API: hosted checkout, holds, captures, refunds |
| `kasapay-paypal` | PayPal's Orders v2 and Payments v2: create, read, capture, hold, release, refund |

**The shared trait is `charge`, `charge_status`, `capture`, `cancel`,
`refund`, `lookup` and `instruments`**, with `capabilities()` saying which of them a
provider actually does — iyzico's In-Store flow takes the money at
authorisation and has no capture step, and a caller planning a checkout needs
to know that before it has a payment.

**`lookup` is for the call whose answer never arrived.** A charge that times
out is the one failure where nobody knows whether the money moved, and
`charge_status` cannot help — it takes the provider's own identifier, which is
what the lost reply never delivered. `lookup` is keyed by the caller's own
order reference instead, and `Ok(None)` means no record and a safe retry. Two
of the five can answer it; the other four say so, and say what to do instead.

**`Webhook` is the second trait, and it is `async` because verification is
not one mechanism.** Stripe signs the bytes it posts, PayTR signs three fields
of them, Mollie signs nothing at all and posts an identifier to read back, and
PayPal verifies a delivery for you over its own API. Four implementations,
three mechanisms, one `verify(&Delivery) -> Result<Event, Error>`. An unsigned
body never becomes an `Event`, and neither does a **doubly** signed one:
`Delivery::signed_header` refuses a delivery carrying two of a header the
signature depends on rather than picking one, because two claims about one
delivery mean something in front of the verifier read it differently; an event type the library does not model is
`EventKind::Other` and never an error, because refusing one earns days of
redeliveries for something nobody wanted.

**Four, not five: iyzico has no `Webhook` implementation**, and the reason is
that there is nothing to implement it from. The sweep in `specs/iyzico/` is
their whole documentation site in both languages, 96 operations, and not one
of them documents a delivery iyzico posts or how it is signed. Verifying a
body means knowing the mechanism, and inventing one would be the opposite of
what this library is for. What iyzico does document is the callback its hosted
form redirects to, which carries a token rather than a signed body —
`classic::Client::checkout_result` is what reads that.

**A refund is its own object, not a status on the payment.** `Status` has no
`Refunded` and will not grow one: Stripe leaves a refunded PaymentIntent
`succeeded`, so a caller branching on it would take the wrong path for every
Stripe payment. `Provider::refund` answers a `Refund` with its own identifier
where the provider issues one — iyzico issues none, and says so rather than
handing back a composed key in the field a real one lives in — and, at iyzico's
counter, its own `NextAction`, because there the payer has to approve the
refund in an app before the money moves.

**iyzico** is three APIs that barely resemble each other, and no two of them
authenticate the same way. `in_store` is the counter flow, three plain headers,
lira only. `terminal` is a cash register driving a physical POS device: an
OAuth2 bearer token that expires, and a call that returns when somebody has
presented a card — in both of the integrations iyzico offers for it, including
the one where the device issues the fiscal receipt and a refund names the lines
coming back rather than an amount. `classic` is everything else, signed with
`IYZWSv2`: the hosted checkout form, Pay with iyzico, pre-authorisation and
capture, refunds, cancel, stored cards and charging one, BIN and instalment
lookup — with `iyzilink`, pay-by-link, `subscription`, the catalogue and the
subscriptions sold out of it, `onboarding`, marketplace sellers and their
payouts, and `mass`, money going out rather than coming in, over the same
client. Eighty-eight of iyzico's ninety-six documented
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
- **PayPal's `Provider::cancel` always refuses**, and the reason is not that
  there is nothing to call. `/v2/checkout/orders` itself has no cancel or
  void, but PayPal's Authorizations resource does document one —
  `POST /v2/payments/authorizations/{id}/void` — for a hold `PayPal::authorize`
  and `PayPal::authorize_order` place. It is keyed by the authorization's own
  id, not the order's, and `cancel(&PaymentId)` cannot express that — the
  same shape `PayPal::capture_authorization` needing its own
  `AuthorizationId` has. This is the one provider here that shows `cancel`
  assuming every hold is reachable by the order id that opened it. Its
  `Provider::capture` is unconditional too, the other direction: every order
  needs an explicit capture call after the payer approves regardless of the
  intent it was created with, so `Capabilities::separate_capture` is `true`.
- **What one provider requires, another has never heard of.** iyzico's classic
  form refuses a payment without a buyer's identity number, an address and an
  itemised basket; PayTR refuses one without the payer's own IP; Stripe and
  Mollie ask for none of it. So `ChargeRequest` carries `buyer`,
  `billing_address`, `shipping_address` and `basket`, all optional, and an
  adapter that is not given one it needs answers `ErrorKind::InvalidRequest`
  **naming the field**, before a socket opens. The request that works
  everywhere is the one carrying what the strictest provider asks; swapping to
  a laxer one costs nothing, because the extra fields are ignored.
- **A hosted form still has settings the trait has no word for.** Holding the
  money rather than taking it, which instalment counts to offer, a
  `cardUserKey` that is not the customer reference — those are
  `classic::Client::start_checkout_form` and `PayTr::start_payment`, and
  `Provider::charge` builds the same request without them.
- A form the payer has not finished has no payment id, only its own
  `classic::FormToken`, and `classic::Client::checkout_result` is what takes
  one — `charge_status` reads a finished payment by its id. An identifier says
  what it names as well as who issued it, so handing one to the other's call
  does not compile.

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
`Provider::instruments` lists what a customer has on file, the same shape at
every adapter: an identity and something to show them choosing between saved
instruments, never a card number. Charging one is not the same shape twice —
`iyzico::classic::Client::pay_with_saved_card` is `POST /payment/auth`, the
same endpoint an ordinary card payment uses, with the `cardUserKey` and
`cardToken` where the number would be, but wants a buyer, two addresses and a
basket beside it; `saved::Card` has no field for a number and refuses a value
that is one by shape. So charging stays each adapter's own call, and
`Capabilities::saved_instruments` says which providers can do it before there
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

`specs/` records what each provider said its API was, dated. Stripe, Mollie and
PayPal publish an OpenAPI file; iyzico's is reassembled from the fragments
embedded in their documentation, and PayTR's is a record of their field
tables — including which fields enter the token hash, which is what signs a
request.

**Mollie's is recorded without a copy of it.** Theirs is licensed
CC-BY-NC-SA-4.0 and this repository is MIT, and a non-commercial share-alike
file inside an MIT tree is a restriction on exactly the people this licence
invites. So `specs/mollie/` holds a dated meta and two hashes, and
`scripts/fetch_mollie.py --write-document` is how you read the document itself.
PayPal's is Apache-2.0, permissive like Stripe's, so `specs/paypal/` keeps the
subset itself the same way `specs/stripe/` does.

A weekly job refetches all five and opens a PR when anything moved. A lost
field is the line to read first: a provider withdrawing one and the script
dropping one look identical in a diff, and only one of those is fine —
[`specs/README.md`](specs/README.md).

## Adding a provider

Implement `Provider` in a `kasapay-<name>` crate, add a spec fetcher under
`scripts/`, add a feature to `kasapay`. Tests run against `wiremock`; no
credentials, no network.

## One contract, checked the same way against every adapter

`Capabilities` is a promise about behaviour, and until `crates/kasapay/tests/
conformance.rs` nothing made the promise and the behaviour agree: each adapter
asserted its own answers in its own file, so a flag flipped in one place while
the method below it kept doing what it always did would have passed every test
in the workspace.

That file asks the same questions of all six clients through nothing but
`dyn Provider`, against a mock server that answers 500 to everything — which is
how a call that is genuinely implemented (it fails *there*) is told apart from
one that is refused (it never arrives):

- where a capability is `false`, the paired call answers `Unsupported`;
- where it is `true`, that call does **not**, and a request reaches the wire;
- a refusal costs nothing — not one byte;
- every error names the provider it came from;
- a charge missing fields is `InvalidRequest`, never `Unsupported`, because
  `Unsupported` means "never, for anyone" and this is "not with these fields".

Six clients, five `ProviderId`s: `classic` and `in_store` are two APIs of
iyzico's and both answer `iyzico`.

## What is read off a document rather than observed

Nothing here has been run against a live account, and
[`UNVERIFIED.md`](UNVERIFIED.md) is the list of places where that matters: a
status iyzico infers rather than states, a field their prose and their schema
spell differently, a PayTR response nobody has ever seen the shape of. Each
entry says what the library does today, why the reading might be wrong, and the
one call that would settle it.

Where being wrong could take money twice or refund it twice, the implementation
takes the side that fails loudly or does nothing at all — sending both spellings
of an optional field, refusing an idempotency key rather than dropping it,
reporting a payment as pending rather than paid. That is what makes the list a
set of readings to confirm rather than a set of bugs.

## Examples

`crates/kasapay/examples/` — a Stripe charge and refund, an iyzico hosted
checkout form end to end, an iyzico Link sold and taken down, a PayTR hosted
payment with the notice it posts back, a Mollie payment beside a Mollie hold,
a PayPal order opened and then captured once the payer has approved it, a
subscription sold — product, plan and the hosted form that signs somebody up —
and a webhook handler answering three deliveries: one the provider signed, one
nobody signed, and one carrying a status the provider does not document. All
eight compile in CI, so they cannot drift from the API the way a README snippet
can.

The webhook one is the one to read before writing a handler. It needs no
credentials and talks to nobody — `cargo run -p kasapay --features paytr
--example webhook` — and the thing it is about is the line that looks like a
missing error path: a delivery that does not verify is still answered `OK`,
because the provider retries anything else for days.

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
