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
The whole In-Store v3 API is documented **only in Turkish**; the Terminal API's
token-refresh service **only in English**. So both are swept and the union is
the coverage. The index lists what was Turkish-only.

Where an endpoint appears in each, the two are not the same fragment in two
languages. They differ in substance: the cancel-and-refund page carries `reason`
and `description` only in Turkish, and the In-Store refund field is
`refundAmount` in one language and `refundPrice` in the other. So the fragment
documenting the most fields is the base — English breaking a tie, because its
prose is what most readers get — and every field any other fragment documents
and the base lacks is grafted onto it. Each graft is named in the index.

Grafting is per operation rather than per page, because pages overlap without
matching: the checkout form's own page describes initialising *and* querying
while a second page describes only the query, and the two disagree about where
`currency` sits.

Grouping comes from the API path rather than the documentation URL, because the
same API is filed under `urunler/abonelik` in Turkish and `products/subscription`
in English — grouping by page filed everything twice under two names.

### The currency list is per product, not per company

There is no one list of currencies iyzico takes. Counting every `currency*`
property that carries an `enum`, across all eleven groups, there are three:

| currencies | where |
|---|---|
| TRY USD EUR GBP RUB CHF NOK | `iyzilink`, `onboarding` |
| TRY USD EUR GBP CHF NOK | `payment`, `cardstorage`, `reporting` |
| TRY USD EUR | `subscription`, `terminal-host`, `mass`, two schemas in `payment` |

So a link can be priced in roubles and a payment cannot; a subscription plan
can be priced in three currencies and a link in seven. A new module has to read
its own pages rather than inheriting another's list, and `kasapay-core`'s
`Currency` naming a currency is not the same as iyzico taking it for a given
product.

Two inside `payment` contradict the rest of `payment`: `PostAuthResponse` and
`ConvertedPayout` allow only TRY, USD and EUR while the authorisation that
produces them allows six. A capture in sterling would answer a currency its own
response schema forbids.

Regenerate it after a refetch with `python3 scripts/currency_enums.py`.

### How authentication is declared

Two ways, and counting only one of them is how this file twice said something
false. Per operation, from the dated index's `authentication` block:

| | operations |
|---|---|
| a `securityScheme` — Bearer JWT, `x-api-key`, Basic | 20 |
| an `Authorization` **parameter**, described as an `IYZWSv2` signed hash | 67 |
| neither | 9 |

The 67 are the classic API. They declare the header as an ordinary parameter
rather than a security scheme, which is unusual and easy to miss — an earlier
version of this file reported them as documenting no authentication at all.

Of the 9 that declare neither, 5 are In-Store, which authenticates with three
plain headers described in prose on its overview page, 3 are softpos, and one
is `/in-store/oauth2/authorize`, which is the Terminal API's rather than
In-Store's — see below.

These counts move when the fragments do. They read 18 / 67 / 11 until grafting
what each language alone documents brought a `security` block onto
`/v3/in-store/payment/init` and `/v3/in-store/payment/refund`, which had none.
Recount from the dated index rather than trusting the table.

The scheme itself — how the signature is computed — lives on
[its own page](https://docs.iyzico.com/en/getting-started/preliminaries/authentication/hmacsha256-auth),
which carries no fragment and so is not in `specs/` at all.
`kasapay-iyzico` implements it in `signing.rs`.

### The fragments are not the whole documentation

Everything under `specs/iyzico/` comes from the OpenAPI fragments embedded in
iyzico's pages. Those pages also carry prose, worked examples and links to
iyzico's own SDKs, and **none of that is here**. So a field iyzico documents
only in a sentence, or only by sending it in their PHP SDK, is missing from
these files and its absence means nothing.

The case that proved it, from #97: the checkout form's `CFInitializeRequest`
carries no `cardUserKey` and no `registerCard`. Reading only this directory,
the conclusion is that iyzico offers no way to fill their card vault without
handling a card number — which is wrong, and would have cost the crate its
whole saved-card story. iyzico's SDKs send `cardUserKey` on that call, the
`checkoutFormContent` it answers sets `registerCardEnabled`, and the form's
result reads `cardToken` and `cardUserKey` back. `PaymentCardSaved` in
`specs/iyzico/payment/` documents the pair; nothing documents where it comes
from.

So: **`specs/` is evidence that iyzico said something, never evidence that they
did not.** Before concluding an endpoint cannot do a thing, read the page — its
URL is on the operation as `x-iyzico-source` — and look at what their SDKs
send.

### Validation

`scripts/validate_specs.py` checks every OpenAPI document against the OpenAPI
schema, against a list of types that are not OpenAPI's, and against a null
sitting where a string belongs — which is what a fragment carries when its
author left the field blank. A document that is not OpenAPI, such as the PayTR
record, is skipped by name rather than failed. CI runs it on every push and
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

### `/in-store/oauth2/*` is the Terminal API's login, not In-Store's

`in-store/latest.yaml` also carries `/in-store/oauth2/authorize`,
`/in-store/oauth2/token` and `/in-store/oauth2/token/refresh`. They are filed
there because grouping is by path and theirs begins `/in-store`. They belong to
a different product.

All three come from one page,
[Login](https://docs.iyzico.com/en/products/physical-pos/terminal-api-integration/login.md),
under **Physical POS → Terminal API Integration**. Their fragments say
`info.title: Terminal API – Outside Flow`, `version: 1.0.3` — the same document
as the VUK 509 fragments that define `/v2/terminal-host/*`, and nothing like the
`iyzico In-Store API` that titles every CepPOS fragment. iyzico's description of
the `access_token` they issue says it is "used as Bearer Token in Terminal Host
services", which is what those `/v2/terminal-host/*` operations declare, in an
integration where a cash register drives a physical POS device by
`deviceUniqueId`. The Turkish page opens by saying so outright: *Terminal API
servislerine erişim sağlamak için iyzico OAuth2 kimlik doğrulama yapısı
kullanılmaktadır.*

Nothing on iyzico's CepPOS App2App pages — the In-Store API — mentions OAuth,
in either language, and no other page in the sweep mentions `/in-store/oauth2`.

Two details follow from the same source and are not evidence of anything else:

- The paths carry no version segment because those fragments declare a bare
  `https://api.iyzipay.com` server, where the In-Store ones declare
  `/v3/in-store` and the Terminal Host ones `/v2/terminal-host`.
- `token/refresh` is English-only because the Turkish Login page documents
  `authorize` and `token` and then says an expired token means logging in
  again.

## PayTR — `specs/paytr/`

PayTR publishes no machine-readable description of any kind, so this is not one.
`scripts/fetch_paytr.py` sweeps the API pages listed in their sitemap, in both
languages, and records the field table from each:

    <YYYY-MM-DD>.yaml   every documented field, per page
    latest.yaml         symlink to the newest

A row is the field's name and type, whether it is mandatory, **whether it enters
the token hash**, its description and its constraints. That third column is why
this file exists: the hash is what signs a request, and a field silently
entering or leaving it is a signing failure that reads like bad credentials.

**The order of the rows is not the order of the hash.** The iFrame API's table
lists `currency` fifth; PayTR's own sample code signs it ninth, after
`max_installment`, which is what `kasapay-paytr` does. The table says which
fields are signed. Only their sample code says in what order, and nothing
machine-readable says either.

Each page also carries a digest over its tables rather than over the page, so a
changed footer is not reported as a changed API.

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
    python3 scripts/fetch_paytr.py

All three take an optional `YYYY-MM-DD` argument.

    python3 scripts/compare_specs.py --against-git origin/main

says what a change did to the fields the specs carry — which a diff of a few
thousand reordered YAML lines does not.
