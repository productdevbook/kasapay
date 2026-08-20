"""Checks the counts this project writes into its own prose.

A number in a sentence is a claim nothing compiles. Every one that has rotted
here rotted the same way: somebody added the ninth thing and the sentence still
said eight, and the sentence is what the next reader believes.

Most of them should not be numbers at all. "A change here reaches every
adapter" carries what "five adapters" carried and cannot go stale, so those
were rewritten rather than checked — a rule that cannot be broken beats a check
that catches it being broken.

What is left are the counts that tell a reader something a word cannot: how
many failure classes there are to hold in your head, and how much of a table is
actually asserted. Those are checked here, against the source rather than
against each other.

    python3 scripts/counts.py

Exits non-zero naming every citation that disagrees with what it counts. CI
runs it on every push.

## What this does not do

It does not find a count nobody listed below. That was measured before it was
written: 112 (number + countable noun) pairs live in this repository's prose,
and almost all of them are rhetorical — "four ways of reaching green", "six
currencies iyzico settles in", "one provider". A check over all of them would
be ninety false positives, which is a check people learn to route around.

So this list is hand-kept, and that is the weakness. It may only shrink: a
count that becomes a word, or a command, leaves it and does not come back.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def money_safety_classes() -> int:
    """The numbered failure classes in the money-safety skill."""
    return len(re.findall(r"^### \d+\. ", read(".claude/skills/money-safety/SKILL.md"), re.M))


def money_safety_shipped() -> int:
    """How many of those name a defect this workspace actually shipped."""
    text = read(".claude/skills/money-safety/SKILL.md")
    sections = re.split(r"^### \d+\. ", text, flags=re.M)[1:]
    return sum(1 for s in sections if re.search(r"\*\*This (has )?shipped", s))


def capability_flags() -> int:
    """The `bool` fields on `Capabilities` a caller can branch on."""
    text = read("crates/kasapay-core/src/provider.rs")
    body = text.split("pub struct Capabilities {", 1)[1].split("\n}", 1)[0]
    return len(re.findall(r"^\s+pub \w+: bool,", body, re.M))


# Each entry: what it counts, how, and every sentence that cites it. A citation
# is a pattern with one group, which must read back as the number.
CHECKED = [
    (
        "failure classes in money-safety",
        money_safety_classes,
        [
            ("CLAUDE.md", r"the (\w+) ways a payments library loses"),
            (".claude/skills/money-safety/SKILL.md", r"The (\w+) ways a payments library loses"),
            (".claude/skills/money-safety/SKILL.md", r"^## The (\w+)$"),
            (".claude/agents/kasapay-review.md", r"the (\w+) ways this kind of library loses"),
        ],
    ),
    (
        "classes that name a shipped defect",
        money_safety_shipped,
        [
            ("CLAUDE.md", r"(\w+) of the nine name"),
            (".claude/skills/money-safety/SKILL.md", r"the (\w+) defects this workspace actually"),
            (".claude/agents/kasapay-review.md", r"and (\w+) of the nine"),
        ],
    ),
    (
        "Capabilities flags",
        capability_flags,
        [("README.md", r"asserts six of the (\w+) flags")],
    ),
]

WORDS = {
    1: "one", 2: "two", 3: "three", 4: "four", 5: "five", 6: "six", 7: "seven",
    8: "eight", 9: "nine", 10: "ten", 11: "eleven", 12: "twelve",
}


def agrees(said: str, actual: int) -> bool:
    """Whether a sentence's word for a number is that number.

    Spelled or in digits, and case does not matter — a sentence may open with
    it. This is the comparison `main` gates on, and the one `--self-test`
    exercises: a self-test that recomputes the comparison instead of calling it
    proves the copy works and says nothing about the original.
    """
    return said.lower() in (WORDS.get(actual, str(actual)), str(actual))


def self_test() -> int:
    """Feeds `agrees` the shapes it exists to tell apart."""
    cases = [
        ("nine", 9, True),
        ("Nine", 9, True),
        ("9", 9, True),
        ("eight", 9, False),
        ("nineteen", 9, False),
        ("", 9, False),
    ]
    for said, actual, expected in cases:
        if agrees(said, actual) is not expected:
            print(f"self-test: agrees({said!r}, {actual}) is not {expected}", file=sys.stderr)
            return 1
    print("self-test: a sentence saying eight where there are nine is caught")
    return 0


def main() -> int:
    wrong = []
    for name, count, citations in CHECKED:
        actual = count()
        for path, pattern in citations:
            found = re.search(pattern, read(path), re.M)
            if found is None:
                wrong.append(f"{path}: nothing matches /{pattern}/ — the sentence citing {name} moved")
                continue
            said = found.group(1)
            if not agrees(said, actual):
                wrong.append(f"{path}: says {said!r} {name}, and there are {actual}")
        print(f"  {actual:3d}  {name}")
    if wrong:
        print("\ncounts that disagree with what they count:", file=sys.stderr)
        for line in wrong:
            print(f"  {line}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(main())
