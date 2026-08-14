import json, sys, hashlib, subprocess, datetime, pathlib, yaml

SRC = "https://raw.githubusercontent.com/stripe/openapi/master/openapi/spec3.json"
# Only the operations kasapay maps onto. The full spec is ~7MB and its diffs are unreadable.
KEEP = [
    "/v1/payment_intents", "/v1/payment_intents/{intent}",
    "/v1/payment_intents/{intent}/confirm", "/v1/payment_intents/{intent}/cancel",
    "/v1/refunds", "/v1/refunds/{refund}",
]

def resolve(spec, keep):
    out, queue, seen = {}, list(keep), set()
    while queue:
        name = queue.pop()
        if name in seen:
            continue
        seen.add(name)
        schema = spec["components"]["schemas"].get(name)
        if schema is None:
            continue
        out[name] = schema
        queue.extend(refs(schema))
    return dict(sorted(out.items()))

def refs(node):
    if isinstance(node, dict):
        r = node.get("$ref")
        if isinstance(r, str) and r.startswith("#/components/schemas/"):
            yield r.rsplit("/", 1)[1]
        for v in node.values():
            yield from refs(v)
    elif isinstance(node, list):
        for v in node:
            yield from refs(v)

if __name__ == "__main__":
    raw = subprocess.run(["curl", "-sSL", SRC], capture_output=True, check=True).stdout
    spec = json.loads(raw)
    paths = {p: spec["paths"][p] for p in KEEP if p in spec["paths"]}
    missing = [p for p in KEEP if p not in spec["paths"]]
    sub = {
        "openapi": spec["openapi"],
        "info": spec["info"] | {"description": f"Subset of {SRC}, limited to the operations kasapay maps."},
        "servers": spec.get("servers", []),
        "paths": paths,
        "components": {"schemas": resolve(spec, list(refs(paths)))},
    }
    day = sys.argv[1] if len(sys.argv) > 1 else datetime.date.today().isoformat()
    d = pathlib.Path(__file__).resolve().parent.parent / "specs" / "stripe"
    d.mkdir(parents=True, exist_ok=True)
    # Only meta is dated here: the subset is still ~1.7MB because PaymentIntent reaches
    # most of Stripe's schemas, and a dated copy per fetch buys diffs nobody can read.
    (d / "latest.yaml").write_text(yaml.safe_dump(sub, allow_unicode=True, sort_keys=False, width=100), encoding="utf-8")
    (d / f"{day}.meta.json").write_text(json.dumps({
        "source": SRC, "fetched": day,
        "api_version": spec["info"].get("version"),
        "upstream_sha256": hashlib.sha256(raw).hexdigest(),
        "kept_paths": list(paths), "missing_paths": missing,
        "schema_count": len(sub["components"]["schemas"]),
    }, indent=2) + "\n", encoding="utf-8")
    print(f"api_version={spec['info'].get('version')} paths={len(paths)} schemas={len(sub['components']['schemas'])} missing={missing}")
