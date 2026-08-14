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
merges them into one document per part of the API:

    <group>/<YYYY-MM-DD>.yaml   that part of the API as one document
    <group>/latest.yaml         symlink to the newest
    <YYYY-MM-DD>.index.json     what was swept, every operation, every conflict

As of 2026-08-14: 314 pages swept, **96 operations across 11 groups** —
`payment`, `subscription`, `terminal-host`, `in-store`, `iyzilink`, `mass`,
`cardstorage`, `softpos`, `onboarding`, `reporting`, `agent`.

Each operation keeps an `x-iyzico-source` pointing at the page it came from.

### Two languages, neither complete

The docs exist in Turkish and English and they do not cover the same endpoints.
The whole In-Store v3 API is documented **only in Turkish**; the In-Store OAuth
refresh **only in English**. So both are swept, the union is the coverage, and
where an endpoint appears in each the English fragment wins so the prose in
`specs/` is readable by anyone. The index lists what was Turkish-only.

Grouping comes from the API path rather than the documentation URL, because the
same API is filed under `urunler/abonelik` in Turkish and `products/subscription`
in English — grouping by page filed everything twice under two names.

### Authentication is mostly undocumented

Only 16 of the 96 operations declare a security scheme at all: Bearer JWT for
Terminal Host and the In-Store OAuth flow, `x-api-key` for In-Store, and Basic
auth for the OAuth token endpoint itself. The other 80 — including every
ordinary card payment — say nothing about how a request is authenticated.

So `specs/` says nothing either. An earlier version of this script applied
In-Store's headers (`x-api-key`, `x-secret-key`, `x-merchant-id`) to all 96 as
a global scheme; those came from prose on one overview page, not from any
fragment, and stating them as fact for the whole API was an invention. Each
group's index now lists the operations iyzico documents no authentication for.

Where an adapter needs to know, read iyzico's own integration guide or ask
them. Do not read it out of these files.

### Validation

`scripts/validate_specs.py` checks every document against the OpenAPI schema and
against a list of types that are not OpenAPI's. CI runs it on every push and
again after each weekly refetch, because these files are assembled by a script
and every bug in that script lands here as a document that looks plausible and
is wrong. Three have already.

### What the script repairs

- Type names that are not OpenAPI types. Java's — `BigDecimal`, `Long`,
  `Integer`, `Double`, `Boolean`, `String` — and iyzico's own lowercase ones.
  `decimal` alone appears 864 times, on nearly every money field in the API.
  An amount becomes `string`/`decimal` so no reader is tempted to put it
  through a float.
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
