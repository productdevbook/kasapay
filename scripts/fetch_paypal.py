"""Records what PayPal's own OpenAPI document said about Orders v2.

PayPal publishes one, in [paypal/paypal-rest-api-specifications], under
Apache-2.0 — permissive, like Stripe's, unlike Mollie's CC-BY-NC-SA — so the
subset kasapay maps is kept here rather than thrown away the way
fetch_mollie.py's is. `LICENSE` in that repository is what says so; the
document itself carries no `info.license` for a script to read.

[paypal/paypal-rest-api-specifications]: https://github.com/paypal/paypal-rest-api-specifications

Two things differ from fetch_stripe.py, both because PayPal's document
references more than schemas.

`checkout_orders_v2.json` is one document among several in that repository —
Orders is the only one kasapay-paypal maps, so only that file is fetched.

Its paths `$ref` into `components/responses` and `components/parameters` as
well as `components/schemas` — PayPal's standard `error` object, and the
`PayPal-Request-Id`/`Prefer` headers, are both shared components rather than
inlined on each operation — so the resolver here follows every section a `$ref`
can name, the same generic walk `fetch_mollie.py` uses, rather than the
schema-only one `fetch_stripe.py` gets away with.
"""

import argparse
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys

import yaml

SRC = "https://raw.githubusercontent.com/paypal/paypal-rest-api-specifications/main/openapi/checkout_orders_v2.json"

# Only the operations kasapay-paypal maps onto: create an order, read it back,
# capture it. Orders v2 is eight paths and `/v2/checkout/orders/{id}` alone
# carries a fourth verb, PATCH, that this adapter does not implement — kept
# per (path, verb) rather than per path so that one does not slip in too.
KEEP = [
    ("/v2/checkout/orders", "post"),
    ("/v2/checkout/orders/{id}", "get"),
    ("/v2/checkout/orders/{id}/capture", "post"),
]

VERBS = {"get", "post", "patch", "delete", "put", "head", "options", "trace"}


def refs(node):
    """Every `#/components/<section>/<name>` this node reaches, as pairs."""
    if isinstance(node, dict):
        pointer = node.get("$ref")
        if isinstance(pointer, str) and pointer.startswith("#/components/"):
            parts = pointer.split("/")
            if len(parts) == 4:
                yield parts[2], parts[3]
        for value in node.values():
            yield from refs(value)
    elif isinstance(node, list):
        for value in node:
            yield from refs(value)


def resolve(spec, wanted):
    """Every component the kept paths reach, transitively, across every section."""
    components = spec.get("components", {})
    out, queue, seen = {}, list(wanted), set()
    while queue:
        section, name = queue.pop()
        if (section, name) in seen:
            continue
        seen.add((section, name))
        node = components.get(section, {}).get(name)
        if node is None:
            continue
        out.setdefault(section, {})[name] = node
        queue.extend(refs(node))
    return {section: dict(sorted(items.items())) for section, items in sorted(out.items())}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("day", nargs="?", help="the date to write under, e.g. 2026-08-14")
    args = parser.parse_args()

    raw = subprocess.run(["curl", "-sSL", SRC], capture_output=True, check=True).stdout
    spec = json.loads(raw)

    paths = {}
    missing = []
    for path, verb in KEEP:
        operation = spec["paths"].get(path, {}).get(verb)
        if operation is None:
            missing.append(f"{verb.upper()} {path}")
            continue
        paths.setdefault(path, {})[verb] = operation
    components = resolve(spec, list(refs(paths)))

    subset = {
        "openapi": spec["openapi"],
        "info": spec["info"]
        | {
            "description": f"Subset of {SRC}, limited to the operations kasapay-paypal maps. "
            "PayPal's own document, under PayPal's own Apache-2.0 licence, cut down and "
            "otherwise unaltered."
        },
        "servers": spec.get("servers", []),
        "paths": paths,
        "components": components,
    }

    day = args.day or datetime.date.today().isoformat()
    directory = pathlib.Path(__file__).resolve().parent.parent / "specs" / "paypal"
    directory.mkdir(parents=True, exist_ok=True)

    # Only meta is dated, the same choice fetch_stripe.py makes and for the
    # same reason: the subset still pulls in every payment_source PayPal's
    # `order` schema knows about, so a dated copy per fetch buys diffs nobody
    # can read. latest.yaml rolls forward in place.
    (directory / "latest.yaml").write_text(
        yaml.safe_dump(subset, allow_unicode=True, sort_keys=False, width=100),
        encoding="utf-8",
    )

    operations = sorted(
        operation.get("operationId", f"{verb.upper()} {path}")
        for path, item in paths.items()
        for verb, operation in item.items()
        if verb in VERBS and isinstance(operation, dict)
    )
    (directory / f"{day}.meta.json").write_text(
        json.dumps(
            {
                "source": SRC,
                "fetched": day,
                "api_version": spec["info"].get("version"),
                # Not in the document's own info block — read from LICENSE in
                # paypal/paypal-rest-api-specifications, checked by hand.
                "licence": "Apache-2.0",
                "document_kept": True,
                "upstream_sha256": hashlib.sha256(raw).hexdigest(),
                "kept_paths": list(paths),
                "missing_paths": missing,
                "operations": operations,
                "component_counts": {
                    section: len(items) for section, items in sorted(components.items())
                },
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    print(
        f"api_version={spec['info'].get('version')} paths={len(paths)} "
        f"operations={len(operations)} missing={missing}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
