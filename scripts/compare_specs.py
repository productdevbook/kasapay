"""Says what moved between two versions of a provider's API description.

A refetch that silently drops a field looks exactly like a refetch that
changed nothing: the file is still valid, the operation count is unchanged,
and the diff is thousands of lines of reordered YAML. That is how
merge_iyzico.py lost `reason` and `refundPrice` for a month.

So this resolves every operation down to the field names it actually carries,
following `$ref`s, and compares two dated documents by that. A lost field is
the thing worth reading; a gained one is worth reading too.

    python3 scripts/compare_specs.py 2026-07-01 2026-08-14
    python3 scripts/compare_specs.py --against-git origin/main

Exits non-zero when a field was lost, so CI can refuse to merge a refetch that
takes documentation away without somebody saying that is what they meant.
"""

import argparse
import pathlib
import subprocess
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"


def fields(document: dict) -> dict[str, set[str]]:
    """Every operation in the document, and the field names it carries.

    Names, not paths. A dotted path through a document where twenty schemas
    refer to each other is exponential in the nesting depth — one iyzico area
    produced millions of them — and what is worth catching is a field that
    stopped being documented at all, which a name catches. A field that only
    moved deeper reads as unchanged, which is the right answer often enough.
    """
    schemas = document.get("components", {}).get("schemas", {})
    # Each named schema is walked once. Without this, twenty operations sharing
    # one response schema take minutes rather than a second.
    resolved: dict[str, set[str]] = {}
    open_now: set[str] = set()

    def named(name: str) -> set[str]:
        if name in open_now:  # a schema that refers to itself, cut where it closes
            return set()
        if name not in resolved:
            open_now.add(name)
            resolved[name] = walk(schemas.get(name, {}))
            open_now.discard(name)
        return resolved[name]

    def walk(node) -> set[str]:
        if not isinstance(node, dict):
            return set()
        if "$ref" in node:
            return named(node["$ref"].rsplit("/", 1)[-1])
        found = set(node.get("properties") or {})
        for schema in (node.get("properties") or {}).values():
            found |= walk(schema)
        if "items" in node:
            found |= walk(node["items"])
        for combinator in ("allOf", "anyOf", "oneOf"):
            for member in node.get(combinator, []):
                found |= walk(member)
        return found

    out: dict[str, set[str]] = {}
    for path, operations in (document.get("paths") or {}).items():
        for verb, operation in operations.items():
            if not isinstance(operation, dict):
                continue
            carried = set()
            body = (operation.get("requestBody") or {}).get("content", {})
            for media in body.values():
                carried |= walk(media.get("schema", {}))
            for response in (operation.get("responses") or {}).values():
                for media in (response.get("content") or {}).values():
                    carried |= walk(media.get("schema", {}))
            out[f"{verb.upper()} {path}"] = carried
    return out


def load(path: pathlib.Path, revision: str | None) -> dict:
    if revision is None:
        return yaml.safe_load(path.read_text(encoding="utf-8"))
    shown = subprocess.run(
        ["git", "show", f"{revision}:{path.relative_to(ROOT)}"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return yaml.safe_load(shown.stdout) if shown.returncode == 0 else {}


def compare(before: dict, after: dict) -> tuple[list[str], list[str], list[str]]:
    """Lost fields, gained fields, and operations that are gone entirely."""
    old, new = fields(before), fields(after)
    lost = [
        f"{operation}: {name}"
        for operation, carried in sorted(old.items())
        if operation in new
        for name in sorted(carried - new[operation])
    ]
    gained = [
        f"{operation}: {name}"
        for operation, carried in sorted(new.items())
        for name in sorted(carried - old.get(operation, set()))
    ]
    return lost, gained, sorted(set(old) - set(new))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", nargs="?", help="a date under specs/, e.g. 2026-07-01")
    parser.add_argument("after", nargs="?", help="a date under specs/, defaults to latest")
    parser.add_argument(
        "--against-git",
        metavar="REVISION",
        help="compare the working tree's specs against a git revision instead",
    )
    args = parser.parse_args()

    if args.against_git:
        documents = [p for p in sorted(SPECS.rglob("*.yaml")) if not p.is_symlink()]
        pairs = [(p, (p, args.against_git), (p, None)) for p in documents]
    elif args.before:
        after = args.after
        pairs = []
        for area in sorted(p for p in SPECS.iterdir() if p.is_dir()):
            old = area / f"{args.before}.yaml"
            new = area / (f"{after}.yaml" if after else "latest.yaml")
            if old.exists() and new.exists():
                pairs.append((new, (old, None), (new, None)))
        if not pairs:
            print(f"nothing under specs/ is dated {args.before}", file=sys.stderr)
            return 1
    else:
        parser.print_usage(sys.stderr)
        return 1

    lost_total = gained_total = 0
    for shown, (old_path, old_rev), (new_path, new_rev) in pairs:
        before, after_doc = load(old_path, old_rev), load(new_path, new_rev)
        if not before or not after_doc:
            continue
        lost, gained, gone = compare(before, after_doc)
        lost_total += len(lost) + len(gone)
        gained_total += len(gained)
        name = shown.relative_to(ROOT)
        if gone:
            print(f"GONE {name}")
            for operation in gone:
                print(f"  {operation}")
        if lost:
            print(f"LOST {name}")
            for entry in lost:
                print(f"  {entry}")
        if gained:
            print(f"NEW  {name}")
            for entry in gained:
                print(f"  {entry}")

    print(f"\n{gained_total} field(s) gained, {lost_total} lost")
    return 1 if lost_total else 0


if __name__ == "__main__":
    sys.exit(main())
