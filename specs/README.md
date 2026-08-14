# Provider API descriptions

What each provider said its API was, on the day it was asked. A record, not a
contract. Nothing in `crates/` is generated from these — they exist so a change
upstream shows up as a diff here before it shows up as a failure in production.

`.github/workflows/spec-drift.yml` refetches weekly and opens a PR when
anything moved.

## iyzico — `specs/iyzico/`

iyzico publishes no OpenAPI file. Their documentation embeds one small OpenAPI
fragment per endpoint, so `scripts/merge_iyzico.py` sweeps every page in
[llms.txt](https://docs.iyzico.com/llms.txt), pulls the fragments out, and
merges them into one document per product area:

    <area>/<YYYY-MM-DD>.yaml   the area's whole API as one document
    <area>/latest.yaml         symlink to the newest
    <YYYY-MM-DD>.index.json    what was swept, every operation, every conflict

As of 2026-08-14: 166 pages swept, 69 carrying fragments, 165 fragments,
**155 operations across 16 areas** — payments, pre-auth, subscriptions,
marketplace, mass payout, card storage, physical POS, CepPOS, PayPOS, links,
reporting, instalment/BIN.

Each operation keeps an `x-iyzico-source` pointing at the page it came from.

### What the script repairs

- Java type names that are not OpenAPI types: `BigDecimal`, `Long`, `Integer`,
  `Double`, `Boolean`, `String`.
- A stray `detail` key sitting among an operation's `responses`.
- Paths are prefixed with the base path from the fragment's own `servers`, so
  `/payment/init` under a `/v3/in-store` server becomes
  `/v3/in-store/payment/init` and does not collide with another product's.
- One schema name meaning two different shapes on two pages. Both are kept, the
  second as `Name2`, and the index records it. Dropping either would silently
  lose an endpoint's request body — `ErrorResponse` alone is redefined dozens
  of times.

Where two pages document the same operation differently, the first wins and the
index says so. Read `notes` in the index before trusting a contested operation.

## Stripe — `specs/stripe/`

    <YYYY-MM-DD>.meta.json   the API version, and a hash of the full upstream spec
    latest.yaml              the subset kasapay maps, resolved to its schemas

Stripe's spec is around 7MB and a PaymentIntent transitively references most of
it — the filtered subset is still 1.7MB. So only metadata is dated:
`upstream_sha256` detects drift, `api_version` names it. The subset rolls
forward in place.

kasapay does not generate a Stripe client from this. `kasapay-stripe` wraps
[`async-stripe`](https://github.com/arlyon/async-stripe), regenerated weekly
from the same document by people who do it full time. The subset here is for
reading when a mapping looks wrong.

## Refetching by hand

    pip install pyyaml
    python3 scripts/merge_iyzico.py    # writes today's date
    python3 scripts/fetch_stripe.py

Both take an optional `YYYY-MM-DD` argument.
