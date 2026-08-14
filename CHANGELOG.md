# Changelog

What changed, and what it costs a caller who upgrades. Kept by hand, in the
order releases happen, newest first.

## Unreleased

These changes break code written against 0.0.1. All are cheap to follow and
all are the kind 0.0.x exists to make.

### Breaking

- **`classic::Client::refund`, `refund_transaction` and `cancel` take a
  `reason`.** `Option<&classic::Reason>`, and `None` sends what they sent
  before — every existing call keeps its meaning by gaining one argument.

- **`Provider` gains `capture`, `cancel` and `capabilities`, none with a
  default.** A provider outside this workspace has to answer all three. No
  default on purpose: an adapter that cannot capture has to say so rather than
  inherit an answer that happens to be wrong for it, and a capability that says
  yes over a call that then fails is a bug in the adapter.

- **`Charge` carries `order_amount`.** `Charge::amount` now means what the
  payer is charged, and `order_amount` what the goods came to — they differ
  under an instalment surcharge, and two adapters were dropping one of the
  pair. `None` means the provider does not say, not that the two are equal.

- **`Charge::raw` is a `Raw`, not a `serde_json::Value`.** Its old type put
  serde_json in the public API of every provider adapter, including ones
  written outside this workspace, and left a provider that answers XML nowhere
  to put its body. `charge.raw["field"]` becomes
  `charge.raw.text_at("/field")`, or `charge.raw.json()` for the whole thing.
- **`kasapay_iyzico::{Iyzico, Config}` are `kasapay_iyzico::in_store::{Client, Config}`.**
  The crate speaks two iyzico APIs now and one flat namespace could not name
  both.

### Added

- **`PayTr::bin_details`**, PayTR's BIN service: the bank, network, company-card
  flag, non-3-D permission and instalment programme behind the first 6 or 8
  digits of a card number. A BIN PayTR has no record of is `Ok(None)`, not an
  error. Its token hashes `bin_number + merchant_id` before the salt — the BIN
  first, unlike every other PayTR call. `/odeme/taksit-oranlari` is deliberately
  not wrapped alongside it: PayTR documents the request and its hash but never
  what one entry of the `oranlar` payload looks like.
- Every GitHub action is pinned to a commit rather than a tag. A tag can be
  moved, and one workflow holds the crates.io publish token.
- A dependency audit. `deny.toml` refuses a known vulnerability, a non-permissive
  licence, a wildcard version or a source outside crates.io, checked on every
  push and again daily — an advisory lands against a tree that has not changed.

- **`Stripe::capture`, whole or partial.** Stripe leaves `amount` at what was
  authorised and reports a partial capture in `amount_received`, so
  `Charge::amount` reads the latter where the two differ — the trait promises
  the amount captured.

- **`kasapay-paytr`, a third provider.** PayTR's hosted form, status query,
  refund and payment-notice verification. It has no payment id of its own — a
  payment is named by the merchant's order reference — so that reference must
  never be reused.

- Two examples under `crates/kasapay/examples/`, built by CI so they cannot
  drift from the API.
- **A reason on an iyzico refund and cancel.** `classic::ReasonCode` is the
  four iyzico documents — `Other`, `Fraud`, `BuyerRequest`, `DoublePayment` —
  and `classic::Reason` pairs one with the optional free text beside it.
  iyzico only accepts a description alongside a reason, so the description
  hangs off the reason rather than sitting in a second `Option` a caller could
  fill on its own. `Fraud` and `DoublePayment` are what a shop tells its
  acquirer, and they land in chargeback and reconciliation reporting.
- `classic::Reversal` is exported. It is what all three of those answer, and
  it was a public type in a private module that no caller could name.
- `classic::Client` implements `Provider` for reading: `charge_status` takes
  the checkout form's token. `charge` answers `Unsupported` and names
  `start_checkout_form`, because the form needs more than `ChargeRequest`
  carries.
- `Stripe::refunds`, the refunds taken off a payment. A refunded PaymentIntent
  still reads `succeeded`, so "how much has come back" is this list summed
  rather than a status — and every provider has to be able to answer it. It
  follows Stripe's cursor to the end, because the default page is ten and
  stopping there would undercount.
- `Stripe::refund` and `Stripe::cancel`, which had no counterpart to iyzico's.
  Refunding in a currency the payment was not in cannot be refused before
  sending — Stripe takes a bare integer — so it is caught against the answer,
  and the error says the money has already moved.
- `classic`, iyzico's other API — the hosted checkout form, refunds, cancel,
  stored cards, BIN lookup. Twelve of iyzico's ninety-six documented
  operations, and none of them touches a card number.
- `IYZWSv2` request signing, and verification of the signature iyzico puts on
  every money-moving response. An unsigned response is refused unless
  `classic::Config::allow_unsigned` says otherwise.
- `in_store::Client::decrypt_callback`, which is how an In-Store payment
  actually finishes. Before it, the only way to learn an outcome was to poll.
- `Money::checked_add`, `checked_sub`, `is_zero`, and `PartialOrd`. No `+` or
  `-` operators: they would have to panic on a currency mismatch.
- `Currency::Jpy` and `Currency::Kwd`, which have zero and three decimal
  places. Every currency before them had two, so nothing exercised the rest of
  the arithmetic.
- `ErrorKind::Untrusted`, for a response that cannot be shown to be the
  provider's. Never retryable, and never to be acted on.
- `kasapay::async_trait`, so implementing `Provider` does not mean guessing
  which version of `async-trait` the trait was defined with.
- A request timeout on both adapters, 30 seconds by default. There was none,
  so a provider that stopped answering hung the caller forever.

### Fixed

- Stripe errors carry the decline code now — `insufficient_funds` rather than
  nothing — and the message is Stripe's own sentence rather than a Debug dump
  of their error struct.

- iyzico reported every refusal as a bad request. A declined card is now
  `Declined` and a bank timeout is `Provider` and retryable, from iyzico's own
  code list. Their `Retry` column is deliberately not used: it says `true` for
  "Email is mandatory", which means the shopper can correct it, not that the
  same request may succeed.

- PayTR reported every refund refusal as a flat rejection. Two of its
  documented codes say to try again later — the refund service being locked,
  and an insufficient balance — and are now retryable, so a caller's retry
  loop no longer gives up on a refund that would have gone through.

- `ChargeRequest::idempotency_key` was accepted and dropped by every adapter.
  Stripe sends it now; iyzico refuses the request rather than pretending.
- `Currency` no longer maps blindly onto Stripe's. Stripe settles in no
  three-decimal currency at all, so a Kuwaiti dinar is `Unsupported` rather
  than quietly turned into something else.

## 0.0.1 — 2026-08-14

First release. `Money`, `Charge`, `Error` and the `Provider` trait, with
Stripe over `async-stripe` and iyzico's In-Store API behind them.
