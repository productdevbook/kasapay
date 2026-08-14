"""Records what PayTR's documentation says, dated, so that changes to it show up.

PayTR publishes no OpenAPI document and no machine-readable description of any
kind. What they do publish is a field table per endpoint — name, type, whether
it is mandatory, **whether it goes into the token hash**, the description and
the constraints — and that last-but-two column is the whole reason this exists.
The hash is what a request is signed with, and the field order it covers had to
be derived from PayTR's PHP and Python SDKs once already. A column that changes
without anyone noticing is a signing bug that looks like bad credentials.

So this sweeps their sitemap, pulls the tables out of every API page in both
languages, and writes one dated YAML. It is a record of documentation, not an
API description, and the file says so: nothing here is OpenAPI and nothing
generates code from it.

    python3 scripts/fetch_paytr.py            # dated today
    python3 scripts/fetch_paytr.py 2026-08-14 # dated deliberately
"""

import datetime
import hashlib
import html
import pathlib
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor

import yaml

SITEMAP = "https://dev.paytr.com/sitemap.xml"
ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "specs" / "paytr"

# The parts of dev.paytr.com that describe an API call. The rest is shopping-cart
# plugins, test card numbers and sales copy, none of which the adapters read.
API_SECTIONS = (
    "iframe-api",
    "direkt-api",
    "iade-api",
    "durum-sorgu",
    "havale-eft-iframe-api",
    "bkm-express",
    "hata-kodlari",
    "link-api",
    "platform-transfer",
)

TAG = re.compile(r"<[^>]+>")
ROW = re.compile(r"<tr[^>]*>(.*?)</tr>", re.S)
CELL = re.compile(r"<t[hd][^>]*>(.*?)</t[hd]>", re.S)
TABLE = re.compile(r"<table.*?</table>", re.S)
HEADING = re.compile(r"<h[1-3][^>]*>(.*?)</h[1-3]>", re.S)


def fetch(url: str) -> str:
    return subprocess.run(
        ["curl", "-sSL", "--max-time", "30", url],
        capture_output=True,
        text=True,
        check=True,
    ).stdout


def text_of(fragment: str) -> str:
    return " ".join(html.unescape(TAG.sub(" ", fragment)).split())


def pages() -> list[str]:
    found = re.findall(r"<loc>([^<]+)</loc>", fetch(SITEMAP))
    wanted = []
    for url in found:
        path = url.removeprefix("https://dev.paytr.com/").strip("/")
        section = path.removeprefix("en/").split("/")[0]
        if section in API_SECTIONS:
            wanted.append(url)
    return sorted(set(wanted))


def tables_in(markup: str) -> list[dict]:
    """Every table on the page, with the nearest heading above it."""
    out = []
    for table in TABLE.finditer(markup):
        before = markup[: table.start()]
        headings = HEADING.findall(before)
        rows = []
        for row in ROW.findall(table.group(0)):
            cells = [text_of(cell) for cell in CELL.findall(row)]
            if any(cells):
                rows.append(cells)
        if len(rows) < 2:  # a table with only a header describes nothing
            continue
        out.append(
            {
                "under": text_of(headings[-1]) if headings else "",
                "columns": rows[0],
                "rows": rows[1:],
            }
        )
    return out


def main() -> int:
    day = sys.argv[1] if len(sys.argv) > 1 else datetime.date.today().isoformat()

    urls = pages()
    if not urls:
        print("PayTR's sitemap listed no API pages", file=sys.stderr)
        return 1
    with ThreadPoolExecutor(max_workers=8) as pool:
        bodies = list(pool.map(fetch, urls))

    recorded = []
    for url, body in zip(urls, bodies, strict=True):
        tables = tables_in(body)
        if not tables:
            continue
        path = url.removeprefix("https://dev.paytr.com/").strip("/")
        # The digest covers the tables, not the page: a changed footer or a new
        # link in the sidebar is not a changed API.
        digest = hashlib.sha256(
            yaml.safe_dump(tables, allow_unicode=True, sort_keys=False).encode()
        ).hexdigest()
        recorded.append(
            {
                "path": path,
                "url": url,
                "language": "en" if path.startswith("en/") else "tr",
                "digest": digest[:16],
                "tables": tables,
            }
        )

    document = {
        "kind": "documentation record, not an API description",
        "provider": "paytr",
        "source": SITEMAP,
        "fetched": day,
        "note": (
            "PayTR publishes no machine-readable description. These are the field "
            "tables from their documentation, kept so that a change to one is "
            "visible in a diff. The column saying whether a field enters the token "
            "hash is the one that matters most: the hash signs the request, and a "
            "field silently entering or leaving it is a signing failure that looks "
            "like bad credentials."
        ),
        "warning": (
            "The order of the rows is not the order of the hash. On the iFrame API "
            "the table lists currency fifth and PayTR's own sample code signs it "
            "ninth. The table says which fields are signed; only their sample code "
            "says in what order."
        ),
        "pages": recorded,
    }

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{day}.yaml").write_text(
        yaml.safe_dump(document, allow_unicode=True, sort_keys=False, width=100),
        encoding="utf-8",
    )
    latest = OUT / "latest.yaml"
    latest.unlink(missing_ok=True)
    latest.symlink_to(f"{day}.yaml")

    tables = sum(len(page["tables"]) for page in recorded)
    print(f"{len(recorded)} pages, {tables} tables -> specs/paytr/{day}.yaml")
    return 0


if __name__ == "__main__":
    sys.exit(main())
