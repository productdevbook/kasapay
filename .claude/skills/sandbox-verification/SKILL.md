---
name: sandbox-verification
description: >
  How to close an UNVERIFIED.md entry against a provider's sandbox without
  taking anybody's money — what evidence counts, what a sandbox genuinely
  settles and what it only suggests, and the rule that an entry leaves the
  register when it is in a test rather than when somebody remembers seeing it.
  Read before touching live credentials of any kind, before deciding a reading
  is confirmed, and when planning what to verify first.
---

# Sandbox verification

`UNVERIFIED.md` holds every claim this workspace makes about a provider that
was read from a document rather than observed. Eighteen entries today, grouped
by the account that would settle each, and each carries the one call that does
it.

This is how one leaves the file.

## Before any credential is used

**Sandbox only, and prove it is a sandbox.** Every adapter takes a base URL —
that is what makes them mockable, and it is what makes this safe. Check the
host against the provider's own sandbox host before the first call, and say in
the report which host was used.

**Never a card number.** No type in this workspace can hold one and that does
not change for a verification run. Where a flow needs a card, it needs the
provider's hosted form and a human at a browser — which is a real limit on what
can be verified unattended, and belongs in the report rather than worked
around.

**Never go looking for keys.** Not in the environment, not in another
project's files, not in a shell history. Credentials arrive because somebody
handed them over for this. Anything else is a key used without its owner
deciding.

**If a live account is ever unavoidable** — and prefer that it is not — the
smallest amount the provider allows, refunded in the same session, and the
whole thing recorded. Say so loudly in the report; it is not a detail.

## What counts as evidence

An observation is the provider's **raw response body**, kept whole, with the
request that produced it and the date. Not a summary, not "it worked".

`Charge::raw` already keeps the provider's answer untouched — that is what it
is for. Record it.

A reading is settled when three things are true:

1. the response is recorded;
2. a test pins it — the recorded body becomes a fixture in the existing
   `wiremock` suite, so the reading is checked on every push from then on;
3. the code says what was observed, where a reader meets it, rather than what
   was assumed.

**An entry leaves `UNVERIFIED.md` when it is in a test and a comment**, not
when somebody remembers seeing it. That is the whole discipline: a memory
decays and a fixture does not.

## What a sandbox settles, and what it only suggests

Being honest about this is most of the value, because the tempting failure is
to close an entry a sandbox never actually answered.

**A sandbox settles**: the shape of a response, the spelling of a field, which
fields are present, whether a request is accepted at all, what an error looks
like, whether an identifier round-trips.

**A sandbox only suggests**: anything a risk engine decides. Fraud statuses, 3-D
Secure step-ups, declines, velocity rules — sandboxes fake these and production
does not. `UNVERIFIED.md`'s A4 (*"`fraudStatus` on a rejection"*) is exactly
this shape, and a sandbox that never rejects anything settles nothing about it.

**A sandbox cannot settle**: what a provider does with a *reused* identifier
under real load, and what settlement looks like days later.

Where a sandbox only suggests, the entry stays in the file with what was seen
added to it. Downgrading an entry from "unknown" to "seen once in sandbox" is
progress and should be recorded as that, not as closure.

## What to verify first

Order by what being wrong costs, not by what is easy:

1. **Anything about a retry being safe** — A3 and B2. This is the double-charge
   question and it is the most expensive thing in the register.
2. **Anything that decides an amount** — a currency the response forbids (A6), a
   capture for less than was authorised (A7).
3. **Anything that decides a status** — what a held payment reads back as (A8),
   the `/payment/query` mapping (A1). Reading a hold as a sale writes money into
   a ledger nobody has taken.
4. **Field names and shapes** — C1, C3, B1. Cheap to check, and they fail loudly
   rather than quietly, so they are last.

One iyzico sandbox account closes A1 through A9 — nine of the eighteen.

## The four ways to close an entry dishonestly

Named because each is tempting when a run is nearly finished:

- **deciding the documentation must be right** because the sandbox did not
  contradict it. Not contradicting is not confirming;
- **closing an entry the sandbox only suggested**, especially anything a risk
  engine decides;
- **recording a summary rather than the body**, so nobody can re-read it later;
- **changing the code to match what was seen without a test**, which leaves the
  next person exactly where this one started.

If a run cannot settle an entry, the entry stays. A register with eighteen
honest entries is worth more than one with four and a shrug.

## The report

- Which host, and how it was shown to be a sandbox.
- Per entry: settled, suggested, or untouched — and the recorded body for each.
- The tests added, and the entries removed from `UNVERIFIED.md` in the same
  change.
- What was noticed and not chased.
