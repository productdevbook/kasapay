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

## Where the boundary is

`kasapay-core` holds no HTTP client and never will. A provider adapter brings
its own. Anything that is true of one provider and not another belongs in that
provider's crate, not in core.

`Currency` is deliberately exhaustive: adding one is a breaking change, so
every adapter is forced to say what it maps to. Do not add `#[non_exhaustive]`
to it, and do not add a wildcard arm to a currency match.

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
by documentation URL. `kasapay-iyzico` implements four of them. Before adding an
endpoint, read that group's `latest.yaml` and the `notes` for it in the dated
index rather than the documentation page: neither language documents everything,
and where they overlap they sometimes disagree.

The specs carry only what iyzico states. Most operations document no
authentication at all, and that absence is recorded rather than filled in — if
an adapter needs to know how a request is signed, that comes from iyzico, not
from `specs/`.

## How a release happens

One trigger, and it is a person's decision rather than an agent's: run the
**Cut a release** workflow with a version, `dry_run` off. It bumps the version
everywhere it is written, dates the changelog's `Unreleased` section, commits
to `main` and pushes the tag. The tag starts **Release**, which packages every
crate, publishes to crates.io in dependency order, and writes the release note
from the changelog's own section for that version.

So the release note is never written twice. What a release says is what the
changelog already said, which is why the changelog is kept in terms of what a
change costs a caller who upgrades rather than what was done.

Two things it will refuse rather than guess: a version that already has a tag,
and a changelog with no section to publish. Run it with `dry_run` on first —
it shows the diff and checks both without committing anything.

`cargo publish` never runs on a developer's machine. It runs from the tag, in
CI, with a token that lives only as a repository secret.

## Unverified against a live API

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
`into_saved_card_charge` reads — 0 is `Pending`, -1 is `Failed`, anything else
is `Captured`. What has not been seen is whether a fraud rejection arrives as
`status: "failure"` or as a success carrying -1.

Two things about Mollie, both read off their prose rather than an example.
`Provider::cancel` is `DELETE /v2/payments/{id}`, and the crate documents an
authorised payment as their 422 there — their words are that releasing a hold
is what `release-authorization` is for, not that the delete refuses one. And
yen goes on the wire as `1200` with no decimal point, because their
multicurrency table gives JPY zero decimal places; no example anywhere shows a
payment in a currency that has none.
