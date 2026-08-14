"""Checks that everything under specs/ is a valid OpenAPI document.

These files are assembled from fragments by merge_iyzico.py, which means every
bug in that script lands here as a document that looks plausible and is wrong.
Three have already: Java type names left where OpenAPI types belong, a global
security scheme invented for operations iyzico documents no authentication for,
and a null description carried over from a fragment whose author left the field
blank.

The checks that need no dependency run always, so that somebody who cannot
install openapi-spec-validator still catches the mistakes that script makes.
Run by CI on every push. Exits non-zero on the first document that fails.
"""

import pathlib
import sys

import yaml

try:
    from openapi_spec_validator import validate
    from openapi_spec_validator.readers import read_from_filename
except ImportError:  # a contributor without the package still gets the type check
    validate = None

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"

# Types the fragments use that are Java's, not OpenAPI's. merge_iyzico.py
# repairs the ones it knows; anything left is a name it has not met yet.
OPENAPI_TYPES = {"array", "boolean", "integer", "null", "number", "object", "string"}

# Keys OpenAPI defines as strings. A null one is what a fragment carries when
# its author left the field blank, and it is not something YAML or a reader can
# make sense of.
STRING_KEYS = frozenset(
    "description format operationId pattern summary title".split()
)


def declared_types(node, found: set[str], in_security: bool = False) -> set[str]:
    """Every `type` in the document, skipping `securitySchemes`.

    A security scheme's `type` is `http` or `apiKey`, which are correct there
    and meaningless as schema types.
    """
    if isinstance(node, dict):
        declared = node.get("type")
        if isinstance(declared, str) and not in_security:
            found.add(declared)
        for key, value in node.items():
            declared_types(value, found, in_security or key == "securitySchemes")
    elif isinstance(node, list):
        for value in node:
            declared_types(value, found, in_security)
    return found


def blank_strings(node, found: list[str], trail: str = "") -> list[str]:
    """Every key OpenAPI wants a string for that carries a null instead."""
    if isinstance(node, dict):
        for key, value in node.items():
            where = f"{trail}/{key}"
            if value is None and key in STRING_KEYS:
                found.append(where)
            blank_strings(value, found, where)
    elif isinstance(node, list):
        for index, value in enumerate(node):
            blank_strings(value, found, f"{trail}[{index}]")
    return found


def main() -> int:
    strict = "--strict" in sys.argv
    if validate is None:
        if strict:
            print(
                "openapi-spec-validator is not installed and --strict was asked for",
                file=sys.stderr,
            )
            return 1
        print("openapi-spec-validator missing: checking types only", file=sys.stderr)

    documents = sorted(p for p in SPECS.rglob("*.yaml") if not p.is_symlink())
    if not documents:
        print("no specs found", file=sys.stderr)
        return 1

    failures = 0
    for document in documents:
        relative = document.relative_to(ROOT)
        first = yaml.safe_load(document.read_text(encoding="utf-8"))
        # PayTR publishes no API description, so specs/paytr/ holds a record of
        # their documentation instead. It is not OpenAPI and does not pretend to
        # be; validating it as OpenAPI would only fail.
        if not isinstance(first, dict) or "openapi" not in first:
            print(f"skip {relative}  (not an OpenAPI document)")
            continue
        if validate is not None:
            try:
                spec, _ = read_from_filename(str(document))
                validate(spec)
            except Exception as error:  # noqa: BLE001 - any failure is a failure
                print(f"FAIL {relative}\n  {error}", file=sys.stderr)
                failures += 1
                continue

        loaded = yaml.safe_load(document.read_text(encoding="utf-8"))
        foreign = declared_types(loaded, set()) - OPENAPI_TYPES
        if foreign:
            print(
                f"FAIL {relative}\n  not OpenAPI types: {', '.join(sorted(foreign))}",
                file=sys.stderr,
            )
            failures += 1
            continue

        blank = blank_strings(loaded, [])
        if blank:
            shown = ", ".join(blank[:5]) + (f" and {len(blank) - 5} more" if len(blank) > 5 else "")
            print(f"FAIL {relative}\n  null where a string belongs: {shown}", file=sys.stderr)
            failures += 1
            continue

        operations = sum(len(ops) for ops in loaded.get("paths", {}).values())
        print(f"ok   {relative}  ({operations} operations)")

    if failures:
        print(f"\n{failures} of {len(documents)} documents failed", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
