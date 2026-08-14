"""Records what Mollie's own OpenAPI document said, without keeping a copy of it.

Mollie publishes one, which iyzico and PayTR do not, so nothing here is
reassembled from documentation pages: the whole file is fetched, cut down to
the operations kasapay maps, checked — and then thrown away.

**The subset is deliberately not committed.** Mollie licenses their document
CC-BY-NC-SA-4.0, and this repository is MIT: a non-commercial share-alike file
inside an MIT tree is a restriction on exactly the people the licence invites,
and one they would not notice. So only `<date>.meta.json` is written — a
version string, two hashes, an operation list, the repairs — which are facts
about the document rather than the document. `--write-document` writes the
subset for somebody reading it, to a path `.gitignore` covers.

What that costs is the field-level diff `compare_specs.py` gives the others.
What it keeps is the weekly job noticing that the version moved or a hash
changed, which is most of why `specs/` exists — and `subset_sha256` is the
sharper of the two, because `upstream_sha256` moves when any of Mollie's
eighty-seven paths does and the subset's moves only for the five that matter.

Three things differ from fetch_stripe.py.

The document is YAML rather than JSON, and PyYAML resolves an unquoted date
into a `datetime`. Several of Mollie's examples carry one, and one of them —
`2023-02-29` — is not a real date, so a plain `safe_load` raises before it can
be subset at all. Timestamp resolution is switched off, which also keeps the
written subset loadable by validate_specs.py.

Mollie refs more component sections than Stripe does — parameters, responses
and examples as well as schemas — so the resolver follows all of them.

One thing is repaired rather than copied, and it is recorded in the meta so the
repair is never silent. Five of Mollie's parameters are a `$ref` with a
`schema` beside it. A Reference Object in OpenAPI 3.1 may carry only `summary`
and `description` alongside its `$ref`, so those five make the document
invalid — `openapi-spec-validator` refuses it. What Mollie plainly means is the
shared parameter with that schema on top, so that is what is written: the
referenced parameter inlined, the siblings overlaid. Nothing is dropped.

The same pattern inside a *schema* is left exactly as it is. `$ref` with
siblings is legal in JSON Schema 2020-12, which is what an OpenAPI 3.1 Schema
Object is, and there are thirty-three of them.

And because nothing is committed, `validate_specs.py` never sees this document
— so the validation happens here instead. That check is not decoration: it is
what caught those five in the first place, on the first CI run.
"""

import argparse
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys

import yaml

try:
    from openapi_spec_validator import validate
except ImportError:  # a contributor without the package still gets everything else
    validate = None

SRC = "https://raw.githubusercontent.com/mollie/openapi/main/specs.yaml"

# Only the paths kasapay maps onto. The full document is ~1.9MB across 87
# paths, and a diff of all of it says nothing about whether an adapter broke.
KEEP = [
    "/v2/payments",
    "/v2/payments/{paymentId}",
    "/v2/payments/{paymentId}/captures",
    "/v2/payments/{paymentId}/refunds",
    "/v2/payments/{paymentId}/release-authorization",
]


class Loader(yaml.SafeLoader):
    """SafeLoader with the implicit timestamp resolver removed."""


Loader.yaml_implicit_resolvers = {
    first: [(tag, regexp) for tag, regexp in resolvers if tag != "tag:yaml.org,2002:timestamp"]
    for first, resolvers in yaml.SafeLoader.yaml_implicit_resolvers.items()
}


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
    """Every component the kept paths reach, transitively."""
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


# A Reference Object may carry these and nothing else.
REFERENCE_KEYS = {"$ref", "summary", "description"}

VERBS = {"get", "post", "patch", "delete", "put", "head", "options", "trace"}


def inline_referenced_parameters(spec, paths) -> list[str]:
    """Repairs a parameter that is a `$ref` with more than a Reference Object may carry."""
    repaired = []
    for path, item in paths.items():
        for verb, operation in item.items():
            if verb not in VERBS or not isinstance(operation, dict):
                continue
            for index, parameter in enumerate(operation.get("parameters", [])):
                if not isinstance(parameter, dict):
                    continue
                pointer = parameter.get("$ref", "")
                extra = set(parameter) - REFERENCE_KEYS
                if not extra or not pointer.startswith("#/components/parameters/"):
                    continue
                target = spec["components"]["parameters"][pointer.rsplit("/", 1)[1]]
                merged = dict(target)
                merged.update({k: v for k, v in parameter.items() if k != "$ref"})
                operation["parameters"][index] = merged
                repaired.append(f"{verb.upper()} {path} parameters[{index}] ({pointer})")
    return repaired


def faults(subset) -> list[str]:
    """The two things the cut and the repairs could get wrong, checked without a dependency.

    `openapi-spec-validator` says far more than this and is run below when it
    is installed. These two run everywhere, because a meta claiming a document
    was checked on a machine that could not check it is worth less than no
    claim at all.
    """
    found = []
    for path, item in subset["paths"].items():
        for verb, operation in item.items():
            if verb not in VERBS or not isinstance(operation, dict):
                continue
            where = f"{verb.upper()} {path}"
            references = [(f"parameters[{i}]", p) for i, p in enumerate(operation.get("parameters", []))]
            references += [(f"responses[{code}]", r) for code, r in operation.get("responses", {}).items()]
            for name, node in references:
                if isinstance(node, dict) and "$ref" in node:
                    extra = sorted(set(node) - REFERENCE_KEYS)
                    if extra:
                        found.append(
                            f"{where} {name} is a Reference Object carrying {extra}"
                        )
    for pointer in sorted(set(pointers(subset))):
        node = subset
        for part in pointer.removeprefix("#/").split("/"):
            node = node.get(part) if isinstance(node, dict) else None
            if node is None:
                found.append(f"{pointer} points at nothing the subset carries")
                break
    return sorted(found)


def pointers(node):
    """Every `$ref` in the document, whatever it points at."""
    if isinstance(node, dict):
        pointer = node.get("$ref")
        if isinstance(pointer, str):
            yield pointer
        for value in node.values():
            yield from pointers(value)
    elif isinstance(node, list):
        for value in node:
            yield from pointers(value)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("day", nargs="?", help="the date to write under, e.g. 2026-08-14")
    parser.add_argument(
        "--write-document",
        action="store_true",
        help="also write the subset itself, for reading. Gitignored: it is Mollie's "
        "file under Mollie's licence and does not belong in an MIT tree.",
    )
    args = parser.parse_args()

    raw = subprocess.run(["curl", "-sSL", SRC], capture_output=True, check=True).stdout
    spec = yaml.load(raw.decode("utf-8"), Loader=Loader)

    paths = {p: spec["paths"][p] for p in KEEP if p in spec["paths"]}
    missing = [p for p in KEEP if p not in spec["paths"]]
    repaired = inline_referenced_parameters(spec, paths)
    components = resolve(spec, list(refs(paths)))
    # securitySchemes are not reachable by $ref — `security` names them — and
    # how a request is authenticated is the first thing a reader wants.
    components["securitySchemes"] = spec["components"]["securitySchemes"]

    subset = {
        "openapi": spec["openapi"],
        "info": spec["info"]
        | {
            "description": f"Subset of {SRC}, limited to the operations kasapay maps. "
            "Mollie's own document, under Mollie's own licence, cut down and "
            "otherwise unaltered."
        },
        "servers": spec.get("servers", []),
        "security": spec.get("security", []),
        "paths": paths,
        "components": components,
    }

    written = yaml.safe_dump(subset, allow_unicode=True, sort_keys=False, width=100)

    # Nothing is committed, so validate_specs.py never gets a chance to. This
    # is where a document Mollie's own tooling would refuse is refused.
    written_back = yaml.safe_load(written)
    broken = faults(written_back)
    if broken:
        print("the subset is not valid OpenAPI:", file=sys.stderr)
        for fault in broken:
            print(f"  {fault}", file=sys.stderr)
        return 1
    checked = "reference objects and pointers, after the repairs below"
    if validate is None:
        print("openapi-spec-validator is not installed: checked the two rules below only",
              file=sys.stderr)
    else:
        try:
            validate(written_back)
        except Exception as error:  # noqa: BLE001 - any failure is a failure
            print(f"the subset is not valid OpenAPI:\n  {error}", file=sys.stderr)
            return 1
        checked = "valid OpenAPI 3.1 per openapi-spec-validator, after the repairs below"

    day = args.day or datetime.date.today().isoformat()
    directory = pathlib.Path(__file__).resolve().parent.parent / "specs" / "mollie"
    directory.mkdir(parents=True, exist_ok=True)

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
                "licence": spec["info"].get("license"),
                "document_kept": False,
                "checked": checked,
                # The whole of what Mollie publishes. Moves when any of their
                # eighty-seven paths does.
                "upstream_sha256": hashlib.sha256(raw).hexdigest(),
                # The subset this would have written. Moves only for the five
                # paths kasapay maps, which is the sharper signal of the two.
                "subset_sha256": hashlib.sha256(written.encode("utf-8")).hexdigest(),
                "kept_paths": list(paths),
                "missing_paths": missing,
                "operations": operations,
                "repaired_reference_objects": repaired,
                "component_counts": {
                    section: len(items) for section, items in sorted(components.items())
                },
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    if args.write_document:
        dated = directory / f"{day}.yaml"
        dated.write_text(written, encoding="utf-8")
        print(f"wrote {dated.relative_to(directory.parent.parent)} (gitignored)")

    print(
        f"api_version={spec['info'].get('version')} paths={len(paths)} "
        f"operations={len(operations)} missing={missing} — {checked}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
