# Working in kasapay

A Rust workspace: one payment API over any payment provider. Public, MIT.

## Commits

Conventional Commits. Every subject starts with a type and a colon:

    feat:      a capability the library did not have
    fix:       behaviour that was wrong
    refactor:  same behaviour, different shape
    perf:      same behaviour, faster
    docs:      README, rustdoc, specs/README.md
    test:      tests only
    build:     Cargo.toml, dependency versions, MSRV, features
    ci:        .github/workflows
    style:     cargo fmt and nothing else
    chore:     anything left over — lint config, scripts

A scope goes in parentheses when it narrows things usefully:
`fix(iyzico): …`, `build(deps): …`. Subject in the imperative, lowercase after
the colon, no full stop.

The body says why, not what — the diff already says what. Measurements, run
ids and PR numbers belong here rather than in code comments.

## Actions are pinned to commits

Every `uses:` in `.github/workflows` names a full commit SHA, with the tag it
came from in a trailing comment. A tag is a mutable pointer: whoever owns the
action can move it, and `release.yml` holds the crates.io publish token while
`spec-drift.yml` can write to the repository.

Two of them read their own ref name to decide what to install —
`dtolnay/rust-toolchain` and `taiki-e/install-action` — so pinning the ref
means passing `toolchain:` or `tool:` explicitly. Pinning without that
silently installs the wrong thing.

To update one: resolve the new tag to its commit and change both the SHA and
the comment.

## Dependencies

`deny.toml` says what the tree may contain: permissive licences only, no
wildcard versions, nothing from outside crates.io, and no known vulnerability.
CI checks it on every push, and `audit.yml` checks the advisories again daily —
an advisory lands against a tree that has not changed, so a clean run at merge
time does not stay clean.

## Changelog

`CHANGELOG.md` is kept by hand. A change that would make somebody's code stop
compiling, or make it do something different, goes in under **Unreleased**
before the PR merges — not at release time, when nobody remembers what it cost.

## More than one person, or agent, at a time

One git checkout, several hands, is a way to lose work. It has happened here:
four agents shared `/home/mkpc/github/kasapay`, and within minutes one had
checked out a branch on top of another's uncommitted changes, a `git add -A`
had swept a third's file into the wrong commit, and a fourth's commit had
landed on somebody else's branch. Nothing was lost, but only because it was
caught quickly and unpicked by hand.

So: **one worktree each.**

    git worktree add ../kasapay-<what-you-are-doing> -b <branch> origin/main

Work only in your own. Do not `git checkout` another branch in a tree somebody
else is using, and prefer `git add <paths>` over `git add -A` when you are not
alone — the wildcard cannot tell your changes from theirs.

Branches are cheap and merges are serial: open the pull request, let CI say
whether it is right, and let one person merge. Two agents merging to `main`
concurrently is the same problem one layer up.

### Rebuilding a branch reverts what landed while you were away

`git reset --mixed origin/main` moves the branch and leaves the working tree
alone — which is the point, and which is also how a whole paragraph of
somebody else's work disappears. The file in your tree is the one you edited
hours ago. Anything that landed in it meanwhile is simply not there, and
committing it takes it out. It has happened here: a branch rebuilt as one
commit would have silently reverted a section of `specs/README.md` that had
landed in between.

Nothing catches this. It is not a conflict, the tests pass, and CI has no
opinion about a paragraph that used to exist. So after rewriting a branch,
before pushing:

    git diff origin/main...HEAD | grep '^-' | grep -v '^---'

Every removed line should be one you meant to remove. If a line you have never
seen appears there, something landed while you were working and you are about
to take it back out.

## Nothing is built or tested on this machine

This machine serves live sites; a build taking every core has taken it off the
air before. So no `cargo build`, `cargo test`, `cargo clippy` or `cargo doc`
locally. `cargo fmt` is the exception — it compiles nothing.

The loop is: write, commit, push, read CI. `.github/workflows/ci.yml` runs
fmt, clippy with warnings denied, nextest, doctests, the MSRV check and the
feature matrix. If something is wrong it is wrong there, and the next commit
is the fix.

`Cargo.lock` is committed, every cargo call in CI is `--locked` bar the feature
matrix — `cargo hack --no-dev-deps` rewrites the manifests, which is what
`--locked` refuses — and the lockfile is written by CI rather than here.
`cargo generate-lockfile` resolves dependencies and compiles nothing, which is
cheap — but the rule is "cargo fmt only" rather than "only what is cheap", so
the **Lockfile** workflow is what runs it: by hand after a dependency changes,
monthly on its own. It pushes a branch and prints a link to open the pull
request from, because a workflow that opens one itself needs a repository
setting that also lets a workflow *approve* one — and this repository pins
every action to a commit and keeps the crates.io token in `release.yml`, so
that is the wrong trade for a convenience. The click is a person's; what it
buys is unchanged, because the resolution is not trusted until CI has run on
the pull request. The point of pinning it is that a red run on
a pull request that touched nothing related can be reproduced at all.

Cutting a release writes the lockfile too: **Cut a release** runs
`cargo update --workspace` after the bump, which rewrites this workspace's own
versions in `Cargo.lock` and leaves every third-party pin alone. Without it the
release commit would land on `main` with a lockfile that disagrees with the
manifests and turn every `--locked` job red.

So **changing a dependency is two pull requests**: the one that edits
`Cargo.toml`, which goes red on `--locked` because the lockfile no longer
matches, and the Lockfile workflow's, which resolves the tree again. Merge the
second and the first goes green. Anything else is a lockfile that says one
thing while CI checks another.

## A claim is the part nothing compiles

This workspace is a third prose by line: 10,939 of its 31,615 non-blank source
lines are doc comments, and that is before `CLAUDE.md`, the changelog, the
register and the skills. The compiler has an opinion about none of it.

It shows. In one audit of seventeen merges, twelve defects were found in the
work itself: **eight were prose**, three were code and one was process. The
three code defects were all caught within minutes — by the compiler, by an
existing test, by a new test. None of the eight was caught by anything but
somebody reading.

So four rules, in the order they pay.

**One claim, one home.** `Provider::cancel` said the opposite of what it did in
six places at once, because the same fact had been written six times. Every
copy is a thing that can rot on its own. A second place cites the first.
Which *word* to use is the same rule one level down, and its home is
[`CONTEXT.md`](CONTEXT.md): not what a type does, which the type says, but
which of several circulating words to reach for — a payer rather than a
shopper, a `Buyer` being the details where a `customer` is the provider's
handle, releasing a hold rather than voiding it.

**A claim that can be checked becomes a check.** Not a promise to remember it.
`conformance.rs` counts its own roster against `impl Provider for` in the
source; `documentation.rs` reads the doc comments; `compare_specs.py` names any
description nothing compared. Each of those replaced a sentence somebody had to
keep true by hand.

**A number in a sentence is a claim nothing compiles.** Every one that has gone
stale here went stale the same way — the ninth thing landed and the sentence
still said eight. Most of them carry nothing a word does not: "a change here
reaches **every** adapter" says what "five adapters" said and cannot go wrong.
Where the number is genuinely informative, it goes in `scripts/counts.py`,
which reads the source for the fact and the prose for the claim, and CI runs it.

That list is hand-kept and may only shrink, and the reason is measured: 112
(number + countable noun) pairs live in this repository's prose, and almost all
are rhetorical — "four ways of reaching green", "six currencies iyzico settles
in". A check over all of them would be ninety false positives, which is a check
people learn to route around.

**A claim that cannot be checked goes in `UNVERIFIED.md`.** Not into a doc
comment where it reads as settled. That file is the register, and an entry
there is the honest shape for something read off a document rather than
observed.

**And measure a rule before you write it.** Twice in one day a fix that sounded
right was refused by its own measurement: widening the doc-seam detector to
skip a blank `///` matched **380** comments, because that is the *correct*
shape for one. A handful means land it. Fifty means the rule is wrong
somewhere, and knowing that before it becomes a test is the whole point.

## The standards this repository is written against

Four skills, and which one to open depends on what you are doing rather than
on taste:

- **`rust-guidelines`** — Microsoft's 89 pragmatic guidelines. Read *before*
  writing: adding a public type, shaping an error, deciding whether to split a
  crate or reach for a macro. This is a published library workspace, so the
  applicable set is `universal`, `correctness`, `performance`, `project`,
  `docs` and `ai`, plus all thirty-three under `libs`.
- **`apollo-rust-review`** — Apollo's standard for judging Rust that already
  exists, with a P0–P3 severity matrix. Read when reviewing a diff. Rank with
  it, and do not report style as if it were correctness.
- **`ratchets`** — how to find the bugs reading does not find, and how to stop
  each one coming back. Its first half is a way of auditing that counts rather
  than judges; its second is how to write a check that reads the codebase's own
  source so a rule is enforced instead of remembered.
- **`agent-briefs`** — writing the role files and the task briefs for a
  multi-agent run. The roles in `.claude/agents/` are written to it.

Two more live in this repository rather than on one machine, because they are
about this library specifically and a contributor should get them with the
clone:

- **`.claude/skills/money-safety`** — the nine ways a payments library loses
  somebody money, each with the scenario. Read before writing or reviewing
  anything that touches an amount, a status, an idempotency key, a refund, a
  webhook, or a value that ends up in a log or a URL. Five of the nine name
  defects this workspace shipped, and the ninth names four of them.
- **`.claude/skills/sandbox-verification`** — how an `UNVERIFIED.md` entry is
  closed against a provider's sandbox without taking anybody's money, and —
  more importantly — which entries a sandbox cannot close at all. Its first
  step is checking the entry still describes the code, because a call site
  moves and a stale entry closes on the wrong function.

`.claude/agents/` holds the roles. Two are worth knowing about before you
need them: `kasapay-verify` is the only one that ever touches a credential, and
its role file leads with what it must never do; `kasapay-review` is the
read-only one, registered without the tools to edit, because an auditor that
can fix what it finds does, and a fixed finding stops being a finding.

Two of them are worth knowing without opening anything, because they are what
an agent writing Rust tends to get wrong:

**M-TAUTOLOGICAL-TESTS.** A test that restates the constant it is testing
passes by construction and adds noise.

**M-NO-META-DESIGN-DOCUMENTATION.** Documentation records the end state, not
the design journey — no "why we picked X over Y" essays, no self-graded tables
of which rules were followed. The README was 367 lines of exactly that before
it was cut to 225. Enduring architectural goals are the exception and belong in
a README's own *Design principles* section.

## Where the boundary is

`kasapay-core` holds no HTTP client and never will. A provider adapter brings
its own. Anything that is true of one provider and not another belongs in that
provider's crate, not in core.

`Currency` is deliberately exhaustive: adding one is a breaking change. Do not
add `#[non_exhaustive]` to it.

The rule about wildcard arms changed in #158, and the reason is worth carrying:
what was never allowed is **mapping** an unknown currency onto something. A
wildcard that **refuses** was always the safe answer, and once the enum grew
past a hundred variants it became the only workable one. So a currency match
may carry `_ =>` where that arm returns an error, and may not where it returns
a value. `crates/kasapay/tests/conformance.rs` walks every `Currency::KNOWN`
past every adapter and asserts each one is either settled or refused before a
socket opens; that test is what replaced the compiler as the thing holding the
guarantee up, and it is not optional.

What decides whether a currency is named at all: ISO 4217 currently defines
it, its minor unit is **exactly two decimal places**, and some provider here
settles in it — plus the nine the library shipped with, whatever their
exponent. The two-decimal rule is a safety rule, not tidiness. Zero- and
three-decimal currencies are where a provider's reading and ISO's diverge
(Stripe treats the Icelandic króna as having no minor unit, and wants its
three-decimal amounts as a multiple of ten), and being wrong about one is a
payment out by a factor of a hundred. Adding such a currency means reading that
provider's documentation first; `money.rs`'s own tests fail until somebody has.

`Charge` is open — every field public, no `#[non_exhaustive]` — because an
adapter in someone else's repository has to be able to build one.

## Specs

`specs/` records what each provider said its API was, dated. Nothing is
generated from it. Refetch with `python3 scripts/merge_iyzico.py` and
`python3 scripts/fetch_stripe.py`; the weekly job does the same and opens a PR.

One of the four keeps no copy of the description. Mollie licenses theirs
CC-BY-NC-SA and this repository is MIT, so `specs/mollie/` is a dated meta and
two hashes; `scripts/fetch_mollie.py --write-document` writes the document
itself to a gitignored path when somebody needs to read it. Do not commit one.

iyzico's side is a sweep of their entire documentation site in both languages —
96 operations across 11 groups, one file each, grouped by API path rather than
by documentation URL. Before adding an endpoint, read that group's
`latest.yaml` and the `notes` for it in the dated index rather than the
documentation page: neither language documents everything, and where they
overlap they sometimes disagree.

How many of the 96 are implemented is not written here, because a number in
prose is wrong the moment a module lands. `python3 scripts/coverage.py` counts
it from the endpoint strings the code actually calls, and answers two lists
that matter more than the total: an operation nothing calls and nothing
explains, and an explanation that no longer describes anything. Both are work.
What it does not reach today, it prints with a reason for each, and CI fails
when one has none — so the breakdown belongs there rather than here, where it
would rot the way the sentence this replaced did.

The specs carry only what iyzico states. Most operations document no
authentication at all, and that absence is recorded rather than filled in — if
an adapter needs to know how a request is signed, that comes from iyzico, not
from `specs/`.

## How a release happens

One trigger, and it is a person's decision rather than an agent's: run the
**Cut a release** workflow with a version, `dry_run` off. It bumps the version
everywhere it is written, rewrites the lockfile's own entries, dates the
changelog's `Unreleased` section, commits to `main`, pushes the tag — and then
asks for **Release** at that tag, which packages every crate, publishes to
crates.io in dependency order, and writes the release note from the changelog's
own section for that version.

**It asks rather than relying on the tag**, and that is not decoration: a tag
pushed with a workflow's own `GITHUB_TOKEN` starts no other workflow, which is
GitHub's guard against a workflow triggering itself for ever. `Release` still
listens for a tag push — a tag pushed by a person does start it — but the one
this workflow pushes never would. 0.0.3 was published by hand because that step
did not exist yet.

So the release note is never written twice. What a release says is what the
changelog already said, which is why the changelog is kept in terms of what a
change costs a caller who upgrades rather than what was done.

Two things it will refuse rather than guess: a version that already has a tag,
and a changelog with no section to publish. Run it with `dry_run` on first —
it shows the diff and checks both without committing anything.

`cargo publish` never runs on a developer's machine. It runs from the tag, in
CI, with a token that lives only as a repository secret.

## Unverified against a live API

The whole list is [`UNVERIFIED.md`](UNVERIFIED.md), by the account each entry
needs: what the library does today, why the reading might be wrong, and the
call that would settle it. What follows is the short version for the two an
adapter's own code most depends on.

iyzico's `/payment/query` status mapping. The documented response body has no
status field, so `receipt.approved` and `isRefundable` are what
`query_into_charge` reads. Check it first against a sandbox account.

iyzico's pre-authorisation, in two places. `Provider::capture` sends
`/payment/postauth` with a `paidPrice` below the authorised amount when it is
asked to, on that field being documented as "the final amount to be collected
from the card" rather than as the authorised one — iyzico does not say in as
many words that a smaller figure is accepted, and `Capabilities::partial_capture`
promises it. And a payment that has been authorised and not captured reads back
as `Status::Captured` from `/payment/detail`, because iyzico answers
`paymentStatus: SUCCESS` for both and documents no values for the `phase` field
that would separate them. One sandbox pre-authorisation settles both.

iyzico's `/payment/auth` status mapping, for the same reason: the documented
response has no `paymentStatus`, so `fraudStatus` is what
`read_payment_answer` reads — 1 and an absent field are `Captured`, -1 is
`Failed`, and 0 and anything iyzico has not named are `Pending`, since their
schemas give the field `enum: [0, -1, 1]` and their prose says to ship only on
1. A pre-authorisation's `Captured` is `Authorized` instead. What has not been
seen is whether a fraud rejection arrives as `status: "failure"` or as a
success carrying -1.

Two things about Mollie, both read off their prose rather than an example.
`Mollie::cancel_payment` is `DELETE /v2/payments/{id}`, and the crate documents
an authorised payment as their 422 there — their words are that releasing a
hold is what `release-authorization` is for, not that the delete refuses one.
`Provider::cancel` is the other call, `release-authorization` itself. And yen
goes on the wire as `1200` with no decimal point, because their multicurrency
table gives JPY zero decimal places; no example anywhere shows a payment in a
currency that has none.
