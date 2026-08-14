"""Reassembles iyzico's API description from the fragments in their docs.

iyzico publishes no OpenAPI file. Their documentation embeds one small OpenAPI
document per endpoint, so this sweeps every page listed in llms.txt, pulls the
fragments out, and merges them into one document per product area.
"""

import copy
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

# Not OpenAPI types. iyzico's fragments use Java's names, and their own
# lowercase ones: `decimal` alone appears 864 times, on nearly every money field
# in the API. An amount is carried as a string so no reader is tempted to put it
# through a float.
JAVA_TYPES = {
    "BigDecimal": ("string", "decimal"),
    "decimal": ("string", "decimal"),
    "Long": ("integer", "int64"),
    "long": ("integer", "int64"),
    "Integer": ("integer", "int32"),
    "Double": ("number", "double"),
    "Boolean": ("boolean", None),
    "String": ("string", None),
    "date": ("string", "date"),
    "Date": ("string", "date"),
}


# iyzico's fragments routinely give a response only a `content` block, and
# OpenAPI requires a description. Filled in by status class rather than left
# blank, so it is obvious the text is ours.
DESCRIPTIONS = {
    "2": "Successful, per iyzico's example response",
    "4": "Rejected, per iyzico's example response",
    "5": "Server error, per iyzico's example response",
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
    return sorted(set(re.findall(r"https://docs\.iyzico\.com/[^)\s]+\.md", index)))


def path_parts(url: str) -> list[str]:
    parts = url.removeprefix("https://docs.iyzico.com/").removesuffix(".md").split("/")
    return parts[1:] if parts and parts[0] == "en" else parts


def is_english(url: str) -> bool:
    return url.startswith("https://docs.iyzico.com/en/")


VERSION = re.compile(r"^v\d+$")


def api_group(full_path: str) -> str:
    """Which part of the API a path belongs to, from the path itself.

    Not from the documentation URL: the same API is filed under `urunler/abonelik`
    in Turkish and `products/subscription` in English, so grouping by page would
    file every operation twice under two names.
    """
    segments = [s for s in full_path.strip("/").split("/") if s and not VERSION.match(s)]
    return segments[0] if segments else "unfiled"


def fragments(markdown: str):
    for match in re.finditer(r'^\{"openapi".*$', markdown, re.M):
        try:
            yield json.loads(match.group(0))
        except json.JSONDecodeError:
            continue


def repair(node, in_security: bool = False):
    """Fixes the things the embedded fragments consistently get wrong.

    `in_security` keeps the walk off `securitySchemes`, where `type: http` and
    `type: apiKey` are the real OpenAPI values rather than Java's.
    """
    if isinstance(node, dict):
        declared = None if in_security else node.get("type")
        if declared in JAVA_TYPES:
            openapi_type, fmt = JAVA_TYPES[declared]
            node["type"] = openapi_type
            if fmt:
                node["format"] = fmt
        # A field the fragment's author left blank arrives as a null description,
        # which OpenAPI refuses. It is an absent description, not an empty one.
        if node.get("description", "") is None:
            del node["description"]
        for key, value in node.items():
            repair(value, in_security or key == "securitySchemes")
    elif isinstance(node, list):
        for value in node:
            repair(value, in_security)
    return node


# Everything OpenAPI 3.0 allows on a Schema Object. Anything else sitting next
# to `items` on an array is a property the fragment's author indented one level
# too high.
SCHEMA_KEYWORDS = frozenset(
    """$ref additionalProperties allOf anyOf default deprecated description discriminator
    enum example exclusiveMaximum exclusiveMinimum externalDocs format items maxItems maxLength
    maxProperties maximum minItems minLength minProperties minimum multipleOf not nullable oneOf
    pattern properties readOnly required title type uniqueItems writeOnly xml""".split()
)


def repair_array_siblings(schema: dict) -> list[str]:
    """Moves a property that landed beside `items` into `items.properties`."""
    moved = []
    items = schema.get("items")
    if schema.get("type") != "array" or not isinstance(items, dict):
        return moved
    for key in [k for k in schema if k not in SCHEMA_KEYWORDS and not k.startswith("x-")]:
        stray = schema.pop(key)
        if isinstance(items.get("properties"), dict):
            items["properties"].setdefault(key, stray)
            moved.append(key)
    return moved


def repair_schema_dialect(node, path_template: str = "", notes: list[str] | None = None):
    """Fixes OpenAPI 3.0 violations the fragments carry.

    All of them come from iyzico rather than from this script:

    - `enum` sitting on a Parameter Object, where 3.0 wants it inside `schema`
    - `in: path` on a parameter the path has no placeholder for
    - `oneOf: [{type: string}, {type: null}]`, which is 3.1's way of saying
      nullable and not a thing in 3.0
    - a property indented beside `items` rather than inside it
    """
    notes = [] if notes is None else notes
    if isinstance(node, dict):
        for key in repair_array_siblings(node):
            notes.append(f"moved `{key}` into the array's items, where it was indented beside them")
        if "name" in node and "in" in node:
            if "enum" in node and isinstance(node.get("schema"), dict):
                node["schema"].setdefault("enum", node.pop("enum"))
            if node.get("in") == "path":
                if f"{{{node['name']}}}" in path_template:
                    node["required"] = True
                else:
                    node["in"] = "query"
                    node.setdefault("required", False)
                    notes.append(
                        f"parameter `{node['name']}` said in: path for a path with no such"
                        " placeholder, read as a query parameter"
                    )
        one_of = node.get("oneOf")
        if isinstance(one_of, list):
            kept = [m for m in one_of if not (isinstance(m, dict) and m.get("type") == "null")]
            if len(kept) != len(one_of):
                node["nullable"] = True
                if len(kept) == 1 and isinstance(kept[0], dict):
                    del node["oneOf"]
                    node.update(kept[0])
                else:
                    node["oneOf"] = kept
        for value in list(node.values()):
            repair_schema_dialect(value, path_template, notes)
    elif isinstance(node, list):
        for value in node:
            repair_schema_dialect(value, path_template, notes)
    return notes


def rename_refs(node, renames: dict[tuple[str, str], str]):
    """Rewrites `$ref`s a merge had to rename, keyed by (section, name)."""
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/"):
            _, _, section, name = ref.split("/", 3)
            if (section, name) in renames:
                node["$ref"] = f"#/components/{section}/{renames[(section, name)]}"
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


def operations_in(fragment: dict) -> frozenset[tuple[str, str]]:
    """Which operations a fragment describes, as (verb, full path)."""
    prefix = base_path(fragment)
    return frozenset(
        (verb, f"{prefix}{path}" if prefix else path)
        for path, ops in fragment.get("paths", {}).items()
        for verb, op in ops.items()
        if isinstance(op, dict)
    )


def documented_fields(fragment: dict) -> int:
    """How much a fragment actually documents, counted in schema properties."""
    return sum(
        len(schema.get("properties") or {})
        for schema in fragment.get("components", {}).get("schemas", {}).values()
        if isinstance(schema, dict)
    )


def deref(fragment: dict, node):
    """Follows a `$ref` into the fragment's own schemas, once."""
    if isinstance(node, dict) and "$ref" in node:
        name = node["$ref"].rsplit("/", 1)[-1]
        return fragment.get("components", {}).get("schemas", {}).get(name, {})
    return node if isinstance(node, dict) else {}


# What a field is allowed to be, as opposed to what it is called or how it
# reads. One language routinely constrains a field the other leaves open —
# mass payout's `currency` carries its enum only in Turkish — and a lost
# constraint is a documented fact lost. `description` is not here: the base
# fragment's prose is what a reader gets, and two languages of it in one field
# help nobody. Neither is `required`, which the two languages disagree about
# outright, and a union of it would invent a stricter API than either.
CONSTRAINTS = (
    "enum",
    "format",
    "pattern",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "multipleOf",
    "nullable",
    "default",
    "example",
)


def graft(base: dict, mine, theirs, other: dict, added: list[str], trail: str = "") -> None:
    """Copies into `mine` everything `theirs` documents and it does not."""
    mine, theirs = deref(base, mine), deref(other, theirs)
    for key in CONSTRAINTS:
        if key in theirs and key not in mine:
            mine[key] = copy.deepcopy(theirs[key])
            added.append(f"{trail.rstrip('.')}.{key}" if trail else key)
    ours, yours = mine.get("properties"), theirs.get("properties")
    if isinstance(ours, dict) and isinstance(yours, dict):
        for name, schema in yours.items():
            where = f"{trail}{name}"
            if name not in ours:
                ours[name] = copy.deepcopy(deref(other, schema))
                added.append(where)
            else:
                graft(base, ours[name], schema, other, added, f"{where}.")
    if "items" in mine and "items" in theirs:
        graft(base, mine["items"], theirs["items"], other, added, trail)


def schemas_of(fragment: dict, verb: str, path: str):
    """The request and response schemas one operation names, in document order."""
    prefix = base_path(fragment)
    for own, ops in fragment.get("paths", {}).items():
        if (f"{prefix}{own}" if prefix else own) != path:
            continue
        op = ops.get(verb)
        if not isinstance(op, dict):
            continue
        for media in (op.get("requestBody") or {}).get("content", {}).values():
            if "schema" in media:
                yield media["schema"]
        for response in (op.get("responses") or {}).values():
            for media in (response.get("content") or {}).values():
                if "schema" in media:
                    yield media["schema"]


def combine_languages(
    found: list[tuple[str, dict]],
) -> tuple[list[tuple[str, dict]], list[str]]:
    """Keeps one fragment per operation, with everything else grafted onto it.

    Neither language documents everything. The whole In-Store v3 API appears
    only in Turkish and the Terminal API's token refresh only in English, so the
    union across pages is the coverage.

    Where both languages describe one operation they are usually the same
    fragment twice — but not always, and the differences are substance rather
    than prose. iyzico's cancel-and-refund page carries `reason` and
    `description` only in Turkish; its In-Store refund is `refundAmount` in one
    language and `refundPrice` in the other. An earlier version of this script
    preferred English on the grounds that it was only choosing how things read.
    It was not: it was dropping documented fields.

    So the fuller fragment is the base — English breaking a tie, because its
    prose is what a reader gets — and every field any other fragment documents
    and it does not is grafted onto it. Grafting is per operation rather than
    per page, because pages overlap without matching: the checkout form's own
    page describes both initialising and querying, while a second page
    describes only the query, and the two disagree about where `currency`
    sits. What was grafted is recorded, because a field iyzico describes in
    one place and not another is worth somebody looking at.
    """
    chosen: dict[frozenset[tuple[str, str]], tuple[str, dict]] = {}
    for source, fragment in found:
        key = operations_in(fragment)
        if not key:
            continue
        held = chosen.get(key)
        if held is None or (documented_fields(fragment), is_english(source)) > (
            documented_fields(held[1]),
            is_english(held[0]),
        ):
            chosen[key] = (source, fragment)

    notes: list[str] = []
    for key, (base_source, base) in sorted(chosen.items(), key=lambda pair: pair[1][0]):
        for verb, path in sorted(key):
            for source, fragment in found:
                if fragment is base or (verb, path) not in operations_in(fragment):
                    continue
                added: list[str] = []
                for mine, theirs in zip(
                    schemas_of(base, verb, path), schemas_of(fragment, verb, path)
                ):
                    graft(base, mine, theirs, fragment, added)
                if added:
                    language = "Turkish" if is_english(base_source) else "English"
                    notes.append(
                        f"{verb.upper()} {path}: {', '.join(sorted(set(added)))} "
                        f"documented only in {language}, grafted on"
                    )

    # Two different things, and calling both "Turkish only" said something
    # false about the second: mass payout's retrieve is documented in English
    # too, and the Turkish fragment was merely the fuller one.
    documented = {
        (verb, path): {is_english(source) for source, fragment in found if (verb, path) in operations_in(fragment)}
        for key in chosen
        for verb, path in key
    }
    only_turkish = sorted(
        f"{verb.upper()} {path}" for (verb, path), langs in documented.items() if langs == {False}
    )
    kept_turkish = sorted(
        f"{verb.upper()} {path}"
        for key, (source, _) in chosen.items()
        if not is_english(source)
        for verb, path in key
        if documented.get((verb, path)) != {False}
    )
    if only_turkish:
        notes.append(f"documented in Turkish only: {', '.join(only_turkish)}")
    if kept_turkish:
        notes.append(
            "documented in both, kept from the Turkish page because it is the "
            f"fuller one: {', '.join(kept_turkish)}"
        )
    return sorted(chosen.values(), key=lambda pair: pair[0]), notes


def merge(group: str, found: list[tuple[str, dict]]) -> tuple[dict, list[str]]:
    spec = {
        "openapi": "3.0.3",
        "info": {
            "title": f"iyzico — {group}",
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
        "tags": [],
        "paths": {},
        "components": {},
    }
    notes: list[str] = []
    seen_tags: set[str] = set()

    seen_operation_ids: set[str] = set()

    for source, fragment in found:
        repair(fragment)
        for template, _ in fragment.get("paths", {}).items():
            notes.extend(
                f"{source}: {n}" for n in repair_schema_dialect(fragment, template, [])
            )
        prefix = base_path(fragment)

        # Every component section, not just schemas: a fragment that keeps its
        # auth header in components/parameters leaves a dangling $ref if only
        # schemas are carried over.
        renames: dict[tuple[str, str], str] = {}
        for section, items in fragment.get("components", {}).items():
            if not isinstance(items, dict):
                continue
            held = spec["components"].setdefault(section, {})
            for name, item in items.items():
                existing = held.get(name)
                if existing is None or existing == item:
                    held[name] = item
                    continue
                if section == "securitySchemes":
                    notes.append(f"security scheme {name} redefined by {source}, kept the first")
                    continue
                # Two pages describe different shapes under one name. Both are
                # kept; dropping either loses an endpoint's request body.
                suffix = 2
                while f"{name}{suffix}" in held:
                    if held[f"{name}{suffix}"] == item:
                        break
                    suffix += 1
                renames[(section, name)] = f"{name}{suffix}"
                held[f"{name}{suffix}"] = item
                notes.append(f"{section} {name} redefined by {source}, kept as {name}{suffix}")

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
                # A Response Object must carry a description; the fragments
                # routinely give only `content`.
                for code, response in (operation.get("responses") or {}).items():
                    if isinstance(response, dict) and "description" not in response:
                        response["description"] = DESCRIPTIONS.get(
                            str(code)[0], "Undocumented by iyzico"
                        )
                operation.setdefault("x-iyzico-source", source)
                # A fragment that names a host of its own. iyzico's docs carry
                # somebody else's API as well as their own — the PayPOS
                # operations are Paynet's, at api.paynet.com.tr — and writing
                # iyzico's servers over the whole document said every one of
                # them lives at api.iyzipay.com. Carried onto the operation,
                # which is what OpenAPI's per-operation `servers` is for.
                foreign = [
                    server
                    for server in fragment.get("servers", [])
                    if isinstance(server, dict)
                    and not server.get("url", "").startswith((PRODUCTION, SANDBOX))
                ]
                if foreign:
                    operation.setdefault("servers", foreign)
                    hosts = ", ".join(sorted({s.get("url", "") for s in foreign}))
                    notes.append(
                        f"{verb.upper()} {full} is served from {hosts}, not iyzico's own host"
                    )
                # iyzico reuses one operationId across two operations — the
                # In-Store payment query is documented as both a GET and a POST.
                op_id = operation.get("operationId")
                if op_id:
                    unique, n = op_id, 2
                    while unique in seen_operation_ids:
                        unique, n = f"{op_id}{n}", n + 1
                    if unique != op_id:
                        notes.append(f"operationId {op_id} reused by {source}, renamed {unique}")
                        operation["operationId"] = unique
                    seen_operation_ids.add(unique)
                if "security" not in operation and fragment.get("security"):
                    operation["security"] = fragment["security"]
                existing = spec["paths"].setdefault(full, {}).get(verb)
                if existing is None:
                    spec["paths"][full][verb] = operation
                elif existing != operation:
                    notes.append(f"{verb.upper()} {full} redefined by {source}, kept the first")

    for section in [k for k, v in spec["components"].items() if not v]:
        del spec["components"][section]

    spec["tags"].sort(key=lambda t: t["name"])
    spec["paths"] = dict(sorted(spec["paths"].items()))
    spec["components"]["schemas"] = dict(sorted(spec["components"]["schemas"].items()))
    return spec, notes


def authentication(spec: dict) -> dict:
    """How each operation says it is authenticated.

    Most of them do not use a `securityScheme`: they declare the `Authorization`
    header as an ordinary parameter instead, which is why an earlier version of
    this script reported them as documenting no authentication at all.
    """
    named = {
        name
        for name, parameter in spec["components"].get("parameters", {}).items()
        if isinstance(parameter, dict) and parameter.get("name", "").lower() == "authorization"
    }
    by_scheme, by_parameter, neither = [], [], []
    for path, operations in spec["paths"].items():
        for verb, operation in operations.items():
            label = f"{verb.upper()} {path}"
            parameters = operation.get("parameters") or []
            refs = {
                p.get("$ref", "").rsplit("/", 1)[-1] for p in parameters if isinstance(p, dict)
            }
            inline = any(
                isinstance(p, dict) and p.get("name", "").lower() == "authorization"
                for p in parameters
            )
            if "security" in operation:
                by_scheme.append(label)
            elif refs & named or inline:
                by_parameter.append(label)
            else:
                neither.append(label)
    return {
        "by_security_scheme": sorted(by_scheme),
        "by_authorization_parameter": sorted(by_parameter),
        "undeclared": sorted(neither),
    }


def main() -> None:
    day = sys.argv[1] if len(sys.argv) > 1 else datetime.date.today().isoformat()
    urls = pages()
    with ThreadPoolExecutor(max_workers=8) as pool:
        bodies = list(pool.map(fetch, urls))

    every: list[tuple[str, dict]] = []
    page_notes: list[str] = []
    for url, body in zip(urls, bodies, strict=True):
        found = list(fragments(body))
        every.extend((url, fragment) for fragment in found)
        # A page whose fragments disagree about which version of the API they
        # belong to. iyzico's In-Store V3 page carries one fragment declaring a
        # v2 server beside six declaring v3, which is how /v2/in-store/crypt/decrypt
        # reached this file. Both are kept — the disagreement is theirs — but a
        # reader deciding which path to call should know it is there.
        bases = {base_path(fragment) for fragment in found}
        if len(bases) > 1:
            page_notes.append(
                f"{url}: fragments on one page declare different servers "
                f"({', '.join(sorted(b or '/' for b in bases))}), all kept"
            )

    # Deduplicate across languages before grouping, or the same operation lands
    # in two groups under two names.
    every, language_notes = combine_languages(every)
    language_notes = page_notes + language_notes

    by_area: dict[str, list[tuple[str, dict]]] = {}
    for source, fragment in every:
        groups = {api_group(path) for _, path in operations_in(fragment)}
        for group in groups:
            by_area.setdefault(group, []).append((source, fragment))

    import yaml

    index = {
        "fetched": day,
        "source": INDEX,
        "pages_swept": len(urls),
        "pages_with_fragments": sum(len({s for s, _ in v}) for v in by_area.values()),
        "fragments": sum(len(v) for v in by_area.values()),
        "language": "both; one fragment per operation, with what the other language alone documents grafted on",
        "upstream_sha256": hashlib.sha256("".join(bodies).encode()).hexdigest(),
        "notes": language_notes,
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
            "authentication": authentication(spec),
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
