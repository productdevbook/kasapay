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

### How authentication is declared

Two ways, and counting only one of them is how this file twice said something
false. Per operation, from the dated index's `authentication` block:

| | operations |
|---|---|
| a `securityScheme` — Bearer JWT, `x-api-key`, Basic | 18 |
| an `Authorization` **parameter**, described as an `IYZWSv2` signed hash | 67 |
| neither | 11 |

The 67 are the classic API. They declare the header as an ordinary parameter
rather than a security scheme, which is unusual and easy to miss — an earlier
version of this file reported them as documenting no authentication at all.

Of the 11 that declare neither, 8 are In-Store, which authenticates with three
plain headers described in prose on its overview page, and 3 are softpos.

The scheme itself — how the signature is computed — lives on
[its own page](https://docs.iyzico.com/en/getting-started/preliminaries/authentication/hmacsha256-auth),
which carries no fragment and so is not in `specs/` at all.
`kasapay-iyzico` implements it in `signing.rs`.

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
- A response with no `description`, which OpenAPI requires. Filled in by status
  class — "Successful, per iyzico's example response" — so it is obvious the
  text is ours rather than theirs.
- `enum` on a Parameter Object, which belongs inside its `schema` in 3.0.
- `in: path` on a parameter the path has no placeholder for. Read as a query
  parameter, which is what it is.
- `oneOf: [{type: string}, {type: null}]` — 3.1's spelling of nullable.
- A property indented beside `items` rather than inside them. Moved in rather
  than dropped, since dropping loses a documented field.
- One `operationId` reused across two operations.

Every one of those is recorded in the dated index against the page it came
from, so a repair is never silent.
- Paths are prefixed with the base path from the fragment's own `servers`, so
  `/payment/init` under a `/v3/in-store` server becomes
  `/v3/in-store/payment/init` and does not collide with another product's.
- One schema name meaning two different shapes on two pages. Both are kept, the
  second as `Name2`, and the index records it. Dropping either would silently
  lose an endpoint's request body — `ErrorResponse` alone is redefined dozens
  of times.

Where two pages document the same operation differently, the first wins and the
index says so. Read `notes` in the index before trusting a contested operation.

### `/v2/in-store/crypt/decrypt` is not a second version to choose from

`in-store/latest.yaml` carries both `/v3/in-store/crypt/decrypt` and
`/v2/in-store/crypt/decrypt`. The v2 one comes from a single fragment on the
page titled *In-Store API V3*, whose six sibling fragments all declare a
`/v3/in-store` server while that one declares `/v2/in-store`; all seven say
`version: 3.0`. Since a path is prefixed with its own fragment's server, that
one stale URL produced a second path.

v2 does exist — as a separate, older integration, documented in prose with no
fragments, at `/v2/in-store/user-info/list`, `/v2/in-store/payment` and
`/v2/in-store/payment/refund`. None of those are in `specs/` for want of a
fragment to sweep. `kasapay-iyzico` speaks v3.

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
