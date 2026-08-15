"""Which documented operations `crates/kasapay-*` actually calls.

    python3 scripts/coverage.py

For iyzico this is a command instead of a hand count, because a hand count is
wrong the moment a module lands: issue #8 has had its title edited five times
in one day, each time by someone re-counting `pub async fn` and hoping.

## What "documented" means

Every (method, path) under `paths:` in `specs/iyzico/*/latest.yaml`, grouped
by the directory it lives in — the grouping `merge_iyzico.py` already did, by
API path rather than documentation URL. See `specs/README.md`.

## What "implemented" means, and how this tells it from a fixture

The honest signal is which endpoint strings `crates/kasapay-iyzico/src`
actually calls, not which ones its comments mention. Three things make that
harder than a plain grep for `"/`:

- **A relative path against a base URL.** `in_store::Client::endpoint`
  joins e.g. `"user"` against `https://api.iyzipay.com/v3/in-store/`; the
  literal alone is not the path iyzico documents.
- **A path built with `format!`.** `mass::Client` keeps
  `const CANCEL: &str = "/v1/mass/payout/cancel"` and calls
  `format!("{CANCEL}/{}", request_id)`, so the literal iyzico documents
  (`/v1/mass/payout/cancel/{requestId}`) never appears as one string anywhere
  in the source.
- **A string that is not a call at all.** `/payment/auth` and
  `/payment/detail` both sit in `signing.rs`, as fixtures for the `IYZWSv2`
  signature tests — proof the signing code handles those paths, not proof
  anything calls them. `#[cfg(test)] mod tests { ... }` blocks are stripped
  before anything else runs, crate-wide, for exactly this reason. Comments
  are stripped too: a doc comment that mentions a path in prose (`login.rs`'s
  module doc walks through `/authorize`, `/token`, `/token/refresh`) is not a
  call either, even where the call two lines below happens to be real.

So this script:

1. Strips every comment (`//`, `///`, `//!`) with a string-literal-aware
   scanner, so a URL containing `//` is not mistaken for one.
2. Deletes every `mod tests { ... }` block by brace-counting from there.
3. Collects `const NAME: &str = "...";` bindings, per file.
4. Reads the path argument off every call that plausibly sends a request —
   `.post(`, `.get(`, `.delete(`, `.put(`, `.patch(`, `self.endpoint(`, any
   method at all whose first argument resolves to a path, and any
   `Method::X, path, ...` (not
   anchored to a function name, since `kasapay-mollie`'s equivalent is
   `.send(reqwest::Method::POST, "/v2/payments", ...)`, not `.request(` —
   resolving a bare identifier to a same-file `const` when one matches.
5. Separately collects every `format!("...")` template in what is left, since
   a helper like `mass::client::cancel`'s path is never a literal — only a
   template. `{CONST}` is substituted from step 3; anything else inside `{}`
   becomes a one-segment wildcard. Templates that read as prose rather than a
   path (contain a space, or a character outside `[A-Za-z0-9_./{}-]`) are
   dropped — this is what keeps `format!("iyzico pages links from page 1 in
   counts of 1 to {MAX_COUNT}")` out of the candidate pool.
6. Matches a candidate against a documented path **segment for segment,
   exactly** — not by suffix. `self.endpoint("user")` on `in_store::Client`
   resolves to `v3/in-store/user` first, using that module's own
   `Config::PRODUCTION` to restore the prefix its base URL hides, and *then*
   has to equal the documented path exactly. A plain suffix match was tried
   first and over-counted: `/payment/refund` (classic API, 2 segments) is a
   trailing match for `/v3/in-store/payment/refund` and
   `/v2/terminal-host/gmu/payment/refund` too, and both came back "reached"
   as a result. The one deliberate exception is a candidate whose *first*
   segment is a wildcard — `*/status/*`, from `iyzilink`'s
   `format!("{}/status/{}", product_path(token)?, ...)` — because there the
   unresolved part is a function call of unknown length, not one segment, and
   only the tail can be trusted. A **single-segment** candidate additionally
   has to be at least four letters and carry a specific HTTP verb (from
   `.post(`/`.get(`/... or `Method::X`), so a bare `format!` template whose
   call site this script could not resolve never counts alone.

## What this heuristic cannot see

- A `format!` template resolved by tracing only as far as the string in the
  source, not the call site that consumes it — so a candidate coming only
  from step 5 counts on **path alone**, any documented HTTP method, once it
  has two or more segments. This can credit a method nothing actually calls,
  on a path where a sibling method genuinely is implemented.
- Matching is global, not per-module: a candidate is checked against every
  documented path, not just the ones its own client plausibly speaks. Two
  unrelated areas whose full, exact paths coincide would double-count. None
  do, today — worth re-checking after a refetch.
- It cannot tell "reads the documented request/response shape and calls it
  correctly" from "sends something to the right URL". It answers only the
  question in the issue title: which endpoints are reached at all.
"""

from __future__ import annotations

import collections
import glob
import json
import pathlib
import re
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
IYZICO_SRC = ROOT / "crates" / "kasapay-iyzico" / "src"
MOLLIE_SRC = ROOT / "crates" / "kasapay-mollie" / "src"

WILDCARD = "\x01"

# ---------------------------------------------------------------------------
# Documented: specs/iyzico/*/latest.yaml
# ---------------------------------------------------------------------------


def documented_iyzico() -> dict[str, list[tuple[str, str]]]:
    """area -> [(METHOD, /path), ...], one entry per operation."""
    by_area: dict[str, list[tuple[str, str]]] = {}
    for path in sorted(glob.glob(str(ROOT / "specs" / "iyzico" / "*" / "latest.yaml"))):
        area = pathlib.Path(path).parent.name
        doc = yaml.safe_load(pathlib.Path(path).read_text(encoding="utf-8"))
        ops: list[tuple[str, str]] = []
        for route, methods in (doc.get("paths") or {}).items():
            if not isinstance(methods, dict):
                continue
            for verb in methods:
                if verb.lower() in {
                    "get",
                    "post",
                    "put",
                    "delete",
                    "patch",
                    "head",
                    "options",
                }:
                    ops.append((verb.upper(), route))
        by_area[area] = sorted(ops)
    return by_area


# ---------------------------------------------------------------------------
# Source cleanup: comments and #[cfg(test)] mod tests { ... } blocks out
# ---------------------------------------------------------------------------


def strip_comments(text: str) -> str:
    """Removes `//...` and `/*...*/`, leaving string literals untouched.

    A plain regex would eat the `//` in `"https://api.iyzipay.com/"`; this
    walks the text tracking whether it is inside a string so it does not.
    """
    out: list[str] = []
    i, n = 0, len(text)
    in_string = False
    while i < n:
        c = text[i]
        if in_string:
            out.append(c)
            if c == "\\" and i + 1 < n:
                out.append(text[i + 1])
                i += 2
                continue
            if c == '"':
                in_string = False
            i += 1
            continue
        if c == '"':
            in_string = True
            out.append(c)
            i += 1
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            while i < n and text[i] != "\n":
                i += 1
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            i += 2
            while i + 1 < n and not (text[i] == "*" and text[i + 1] == "/"):
                i += 1
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def strip_test_modules(text: str) -> str:
    """Deletes every `mod tests { ... }` block, brace-matched from `{`."""
    out: list[str] = []
    i, n = 0, len(text)
    pattern = re.compile(r"\bmod\s+tests\s*\{")
    while i < n:
        m = pattern.search(text, i)
        if not m:
            out.append(text[i:])
            break
        out.append(text[i : m.start()])
        depth = 1
        j = m.end()
        while j < n and depth:
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
            j += 1
        i = j
    return "".join(out)


def cleaned_source(path: pathlib.Path) -> str:
    return strip_test_modules(strip_comments(path.read_text(encoding="utf-8")))


# ---------------------------------------------------------------------------
# Implemented: endpoint strings actually reached
# ---------------------------------------------------------------------------

STR = r'"(?:[^"\\]|\\.)*"'
CONST_RE = re.compile(r'const\s+(\w+)\s*:\s*&(?:\'static\s+)?str\s*=\s*(' + STR + r")\s*;")

# (method-bearing call, capturing group with a bare identifier or a literal)
METHOD_CALL_RE = re.compile(
    r"\.(post|get|delete|put|patch)(?:::<[^>]*>)?\(\s*"
    r'(?:self\.endpoint\(\s*("(?:[^"\\]|\\.)*")\s*\)|(&?\w+)|("(?:[^"\\]|\\.)*"))'
)

# Any call that takes an explicit `Method::X` (or `reqwest::Method::X`) enum
# followed by the path — `classic::Client::request`, `mollie::Client::send`,
# whatever the next module calls it — not anchored to one method name, since
# the enum argument is what says "this is the path", not the function name.
REQUEST_CALL_RE = re.compile(
    r"(?:^|[\s(,])(?:reqwest::)?Method::(\w+)\s*,\s*(&?\w+|" + STR + r")",
    re.MULTILINE,
)
# Any method whose first argument is a path. Not a list of names: naming them
# made a module rename itself to be counted, which is the measurement changing
# the thing measured. `add` ignores anything that does not resolve to a string,
# so `.expect("valid url")` costs nothing. The verb is unknown, because an
# unrecognised helper's name does not say which one it sends.
HELPER_CALL_RE = re.compile(r"\.(\w+)\(\s*(" + STR + r"|&?\w+)")
FORMAT_RE = re.compile(r'format!\(\s*"((?:[^"\\]|\\.)*)"')

PATH_CHARSET = re.compile(r"^[A-Za-z0-9_.\-/{}\x01]+$")
PLACEHOLDER_RE = re.compile(r"\{[^{}]*\}")


def unquote(literal: str) -> str:
    return literal[1:-1] if literal.startswith('"') and literal.endswith('"') else literal


def resolve(raw: str, consts: dict[str, str]) -> str | None:
    """Substitutes known `const`s, wildcards the rest, or rejects non-paths."""

    def sub(m: re.Match[str]) -> str:
        name = m.group(0)[1:-1]
        return consts.get(name, WILDCARD)

    resolved = PLACEHOLDER_RE.sub(sub, raw)
    resolved = resolved.split("?", 1)[0]  # drop a query string
    resolved = resolved.strip("/")
    if not resolved or " " in resolved or not PATH_CHARSET.match(resolved):
        return None
    return resolved


Candidate = collections.namedtuple("Candidate", "method segments source")

BASE_URL_RE = re.compile(r'https?://[^/"]+(/[^"]*)?')


def module_base_path(module_dir: pathlib.Path) -> tuple[str, ...]:
    """The path segments baked into a module's `Config::PRODUCTION`, if any.

    `in_store::Client` joins its calls against
    `https://api.iyzipay.com/v3/in-store/` — so `self.endpoint("user")` never
    appears as `/v3/in-store/user` anywhere in the source, and matching it
    against the documented path needs this prefix restored first. Everything
    else in this crate points at the bare host, so this is `()` for them —
    a plain literal is already the whole path and gets no free pass to match
    a same-suffix path three products over (`/payment/refund` is not
    `/v3/in-store/payment/refund`; matching it against one lost the count by
    two before this existed).
    """
    for rs_file in sorted(module_dir.rglob("*.rs")):
        text = cleaned_source(rs_file)
        m = re.search(r'const\s+PRODUCTION\s*:\s*&\'static\s+str\s*=\s*"([^"]+)"', text)
        if not m:
            continue
        url_match = BASE_URL_RE.fullmatch(m.group(1))
        if not url_match or not url_match.group(1):
            return ()
        return tuple(seg for seg in url_match.group(1).split("/") if seg)
    return ()


def candidates_from_file(path: pathlib.Path, base_path: tuple[str, ...]) -> list[Candidate]:
    text = cleaned_source(path)
    consts = {name: unquote(value) for name, value in CONST_RE.findall(text)}
    found: list[Candidate] = []

    def add(method: str | None, raw: str, where: str, *, relative: bool = False) -> None:
        raw = raw.lstrip("&")
        if raw.startswith('"'):
            literal = unquote(raw)
        elif raw in consts:
            literal = consts[raw]
        else:
            return  # a local variable this script does not trace; see FORMAT_RE below
        resolved = resolve(literal, consts)
        if resolved is None:
            return
        segments = resolved.split("/")
        if relative:
            segments = list(base_path) + segments
        found.append(Candidate(method, tuple(segments), where))

    for m in METHOD_CALL_RE.finditer(text):
        verb, endpoint_lit, ident, lit = m.groups()
        if endpoint_lit is not None:
            # `self.endpoint("...")`: relative to this module's base path.
            add(verb.upper(), endpoint_lit, f"{path.relative_to(ROOT)}:.{verb}(self.endpoint(...))", relative=True)
        else:
            add(verb.upper(), ident or lit, f"{path.relative_to(ROOT)}:.{verb}(...)")

    for m in REQUEST_CALL_RE.finditer(text):
        verb, arg = m.groups()
        add(verb.upper(), arg, f"{path.relative_to(ROOT)}:Method::{verb}, ...")

    for m in HELPER_CALL_RE.finditer(text):
        helper, arg = m.groups()
        add(None, arg, f"{path.relative_to(ROOT)}:.{helper}(...)")

    # format!() templates: method is whatever the call site used, which this
    # script does not trace back to here — recorded as method=None, "any".
    for m in FORMAT_RE.finditer(text):
        sub_pattern = PLACEHOLDER_RE.sub(lambda mm: consts.get(mm.group(0)[1:-1], WILDCARD), m.group(1))
        resolved = resolve(sub_pattern, consts)
        if resolved is None:
            continue
        found.append(Candidate(None, tuple(resolved.split("/")), f"{path.relative_to(ROOT)}:format!(...)"))

    return found


def implemented_candidates(src_root: pathlib.Path) -> list[Candidate]:
    out: list[Candidate] = []
    module_bases: dict[pathlib.Path, tuple[str, ...]] = {}
    for path in sorted(src_root.rglob("*.rs")):
        module_dir = path.parent
        if module_dir not in module_bases:
            module_bases[module_dir] = module_base_path(module_dir)
        out.extend(candidates_from_file(path, module_bases[module_dir]))
    return out


def doc_segments(route: str) -> tuple[str, ...]:
    parts = route.strip("/").split("/")
    return tuple(WILDCARD if p.startswith("{") and p.endswith("}") else p for p in parts)


def path_only_match(cand: Candidate, segments: tuple[str, ...]) -> bool:
    """Whether a candidate's segments identify one documented path, ignoring method.

    A candidate is, by construction, already the *whole* path this script
    could reconstruct — a literal, a resolved `const`, or a `self.endpoint`
    relative path with its module's base restored. So it has to match
    **exactly**, segment for segment (a `WILDCARD` standing for any one
    segment): matching only the tail let `/payment/refund` (classic API)
    count as `/v3/in-store/payment/refund` and `/v2/terminal-host/gmu/payment/refund`
    too, because both happen to end the same way.

    The one exception is a candidate whose *first* segment is a `WILDCARD`:
    that only happens when a `format!` template's placeholder was filled by
    a function call this script does not evaluate (`product_path(token)?`,
    which itself returns a multi-segment path) rather than a single
    identifier — so the missing prefix is of unknown length, not one
    segment, and only the tail can be trusted. `iyzilink::update`'s
    `format!("{}/status/{}", product_path(...), ...)` is why this exists.
    """
    n = len(cand.segments)
    if n == 0 or n > len(segments):
        return False
    if n == 1 and (cand.segments[0] == WILDCARD or len(cand.segments[0]) < 4):
        return False
    if cand.segments[0] == WILDCARD:
        tail = segments[-n:]
        return all(c == WILDCARD or c == s for c, s in zip(cand.segments, tail))
    if n != len(segments):
        return False
    return all(c == WILDCARD or c == s for c, s in zip(cand.segments, segments))


def candidate_matches(cand: Candidate, method: str, segments: tuple[str, ...]) -> bool:
    if cand.method is not None and cand.method != method:
        return False
    if cand.method is None and len(cand.segments) == 1:
        # No verb to anchor a single-segment guess to — too weak to count.
        return False
    return path_only_match(cand, segments)


def match_iyzico(
    documented: dict[str, list[tuple[str, str]]], candidates: list[Candidate]
) -> tuple[dict[tuple[str, str, str], list[str]], set[tuple[str, str, str]]]:
    """(area, method, path) -> [evidence, ...] for every documented op that matched."""
    evidence: dict[tuple[str, str, str], list[str]] = collections.defaultdict(list)
    for area, ops in documented.items():
        for method, route in ops:
            segs = doc_segments(route)
            for cand in candidates:
                if candidate_matches(cand, method, segs):
                    evidence[(area, method, route)].append(cand.source)
    return evidence, set(evidence)


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------


def print_iyzico_report() -> int:
    documented = documented_iyzico()
    candidates = implemented_candidates(IYZICO_SRC)
    evidence, matched = match_iyzico(documented, candidates)

    total_doc = sum(len(v) for v in documented.values())
    total_impl = len(matched)

    print("## iyzico\n")
    print(f"{'area':<16}{'implemented':>13}{'documented':>13}")
    for area in sorted(documented):
        ops = documented[area]
        impl = sum(1 for op in ops if (area, *op) in matched)
        print(f"{area:<16}{impl:>13}{len(ops):>13}")
    print(f"{'total':<16}{total_impl:>13}{total_doc:>13}\n")

    for area in sorted(documented):
        remaining = [op for op in documented[area] if (area, *op) not in matched]
        if not remaining:
            continue
        print(f"### {area}: not yet reached ({len(remaining)})")
        for method, route in remaining:
            print(f"    {method:<7}{route}")
        print()

    claimed = 42
    print(f"## against issue #8's claim of {claimed}\n")
    if total_impl == claimed:
        print(f"Agrees: {total_impl} of {total_doc}.")
    else:
        print(f"Disagrees: this script counts {total_impl} of {total_doc}, issue #8 says {claimed}.")
        print(
            "The difference is worth reading as a diff, not a verdict — a hand count and a"
            " heuristic can each be wrong in different directions. Rerun with the evidence"
            " below to see which operations moved."
        )
    print()
    print("Evidence, one call site per matched operation (first match only):")
    for key in sorted(matched):
        area, method, route = key
        print(f"    {method:<7}{route:<55}{evidence[key][0]}")
    return 0


def print_stripe_report() -> None:
    path = ROOT / "specs" / "stripe" / "latest.yaml"
    if not path.exists():
        print("## Stripe\n\nspecs/stripe/latest.yaml is missing.\n")
        return
    doc = yaml.safe_load(path.read_text(encoding="utf-8"))
    ops = sum(len(m) for m in (doc.get("paths") or {}).values() if isinstance(m, dict))
    print("## Stripe\n")
    print(
        f"specs/stripe/latest.yaml already **is** the subset kasapay maps — {len(doc.get('paths', {}))} paths, "
        f"{ops} operations, resolved from Stripe's ~7MB document down to what a PaymentIntent flow touches. "
        "It was not generated independently of the crate, so 'documented vs implemented' is circular here: "
        "kasapay-stripe wraps async-stripe rather than calling these paths itself, so there is no endpoint "
        "string in crates/kasapay-stripe/src to match against them at all. See specs/README.md.\n"
    )


def print_paytr_report() -> None:
    path = ROOT / "specs" / "paytr" / "latest.yaml"
    if not path.exists():
        print("## PayTR\n\nspecs/paytr/latest.yaml is missing.\n")
        return
    doc = yaml.safe_load(path.read_text(encoding="utf-8"))
    pages = doc.get("pages") or []
    tables = sum(len(p.get("tables") or []) for p in pages)
    rows = sum(len(t.get("rows") or []) for p in pages for t in (p.get("tables") or []))
    print("## PayTR\n")
    print(
        f"specs/paytr/latest.yaml records {len(pages)} pages, {tables} field tables, {rows} field rows — "
        "PayTR publishes no machine-readable list of operations, so there is nothing here shaped like "
        "'documented operations' to compare against the crate. See specs/README.md.\n"
    )


def print_mollie_report() -> None:
    metas = sorted(glob.glob(str(ROOT / "specs" / "mollie" / "*.meta.json")))
    if not metas:
        print("## Mollie\n\nNo specs/mollie/*.meta.json found.\n")
        return
    meta = json.loads(pathlib.Path(metas[-1]).read_text(encoding="utf-8"))
    kept_paths: list[str] = meta.get("kept_paths", [])
    operations: list[str] = meta.get("operations", [])
    print("## Mollie\n")
    print(
        f"specs/mollie/{pathlib.Path(metas[-1]).name} keeps a meta only (Mollie's OpenAPI document is "
        "CC-BY-NC-SA and this repository is MIT) — {} kept paths, {} operationIds, no method-per-operation "
        "recorded. So this can check path coverage, the same call-site heuristic as iyzico, but not "
        "per-operation, since knowing which of the {} operationIds is GET vs POST vs DELETE needs the "
        "document itself, which is deliberately not kept.\n".format(
            len(kept_paths), len(operations), len(operations)
        )
    )
    if not MOLLIE_SRC.exists():
        print("crates/kasapay-mollie/src does not exist here.\n")
        return
    candidates = implemented_candidates(MOLLIE_SRC)
    print(f"{'path':<45}{'reached':>9}")
    reached = 0
    for route in kept_paths:
        segs = doc_segments(route)
        hit = any(path_only_match(c, segs) for c in candidates)
        reached += hit
        print(f"{route:<45}{'yes' if hit else 'no':>9}")
    print(f"\n{reached} of {len(kept_paths)} kept paths have a matching call site in crates/kasapay-mollie/src.\n")
    print(f"operationIds documented, for reference: {', '.join(sorted(operations))}\n")


def main() -> int:
    print_iyzico_report()
    print_stripe_report()
    print_paytr_report()
    print_mollie_report()
    return 0  # this reports, it does not gate


if __name__ == "__main__":
    sys.exit(main())
