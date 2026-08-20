"""Prints one release's section of CHANGELOG.md, for the GitHub release body.

The changelog is written by hand and says what a change costs a caller who
upgrades. That is what a release note should say too, so the release note is
the changelog's own section rather than a second telling of it that can drift.

    python3 scripts/release_notes.py 0.0.2

Exits non-zero if there is no section for that version, which is what should
stop a release rather than publishing one with an empty body.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"

# `## 0.0.2 — 2026-08-15`. The whole line is matched, so the date after the
# version does not fall into the body.
HEADING = re.compile(r"^## +(\S+).*$", re.MULTILINE)


def section(text: str, version: str) -> str | None:
    starts = [(m.group(1), m.start(), m.end()) for m in HEADING.finditer(text)]
    for index, (name, _, end) in enumerate(starts):
        if name != version:
            continue
        after = starts[index + 1][1] if index + 1 < len(starts) else len(text)
        return text[end:after].strip("\n")
    return None


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    version = sys.argv[1].removeprefix("v")
    body = section(CHANGELOG.read_text(encoding="utf-8"), version)
    if body is None:
        print(f"CHANGELOG.md has no section for {version}", file=sys.stderr)
        return 1
    # A heading with nothing under it is not a section to publish. `section`
    # answers "" for one, which is not None, so the check above let it through
    # and `gh release create` was handed an empty note.
    if not body.strip():
        print(f"CHANGELOG.md's section for {version} is empty", file=sys.stderr)
        return 1
    print(body)
    return 0


if __name__ == "__main__":
    sys.exit(main())
