# Provider API descriptions

What each provider said its API was, on the day it was asked. A record, not a
contract. Nothing in `crates/` is generated from these — they exist so a change
upstream shows up as a diff here before it shows up as a failure in production.

Four of the five keep the description itself. **Mollie's is recorded without a
copy of it**, because theirs is licensed CC-BY-NC-SA and this repository is
MIT; `specs/mollie/` holds a dated meta and two hashes instead, and the section
below says what that buys and what it costs. PayPal's document is Apache-2.0,
permissive like Stripe's MIT, so its subset is kept the same way Stripe's is.

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

### Not all of it is iyzico's API

Five of the ninety-six operations are somebody else's. The `agent` and
`softpos` groups are **Paynet's** PayPOS API, documented on iyzico's site as
part of their offering, and their fragments say so: their own `servers` block
names `api.paynet.com.tr` and `pts-api.paynet.com.tr`, and their `info.title`
is `PayPOS (Paynet) API`.

The merge used to write iyzico's two hosts over the whole document, so those
five said they were served from `api.iyzipay.com` — a caller building from the
spec would have called the wrong company. A fragment that names a host of its
own now keeps it, carried onto the operation where OpenAPI's per-operation
`servers` belongs, and the dated index names each one.

So: read the operation's own `servers` before its group's. Nine of the eleven
groups have none, which means iyzico's, and two do.

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

softpos's 3, and `agent`'s 2 next to them (counted above as classic-shaped
`Authorization` parameters, because that is the field's name — see
`kasapay_iyzico::agent`'s module documentation for why the name is where the
resemblance to the classic API ends), are also where the merged documents
mislead about *where* a request goes, not only how it authenticates. Every
one of the five fragments declares its own `servers` block, and it is not
iyzico's: `https://api.paynet.com.tr` / `https://pts-api.paynet.com.tr`,
titled `"PayPOS (Paynet) API"`. `scripts/merge_iyzico.py`'s `merge()` does not
carry a fragment's own `servers` into the document it assembles — it always
writes the constant iyzico pair — so `specs/iyzico/agent/latest.yaml` and
`specs/iyzico/softpos/latest.yaml` both show `api.iyzipay.com` at the top
level regardless. Read `x-iyzico-source` for these two groups rather than the
merged `servers` block.

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

## PayPal — `specs/paypal/`

    <YYYY-MM-DD>.meta.json   the API version, and a hash of the full upstream document
    latest.yaml              the subset kasapay-paypal maps, resolved to its schemas

PayPal publishes one OpenAPI document per API, in
[paypal/paypal-rest-api-specifications](https://github.com/paypal/paypal-rest-api-specifications),
under **Apache-2.0** — checked against `LICENSE` in that repository, since the
document's own `info` block carries no `license` field the way Mollie's does.
Apache-2.0 is permissive the way Stripe's MIT is, so the subset itself is kept
rather than thrown away: `scripts/fetch_paypal.py` fetches
`openapi/checkout_orders_v2.json`, cuts it to the three operations
`kasapay-paypal` maps — create an order, read it back, capture it — and rolls
the subset forward into `latest.yaml` the same way `fetch_stripe.py` does,
because Orders v2's `order` schema pulls in every `payment_source` PayPal
documents and a dated copy per fetch would buy diffs nobody can read.

### One path carries a fourth verb this crate does not implement

`/v2/checkout/orders/{id}` documents both `GET` and `PATCH` — reading an order
back, and editing one already created. `kasapay-paypal` implements only the
read, so `KEEP` in the fetcher names `(path, verb)` pairs rather than bare
paths: keeping the whole path item would have pulled `PATCH`'s JSON Patch
request schema into the subset and into `compare_specs.py`'s count of what
this crate maps, for an operation nothing here calls.

### What the document does not say

**Which currencies PayPal takes.** `currency_code` is described as "the
three-character ISO-4217 currency code" and enumerates nothing — the same gap
Mollie's document has. The list of twenty-five, and which have no minor unit,
is on PayPal's [currency codes reference][paypal-currencies] and nowhere
machine-readable in the OpenAPI document itself. `kasapay-paypal` maps seven of
them and refuses Turkish lira and Kuwaiti dinar before a request is built —
the same two Mollie refuses, because both are simply absent from PayPal's
list too.

**What a created or captured order's top-level `status` actually looks
like.** The schema declares the field and the `Prefer` header's own prose says
a minimal response includes it, but not one of the three documented example
responses this crate's operations answer with — not create, not read, not
capture — carries one. `kasapay-paypal`'s own documentation says what it reads
instead.

[paypal-currencies]: https://developer.paypal.com/api/rest/reference/currency-codes/

## Mollie — `specs/mollie/`

    <YYYY-MM-DD>.meta.json   the whole of what is kept

**This is the one provider whose document is deliberately not here.** Mollie
publishes a real OpenAPI 3.1 document — `specs.yaml` in
[mollie/openapi](https://github.com/mollie/openapi) — so nothing is
reassembled from documentation pages; `scripts/fetch_mollie.py` fetches it,
cuts it to the five paths kasapay maps, checks it, records what it said, and
throws it away.

### Why no copy is kept

`info.license` on Mollie's document says **CC-BY-NC-SA-4.0** — attribution,
non-commercial, share-alike — and this repository is MIT and says so to
everyone who forks or vendors it. A non-commercial share-alike file sitting
inside an MIT tree is a restriction on exactly the commercial users the licence
invites, and one they would not notice. Mollie's licence is theirs to set and
there is nothing wrong with it; it simply does not belong in here. Stripe's, by
contrast, is MIT, which is why theirs is kept.

Subsetting does not change that. A cut-down copy is still a copy.

### What is kept instead, and what it costs

`<date>.meta.json` carries the API version, the licence, the paths and
`operationId`s, the repairs below, the component counts, and two hashes:

| | moves when |
|---|---|
| `upstream_sha256` | anything in Mollie's whole 1.9MB document does |
| `subset_sha256` | one of the five paths kasapay maps does |

The second is the sharper of the two and it is the reason both are there: the
first moves whenever any of Mollie's eighty-seven paths changes, which is
often and mostly irrelevant.

What this costs is the field-level diff `compare_specs.py` gives the others —
it will not name a field Mollie withdrew, only say that something under the
five paths moved. `compare_specs.py` says so out loud rather than staying
silent about Mollie, because silence reads as "nothing changed".

What it keeps is the weekly job noticing the version or a hash moved, which is
most of why `specs/` exists.

### Reading the document yourself

    python3 scripts/fetch_mollie.py --write-document

writes `specs/mollie/<date>.yaml` — the same subset, around 400KB of the
upstream 1.9MB. `.gitignore` covers `specs/mollie/*.yaml` so it cannot be
committed by accident. Delete it when you are done, or do not; git will not
see it either way.

### Two things the document does not say

**Which currencies Mollie takes.** `currency` is described as "a
three-character ISO 4217 currency code" and enumerates nothing, anywhere in the
document. The list of twenty-seven, and the decimal places each has, is on
their [multicurrency page](https://docs.mollie.com/docs/multicurrency) and
nowhere machine-readable. `kasapay-mollie` maps seven of them and refuses lira
and Kuwaiti dinar before a request is built.

**How a webhook is authenticated.** It is not, and the document has no webhook
in it at all. Mollie posts one form field — the payment's id — to the address
the payment was created with, and nothing that proves the post was theirs.

### Mollie's own document is not quite valid OpenAPI, and the fetcher repairs it

Five of the parameters on the kept operations are a `$ref` with a `schema`
beside it. A Reference Object in OpenAPI 3.1 may carry `summary` and
`description` alongside its `$ref` and nothing else, so those five make the
document invalid and `openapi-spec-validator` refuses it outright — which is
how this was found, in CI, rather than by reading.

What Mollie plainly means is the shared parameter with that schema on top, so
that is what is built: the referenced parameter inlined and the siblings
overlaid. Nothing is dropped, and each repair is named in the dated meta under
`repaired_reference_objects`, so it is never silent — the same rule
`merge_iyzico.py` follows.

The same pattern **inside a schema** is left exactly as it is. `$ref` with
siblings is legal in JSON Schema 2020-12, which is what an OpenAPI 3.1 Schema
Object is, and there are thirty-three of them.

**The check moved into the fetcher when the document stopped being kept.**
`validate_specs.py` walks files under `specs/`, and there is no longer a Mollie
file for it to walk — so `fetch_mollie.py` validates the subset itself, before
writing the meta, and exits non-zero rather than recording a document it could
not check. It runs `openapi-spec-validator` when that is installed, and two
rules of its own whether it is or not: no Reference Object carrying more than
`$ref`, `summary` and `description`, and no `$ref` pointing at something the
cut left behind. Those two are what the cut and the repairs could plausibly get
wrong, and the first is exactly what caught these five. `checked` in the meta
says which of the two ran.

### What the fetcher does that the Stripe one does not

- **Loads YAML with the timestamp resolver switched off.** Several of Mollie's
  examples carry an unquoted date and one of them is `2023-02-29`, which is not
  a date. A plain `yaml.safe_load` raises on it before the document can be
  subset at all.
- **Follows `$ref` into four component sections**, not just `schemas`: Mollie's
  operations reference shared parameters, responses and examples as well.
  `securitySchemes` are copied in whole, since nothing `$ref`s them and how a
  request is authenticated is the first thing a reader wants.

`scripts/validate_specs.py` learned something here too, and it is worth keeping
even though it will never meet this document in CI. It reads every `type:` in a
file as a schema type, which held until Mollie — whose `_links` objects carry a
field literally called `type`, holding `application/hal+json`, inside example
payloads, and whose `x-speakeasy-pagination` extension carries one holding
`url`. Neither is OpenAPI. Example subtrees and `x-` extensions are now skipped
the way `securitySchemes` already was, so a `--write-document` copy passes, and
so will the next provider whose examples carry a field of that name.

## Refetching by hand

    pip install pyyaml openapi-spec-validator
    python3 scripts/merge_iyzico.py    # writes today's date
    python3 scripts/fetch_stripe.py
    python3 scripts/fetch_paytr.py
    python3 scripts/fetch_paypal.py
    python3 scripts/fetch_mollie.py    # meta only; --write-document for the rest

All five take an optional `YYYY-MM-DD` argument. `openapi-spec-validator` is
only needed by the Mollie one, which checks what it fetched rather than
committing something nothing will check.

    python3 scripts/compare_specs.py --against-git origin/main

says what a change did to the fields the specs carry — which a diff of a few
thousand reordered YAML lines does not.
