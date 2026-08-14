"""Reassembles iyzico's API description from the fragments in their docs.

iyzico publishes no OpenAPI file. Their documentation embeds one small OpenAPI
document per endpoint, so this sweeps every page listed in llms.txt, pulls the
fragments out, and merges them into one document per product area.
"""

import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

INDEX = "https://docs.iyzico.com/llms.txt"
ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "specs" / "iyzico"

PRODUCTION = "https://api.iyzipay.com"
SANDBOX = "https://sandbox-api.iyzipay.com"

# Not OpenAPI types. iyzico's fragments use Java's names for them.
JAVA_TYPES = {
    "BigDecimal": ("string", "decimal"),
    "Long": ("integer", "int64"),
    "Integer": ("integer", "int32"),
    "Double": ("number", "double"),
    "Boolean": ("boolean", None),
    "String": ("string", None),
}


def fetch(url: str) -> str:
    return subprocess.run(
        ["curl", "-sSL", "--max-time", "30", url],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def pages() -> list[str]:
    index = fetch(INDEX)
    urls = sorted(set(re.findall(r"https://docs\.iyzico\.com/[^)\s]+\.md", index)))
    # The English pages carry the same fragments as the Turkish ones.
    return [u for u in urls if "/en/" not in u]


def area(url: str) -> str:
    parts = url.removeprefix("https://docs.iyzico.com/").removesuffix(".md").split("/")
    return "/".join(parts[:2]) if len(parts) > 1 else parts[0]


def fragments(markdown: str):
    for match in re.finditer(r'^\{"openapi".*$', markdown, re.M):
        try:
            yield json.loads(match.group(0))
        except json.JSONDecodeError:
            continue


def repair(node):
    """Fixes the two things the embedded fragments consistently get wrong."""
    if isinstance(node, dict):
        declared = node.get("type")
        if declared in JAVA_TYPES:
            openapi_type, fmt = JAVA_TYPES[declared]
            node["type"] = openapi_type
            if fmt:
                node["format"] = fmt
        for value in node.values():
            repair(value)
    elif isinstance(node, list):
        for value in node:
            repair(value)
    return node


def rename_refs(node, renames: dict[str, str]):
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/schemas/"):
            name = ref.rsplit("/", 1)[1]
            if name in renames:
                node["$ref"] = f"#/components/schemas/{renames[name]}"
        for value in node.values():
            rename_refs(value, renames)
    elif isinstance(node, list):
        for value in node:
            rename_refs(value, renames)


def base_path(fragment) -> str:
    """The path prefix a fragment's own server carries, e.g. `/v3/in-store`."""
    for server in fragment.get("servers", []):
        url = server.get("url", "")
        for host in (PRODUCTION, SANDBOX):
            if url.startswith(host):
                return url.removeprefix(host).rstrip("/")
    return ""


def merge(area_name: str, found: list[tuple[str, dict]]) -> tuple[dict, list[str]]:
    spec = {
        "openapi": "3.0.3",
        "info": {
            "title": f"iyzico — {area_name}",
            "version": "unversioned",
            "description": (
                "Reassembled from the per-endpoint fragments embedded in "
                "https://docs.iyzico.com/. iyzico publishes no OpenAPI file; this "
                "is a record of what was documented, not a contract they offer."
            ),
        },
        "servers": [
            {"url": PRODUCTION, "description": "Production"},
            {"url": SANDBOX, "description": "Sandbox"},
        ],
        "security": [{"ApiKeyAuth": [], "SecretKeyAuth": [], "MerchantIdAuth": []}],
        "tags": [],
        "paths": {},
        "components": {
            "securitySchemes": {
                "ApiKeyAuth": {"type": "apiKey", "in": "header", "name": "x-api-key"},
                "SecretKeyAuth": {"type": "apiKey", "in": "header", "name": "x-secret-key"},
                "MerchantIdAuth": {"type": "apiKey", "in": "header", "name": "x-merchant-id"},
            },
            "schemas": {},
        },
    }
    notes: list[str] = []
    seen_tags: set[str] = set()

    for source, fragment in found:
        repair(fragment)
        prefix = base_path(fragment)

        renames: dict[str, str] = {}
        for name, schema in fragment.get("components", {}).get("schemas", {}).items():
            existing = spec["components"]["schemas"].get(name)
            if existing is None or existing == schema:
                spec["components"]["schemas"][name] = schema
                continue
            # Two pages describe different shapes under one name. Both are kept;
            # dropping either would silently lose an endpoint's request body.
            suffix = 2
            while f"{name}{suffix}" in spec["components"]["schemas"]:
                if spec["components"]["schemas"][f"{name}{suffix}"] == schema:
                    break
                suffix += 1
            renames[name] = f"{name}{suffix}"
            spec["components"]["schemas"][f"{name}{suffix}"] = schema
            notes.append(f"schema {name} redefined by {source}, kept as {name}{suffix}")

        if renames:
            rename_refs(fragment, renames)

        for tag in fragment.get("tags", []):
            if tag["name"] not in seen_tags:
                seen_tags.add(tag["name"])
                spec["tags"].append(tag)

        for path, operations in fragment.get("paths", {}).items():
            full = f"{prefix}{path}" if prefix else path
            for verb, operation in operations.items():
                if not isinstance(operation, dict):
                    continue
                # A stray `detail` key sits among `responses` on some operations.
                operation.pop("detail", None)
                operation.setdefault("x-iyzico-source", source)
                existing = spec["paths"].setdefault(full, {}).get(verb)
                if existing is None:
                    spec["paths"][full][verb] = operation
                elif existing != operation:
                    notes.append(f"{verb.upper()} {full} redefined by {source}, kept the first")

    spec["tags"].sort(key=lambda t: t["name"])
    spec["paths"] = dict(sorted(spec["paths"].items()))
    spec["components"]["schemas"] = dict(sorted(spec["components"]["schemas"].items()))
    return spec, notes


def main() -> None:
    day = sys.argv[1] if len(sys.argv) > 1 else datetime.date.today().isoformat()
    urls = pages()
    with ThreadPoolExecutor(max_workers=8) as pool:
        bodies = list(pool.map(fetch, urls))

    by_area: dict[str, list[tuple[str, dict]]] = {}
    for url, body in zip(urls, bodies, strict=True):
        found = list(fragments(body))
        if found:
            by_area.setdefault(area(url), []).extend((url, f) for f in found)

    import yaml

    index = {
        "fetched": day,
        "source": INDEX,
        "pages_swept": len(urls),
        "pages_with_fragments": sum(
            len({s for s, _ in v}) for v in by_area.values()
        ),
        "fragments": sum(len(v) for v in by_area.values()),
        "upstream_sha256": hashlib.sha256("".join(bodies).encode()).hexdigest(),
        "areas": {},
    }

    for area_name, found in sorted(by_area.items()):
        spec, notes = merge(area_name, found)
        directory = OUT / area_name
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"{day}.yaml").write_text(
            yaml.safe_dump(spec, allow_unicode=True, sort_keys=False, width=100),
            encoding="utf-8",
        )
        latest = directory / "latest.yaml"
        latest.unlink(missing_ok=True)
        latest.symlink_to(f"{day}.yaml")
        index["areas"][area_name] = {
            "operations": sorted(
                f"{verb.upper()} {path}"
                for path, ops in spec["paths"].items()
                for verb in ops
            ),
            "schemas": len(spec["components"]["schemas"]),
            "notes": notes,
        }

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{day}.index.json").write_text(
        json.dumps(index, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    total = sum(len(a["operations"]) for a in index["areas"].values())
    print(f"{len(by_area)} areas, {total} operations, {index['fragments']} fragments")
    for name, a in sorted(index["areas"].items()):
        print(f"  {len(a['operations']):3d}  {name}")


if __name__ == "__main__":
    main()
