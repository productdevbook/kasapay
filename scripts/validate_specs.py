"""Checks that everything under specs/ is a valid OpenAPI document.

These files are assembled from fragments by merge_iyzico.py, which means every
bug in that script lands here as a document that looks plausible and is wrong.
Two have already: Java type names left where OpenAPI types belong, and a global
security scheme invented for operations iyzico documents no authentication for.

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

        operations = sum(len(ops) for ops in loaded.get("paths", {}).values())
        print(f"ok   {relative}  ({operations} operations)")

    if failures:
        print(f"\n{failures} of {len(documents)} documents failed", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
