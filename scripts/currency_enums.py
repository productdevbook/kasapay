"""Prints which currencies each part of iyzico's API says it takes.

There is no one list. A link can be priced in roubles and a payment cannot; a
subscription plan takes three currencies and a link seven. Every module written
against this API has had to establish its own list, and this is how, so the next
one does not inherit somebody else's by mistake.

    python3 scripts/currency_enums.py

Reads `specs/iyzico/*/latest.yaml`, which is what iyzico's documentation said on
the day it was fetched — not what their servers will accept today.
"""

import collections
import glob
import pathlib
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent


def enums(node, area: str, found: collections.Counter) -> None:
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "properties" and isinstance(value, dict):
                for name, schema in value.items():
                    if (
                        name.lower().startswith("currency")
                        and isinstance(schema, dict)
                        and schema.get("enum")
                    ):
                        found[(area, frozenset(schema["enum"]))] += 1
            enums(value, area, found)
    elif isinstance(node, list):
        for value in node:
            enums(value, area, found)


def main() -> int:
    found: collections.Counter = collections.Counter()
    for path in sorted(glob.glob(str(ROOT / "specs" / "iyzico" / "*" / "latest.yaml"))):
        area = pathlib.Path(path).parent.name
        enums(yaml.safe_load(pathlib.Path(path).read_text(encoding="utf-8")), area, found)
    if not found:
        print("no currency enum found under specs/iyzico/", file=sys.stderr)
        return 1

    by_list: dict[frozenset[str], list[tuple[str, int]]] = {}
    for (area, values), count in found.items():
        by_list.setdefault(values, []).append((area, count))
    for values in sorted(by_list, key=lambda v: (-len(v), sorted(v))):
        where = ", ".join(f"{area} ×{count}" for area, count in sorted(by_list[values]))
        print(f"{' '.join(sorted(values)):32} {where}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
