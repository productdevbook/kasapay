"""Writing a dated record only on the day it says something new.

Every fetcher here writes `specs/<provider>/<YYYY-MM-DD>.…`, and most days a
provider has not moved. Writing the record anyway means a weekly job that opens
a pull request every week, twelve files wide, whose whole content is a date —
and a reviewer who stops reading them, which is the one thing the drift job
exists to prevent.

Measured before this was written: `merge_iyzico.py`, `fetch_paytr.py`,
`fetch_stripe.py`, `fetch_paypal.py` and `fetch_mollie.py` were all re-run five
days after the records in the tree. Eleven iyzico areas, byte for byte
identical. PayTR's document, identical but for its own `fetched:` line. Stripe's
and PayPal's subsets, identical. **One** thing had moved: Mollie's upstream
hash — and not their `subset_sha256`, so not a path kasapay maps.

So: compare what was fetched against the newest record already there, ignoring
the date it carries, and write only when they differ. What the tree then
answers is *when a provider last changed*, which is the question a reader
actually has. When it was last **looked at** is the workflow's own run history,
where a check that found nothing belongs.
"""

from __future__ import annotations

import json
import pathlib
import re


def newest_dated(
    directory: pathlib.Path, suffix: str, prefix: str = ""
) -> pathlib.Path | None:
    """The most recent `<prefix><YYYY-MM-DD><suffix>` in `directory`.

    By name rather than by mtime: the name is the record's own claim about when
    it was fetched, and a file copied about keeps its claim and loses its mtime.

    `prefix` is for PayPal, which keeps two documents in one directory and tells
    them apart with `payments-` in front of the date.
    """
    if not directory.is_dir():
        return None
    pattern = re.compile(rf"{re.escape(prefix)}(\d{{4}}-\d{{2}}-\d{{2}}){re.escape(suffix)}")
    dated = [path for path in directory.iterdir() if pattern.fullmatch(path.name)]
    return max(dated, key=lambda path: path.name) if dated else None


def _without_dates(text: str, path: pathlib.Path) -> object:
    """What a record says, with the day it was fetched taken out.

    A record differing only in `fetched` is the same record. JSON is compared
    as data so key order cannot make two identical records look different;
    anything else is compared as text with its `fetched:` line dropped, which
    is what the YAML records carry.
    """
    if path.suffix == ".json":
        try:
            payload = json.loads(text)
        except json.JSONDecodeError:
            return text
        if isinstance(payload, dict):
            return {key: value for key, value in payload.items() if key != "fetched"}
        return payload
    return "\n".join(
        line for line in text.splitlines() if not line.startswith("fetched:")
    )


def write_if_moved(path: pathlib.Path, text: str, previous: pathlib.Path | None) -> bool:
    """Writes `text` to `path` unless `previous` already says the same thing.

    Answers whether it wrote. `previous` of `None` — nothing recorded yet — is
    always a write, and so is a `path` that already exists: re-running today's
    fetch overwrites today's record rather than leaving a stale one beside a
    date that claims otherwise.
    """
    if previous is not None and previous != path:
        already = previous.read_text(encoding="utf-8")
        if _without_dates(already, previous) == _without_dates(text, path):
            return False
    path.write_text(text, encoding="utf-8")
    return True


def point_latest_at(directory: pathlib.Path, name: str) -> None:
    """Points `latest.yaml` at one of the dated files beside it."""
    latest = directory / "latest.yaml"
    latest.unlink(missing_ok=True)
    latest.symlink_to(name)
