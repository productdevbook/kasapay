"""Records what PayPal's own OpenAPI documents said about Orders v2 and Payments v2.

PayPal publishes one document per API, in
[paypal/paypal-rest-api-specifications], under Apache-2.0 — permissive, like
Stripe's, unlike Mollie's CC-BY-NC-SA — so the subset kasapay maps is kept
here rather than thrown away the way fetch_mollie.py's is. `LICENSE` in that
repository is what says so; neither document carries its own `info.license`
for a script to read.

[paypal/paypal-rest-api-specifications]: https://github.com/paypal/paypal-rest-api-specifications

Two things differ from fetch_stripe.py, both because PayPal's documents
reference more than schemas.

`kasapay-paypal` maps operations from two of PayPal's documents rather than
one: `checkout_orders_v2.json` for creating, reading, capturing and
authorizing an order, and `payments_payment_v2.json` for refunding a capture
and capturing an authorization directly. Each is fetched and cut down on its
own — `latest.yaml` for Orders, `payments-latest.yaml` for Payments — because
they are separate upstream files with separate version numbers, and rolling
them into one document would make a diff in either look like it came from
both.

Both documents' paths `$ref` into `components/responses` and
`components/parameters` as well as `components/schemas` — PayPal's standard
`error` object, and the `PayPal-Request-Id`/`Prefer` headers, are shared
components rather than inlined on each operation — so the resolver here
follows every section a `$ref` can name, the same generic walk
`fetch_mollie.py` uses, rather than the schema-only one `fetch_stripe.py`
gets away with.
"""

import argparse
import dataclasses
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys

import yaml

import dated


@dataclasses.dataclass(frozen=True)
class Document:
    """One of PayPal's OpenAPI documents, cut to what this crate maps."""

    name: str
    """What `<name>-latest.yaml` and `<name>-<date>.meta.json` are called —
    empty for Orders, which kept its original bare `latest.yaml` from before
    a second document existed, so an upgrade from #113 sees no file rename."""
    src: str
    keep: list[tuple[str, str]]
    """`(path, verb)` pairs — kept per pair rather than per path, the same
    reason Orders' own `/v2/checkout/orders/{id}` does: it carries `GET` and
    `PATCH` together and this crate implements only the read."""


DOCUMENTS = [
    Document(
        name="",
        src="https://raw.githubusercontent.com/paypal/paypal-rest-api-specifications/main/openapi/checkout_orders_v2.json",
        # create an order, read it back, capture it, place the hold an
        # `intent: AUTHORIZE` order asks for. Orders v2 is eight paths and
        # `/v2/checkout/orders/{id}` alone carries a fourth verb, PATCH, that
        # this adapter does not implement.
        keep=[
            ("/v2/checkout/orders", "post"),
            ("/v2/checkout/orders/{id}", "get"),
            ("/v2/checkout/orders/{id}/capture", "post"),
            ("/v2/checkout/orders/{id}/authorize", "post"),
        ],
    ),
    Document(
        name="payments",
        src="https://raw.githubusercontent.com/paypal/paypal-rest-api-specifications/main/openapi/payments_payment_v2.json",
        # refund a capture, capture an authorization directly, release one
        # that will not be captured. Not kept: reading a capture, an
        # authorization or a refund back by id — this crate has no call that
        # does — reauthorizing a hold, and find-eligible-methods, which is
        # unrelated to any of the three.
        keep=[
            ("/v2/payments/captures/{capture_id}/refund", "post"),
            ("/v2/payments/authorizations/{authorization_id}/capture", "post"),
            ("/v2/payments/authorizations/{authorization_id}/void", "post"),
        ],
    ),
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


def fetch_one(document: Document, day: str, directory: pathlib.Path) -> tuple[str, int, int, list[str]]:
    raw = subprocess.run(["curl", "-fsSL", document.src], capture_output=True, check=True).stdout
    spec = json.loads(raw)

    paths = {}
    missing = []
    for path, verb in document.keep:
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
            "description": f"Subset of {document.src}, limited to the operations "
            "kasapay-paypal maps. PayPal's own document, under PayPal's own "
            "Apache-2.0 licence, cut down and otherwise unaltered."
        },
        "servers": spec.get("servers", []),
        "paths": paths,
        "components": components,
    }

    stem = f"{document.name}-" if document.name else ""
    # Only meta is dated, the same choice fetch_stripe.py makes and for the
    # same reason: the subset still pulls in every schema PayPal's kept
    # operations reference transitively, so a dated copy per fetch buys diffs
    # nobody can read. `<stem>latest.yaml` rolls forward in place.
    (directory / f"{stem}latest.yaml").write_text(
        yaml.safe_dump(subset, allow_unicode=True, sort_keys=False, width=100),
        encoding="utf-8",
    )

    operations = sorted(
        operation.get("operationId", f"{verb.upper()} {path}")
        for path, item in paths.items()
        for verb, operation in item.items()
        if verb in VERBS and isinstance(operation, dict)
    )
    meta = json.dumps(
        {
            "source": document.src,
            "fetched": day,
            "api_version": spec["info"].get("version"),
            # Not in either document's own info block — read from
            # LICENSE in paypal/paypal-rest-api-specifications, checked
            # by hand.
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
    ) + "\n"
    # Dated records are written on the day they say something new, and
    # `fetched` on its own is not something new. See scripts/dated.py.
    dated.write_if_moved(
        directory / f"{stem}{day}.meta.json",
        meta,
        dated.newest_dated(directory, ".meta.json", prefix=stem),
    )

    return spec["info"].get("version"), len(paths), len(operations), missing


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("day", nargs="?", help="the date to write under, e.g. 2026-08-14")
    args = parser.parse_args()

    day = args.day or datetime.date.today().isoformat()
    directory = pathlib.Path(__file__).resolve().parent.parent / "specs" / "paypal"
    directory.mkdir(parents=True, exist_ok=True)

    for document in DOCUMENTS:
        version, paths, operations, missing = fetch_one(document, day, directory)
        label = document.name or "orders"
        print(
            f"{label}: api_version={version} paths={paths} "
            f"operations={operations} missing={missing}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
