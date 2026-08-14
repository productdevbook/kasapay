import json, re, sys, hashlib, subprocess, datetime, pathlib

SRC = "https://docs.iyzico.com/urunler/ceppos-app2app/in-store-api-v3.md"

def fetch():
    return subprocess.run(["curl","-sSL",SRC],capture_output=True,text=True,check=True).stdout

def blocks(md):
    for m in re.finditer(r'^\{"openapi".*$', md, re.M):
        yield json.loads(m.group(0))

NUM = {"BigDecimal": ("string", "decimal"), "Long": ("integer", "int64")}

def fix(node):
    if isinstance(node, dict):
        t = node.get("type")
        if t in NUM:
            node["type"], node["format"] = NUM[t]
        for v in node.values():
            fix(v)
    elif isinstance(node, list):
        for v in node:
            fix(v)
    return node

def merge(bs):
    out = {
        "openapi": "3.0.3",
        "info": {
            "title": "iyzico In-Store API",
            "version": "3.0",
            "description": f"Reconstructed from the per-endpoint schemas embedded in {SRC}.",
        },
        "servers": [
            {"url": "https://api.iyzipay.com/v3/in-store", "description": "Production"},
            {"url": "https://sandbox-api.iyzipay.com/v3/in-store", "description": "Sandbox"},
        ],
        "tags": [], "security": [{"ApiKeyAuth": [], "SecretKeyAuth": [], "MerchantIdAuth": []}],
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
    seen_tags = set()
    for b in bs:
        fix(b)
        legacy = any(s["url"].endswith("/v2/in-store") for s in b.get("servers", []))
        for t in b.get("tags", []):
            if t["name"] not in seen_tags:
                seen_tags.add(t["name"]); out["tags"].append(t)
        for name, schema in b["components"]["schemas"].items():
            prev = out["components"]["schemas"].get(name)
            if prev is not None and prev != schema:
                raise SystemExit(f"schema conflict: {name}")
            out["components"]["schemas"][name] = schema
        for path, ops in b["paths"].items():
            for verb, op in ops.items():
                op.pop("detail", None)
                if legacy:
                    op["servers"] = b["servers"]
                if verb in out["paths"].setdefault(path, {}):
                    raise SystemExit(f"operation conflict: {verb} {path}")
                out["paths"][path][verb] = op
    return out

if __name__ == "__main__":
    md = fetch()
    spec = merge(blocks(md))
    day = sys.argv[1] if len(sys.argv) > 1 else datetime.date.today().isoformat()
    d = pathlib.Path(__file__).resolve().parent.parent / "specs" / "iyzico"
    d.mkdir(parents=True, exist_ok=True)
    try:
        import yaml
        text = yaml.safe_dump(spec, allow_unicode=True, sort_keys=False, width=100)
        ext = "yaml"
    except ImportError:
        text = json.dumps(spec, ensure_ascii=False, indent=2) + "\n"
        ext = "json"
    (d / f"{day}.{ext}").write_text(text, encoding="utf-8")
    latest = d / f"latest.{ext}"
    latest.unlink(missing_ok=True)
    latest.symlink_to(f"{day}.{ext}")
    meta = {"source": SRC, "fetched": day,
            "upstream_sha256": hashlib.sha256(md.encode()).hexdigest(),
            "operations": sorted(f"{v.upper()} {p}" for p, ops in spec["paths"].items() for v in ops)}
    (d / f"{day}.meta.json").write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    print(f"{d}/{day}.{ext}"); print("\n".join(meta["operations"]))
