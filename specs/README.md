# Provider API descriptions

What each provider said its API was, on the day it was asked. These are a
record, not a contract, and nothing in `crates/` is generated from them — they
exist so that a change upstream shows up as a diff here before it shows up as a
failure in production.

`.github/workflows/spec-drift.yml` refetches them weekly and opens a pull
request when anything moved.

## iyzico — `specs/iyzico/`

    2026-08-14.yaml        the whole In-Store API v3 as one document
    2026-08-14.meta.json   where it came from, and a hash of the page it came from
    latest.yaml            symlink to the newest

iyzico publishes no OpenAPI file. Their documentation page embeds one small
OpenAPI 3.0.3 document per endpoint, and `scripts/merge_iyzico.py` fetches the
page, pulls the seven of them out, and merges them into one. It also repairs
two things the embedded documents get wrong:

- `"type": "BigDecimal"` and `"type": "Long"` are not OpenAPI types. They
  become `string`/`decimal` and `integer`/`int64`.
- The `/crypt/decrypt` operation has a stray `detail` key sitting among its
  `responses`. It is dropped.

`/crypt/decrypt` is also the one operation still on the `/v2/in-store` base;
it carries its own `servers` entry saying so.

## Stripe — `specs/stripe/`

    2026-08-14.meta.json   the API version, and a hash of the full upstream spec
    latest.yaml            the subset kasapay maps, resolved to its schemas

Stripe's spec is around 7MB, and a PaymentIntent transitively references most
of it — `latest.yaml` is already 1.7MB after filtering to six operations. So
only the metadata is kept per date: `upstream_sha256` is what detects drift,
and `api_version` is what names it. The subset itself rolls forward in place.

kasapay does not generate a Stripe client from this. `kasapay-stripe` wraps
[`async-stripe`](https://github.com/arlyon/async-stripe), which is generated
from the same document weekly by people who do it full time. The subset here
is for reading when a mapping looks wrong.

## Refetching by hand

    pip install pyyaml
    python3 scripts/merge_iyzico.py    # writes today's date
    python3 scripts/fetch_stripe.py

Both take an optional `YYYY-MM-DD` argument to write under a different date.
