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

iyzico's side is a sweep of their entire documentation site — 155 operations
across 16 product areas, one file per area. `kasapay-iyzico` implements one of
them. Before adding an endpoint, read that area's `latest.yaml` and the
`notes` for it in the dated index rather than the documentation page.

## Unverified against a live API

iyzico's `/payment/query` status mapping. The documented response body has no
status field, so `receipt.approved` and `isRefundable` are what
`query_into_charge` reads. Check it first against a sandbox account.
