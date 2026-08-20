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
import re
import subprocess
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SPECS = ROOT / "specs"

# What a field is allowed to be. Losing one of these loses a documented fact
# without losing the field, which is why they are counted alongside the names.
CONSTRAINTS = (
    "enum",
    "format",
    "pattern",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "multipleOf",
)


def fields(document: dict) -> dict[str, set[str]]:
    """Every operation, and the field names and constraints it carries.

    Names, not paths. A dotted path through a document where twenty schemas
    refer to each other is exponential in the nesting depth — one iyzico area
    produced millions of them — and what is worth catching is a field that
    stopped being documented at all, which a name catches. A field that only
    moved deeper reads as unchanged, which is the right answer often enough.

    Constraints count as things carried, written `currency!enum`. A field whose
    `enum` disappears still has its name, so counting names alone reported no
    loss while mass payout's list of currencies was missing — which is how the
    loss lasted until somebody implementing that API noticed by hand.
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
        for name, schema in (node.get("properties") or {}).items():
            if isinstance(schema, dict):
                found |= {f"{name}!{key}" for key in CONSTRAINTS if key in schema}
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


def latest_at(link: pathlib.Path, revision: str | None) -> pathlib.Path | None:
    """What a `…latest.yaml` names, now or at a revision.

    Two shapes live under `specs/`. Stripe and PayPal roll `latest.yaml`
    forward in place, so the path is the same on both sides. iyzico and PayTR
    write a **new** dated file each refetch and repoint the symlink — so the
    document to compare against is whatever the symlink named then, and pairing
    by path finds no counterpart at the revision and compares nothing.

    That is not a hypothetical: it is why a removed field reported `0 lost` for
    the provider this script's own docstring says it was written for.
    """
    if revision is None:
        if not link.exists():
            return None
        return link.resolve() if link.is_symlink() else link
    relative = link.relative_to(ROOT) if link.is_absolute() else link
    listed = subprocess.run(
        ["git", "ls-tree", revision, "--", str(relative)],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if listed.returncode != 0 or not listed.stdout.strip():
        return None
    # 120000 is git's mode for a symlink; its blob is the target path.
    if listed.stdout.split()[0] != "120000":
        return link
    target = subprocess.run(
        ["git", "show", f"{revision}:{relative}"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if target.returncode != 0:
        return None
    return link.parent / target.stdout.strip()


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

    unpaired: list[str] = []
    if args.against_git:
        pairs = []
        # `*latest.yaml`, not `latest.yaml`: PayPal keeps two documents in one
        # directory and tells them apart with `payments-` in front. Matching the
        # bare filename stopped comparing the half that carries capture, void
        # and refund, and said `0 lost` about it.
        for link in sorted(SPECS.rglob("*latest.yaml")):
            now = latest_at(link, None)
            then = latest_at(link, args.against_git)
            if now is None:
                continue
            if then is None:
                unpaired.append(str(link.relative_to(SPECS)))
                continue
            pairs.append((now, (then, args.against_git), (now, None)))
        unpaired.extend(str(p.relative_to(SPECS)) for p in unwalked(pairs))
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

    considered = {provider_of(shown) for shown, _, _ in pairs}
    spoke_for = set()
    lost_total = gained_total = 0
    for shown, (old_path, old_rev), (new_path, new_rev) in pairs:
        before, after_doc = load(old_path, old_rev), load(new_path, new_rev)
        if not before or not after_doc:
            unpaired.append(str(shown.parent.relative_to(SPECS)))
            continue
        if fields(before) or fields(after_doc):
            spoke_for.add(provider_of(shown))
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
    # A pair that could not be formed is not a pair that came back clean, and
    # printing nothing about it reads as the second.
    if unpaired:
        print("nothing compared for: " + ", ".join(sorted(set(unpaired))))
    # Silence about a provider is not the same as nothing having moved there,
    # and a reader will take it that way unless it is said. PayTR publishes no
    # API description for this to walk, and Mollie's is licensed so that no
    # copy of it can be kept here — `specs/mollie/<date>.meta.json` and its two
    # hashes are what say that one moved.
    silent = sorted(
        provider
        for provider in providers()
        if provider not in spoke_for
        and (provider in considered or not any((SPECS / provider).rglob("*.yaml")))
    )
    if silent:
        print(
            f"no field-level comparison for: {', '.join(silent)} — nothing under specs/ "
            "that both sides carry as an API description"
        )
    return 1 if lost_total else 0


DATED = re.compile(r"(?:.*-)?\d{4}-\d{2}-\d{2}\.yaml")


def unwalked(pairs: list) -> list[pathlib.Path]:
    """API descriptions under `specs/` that nothing in this run compares.

    The walk finds documents through their `…latest.yaml`. A document no such
    link names is invisible to it — which is not hypothetical: matching the
    bare name `latest.yaml` silently dropped `payments-latest.yaml`, and the
    report went on saying `0 field(s) gained, 0 lost`.

    Dated records are expected to be unwalked; the newest of each is what a
    link points at, and the older ones are history.
    """
    walked = {shown for shown, _, _ in pairs}
    return [
        path
        for path in sorted(SPECS.rglob("*.yaml"))
        if not path.is_symlink() and not DATED.fullmatch(path.name) and path not in walked
    ]


def providers() -> list[str]:
    """Every provider `specs/` holds anything for."""
    return sorted(p.name for p in SPECS.iterdir() if p.is_dir())


def provider_of(document: pathlib.Path) -> str:
    return document.relative_to(SPECS).parts[0]


if __name__ == "__main__":
    sys.exit(main())
