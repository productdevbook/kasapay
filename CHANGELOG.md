# Changelog

What changed, and what it costs a caller who upgrades. Kept by hand, in the
order releases happen, newest first.

## Unreleased

Two changes break code written against 0.0.1. Both are cheap to follow and
both are the kind 0.0.x exists to make.

### Breaking

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

- **`kasapay-paytr`, a third provider.** PayTR's hosted form, status query,
  refund and payment-notice verification. It has no payment id of its own — a
  payment is named by the merchant's order reference — so that reference must
  never be reused.

- Two examples under `crates/kasapay/examples/`, built by CI so they cannot
  drift from the API.
- `classic::Client` implements `Provider` for reading: `charge_status` takes
  the checkout form's token. `charge` answers `Unsupported` and names
  `start_checkout_form`, because the form needs more than `ChargeRequest`
  carries.
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

- `ChargeRequest::idempotency_key` was accepted and dropped by every adapter.
  Stripe sends it now; iyzico refuses the request rather than pretending.
- `Currency` no longer maps blindly onto Stripe's. Stripe settles in no
  three-decimal currency at all, so a Kuwaiti dinar is `Unsupported` rather
  than quietly turned into something else.

## 0.0.1 — 2026-08-14

First release. `Money`, `Charge`, `Error` and the `Provider` trait, with
Stripe over `async-stripe` and iyzico's In-Store API behind them.
