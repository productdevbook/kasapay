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

## Unverified against a live API

iyzico's `/payment/query` status mapping. The documented response body has no
status field, so `receipt.approved` and `isRefundable` are what
`query_into_charge` reads. Check it first against a sandbox account.
