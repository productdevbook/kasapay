---
name: money-safety
description: >
  The nine ways a payments library loses somebody money, each with the
  scenario that produces it and the direction to err in. Read before writing
  or reviewing anything in kasapay that touches an amount, a status, an
  idempotency key, a refund, a webhook, or a value that ends up in a log or a
  URL — and before deciding that an unknown value from a provider is safe to
  map onto something. Carries the rule that decides every one of these
  arguments, and the five defects this workspace actually shipped.
---

# Money safety

A bug in a payments library is not a bug in a library. It is a shop that
shipped for nothing, a payer charged twice, or a refund given away a second
time. This file is the list of ways that has happened or nearly happened here.

## The rule that settles every argument below

**Where being wrong could take money twice, or give it away twice, take the
side that fails loudly or does nothing at all.**

A refusal costs a caller an error to read. A wrong guess costs somebody money
and nobody finds out until a reconciliation. They are not comparable, and the
asymmetry is the whole reason this file exists.

Its corollary is the one that gets forgotten: **a field accepted and dropped is
worse than a field refused.** Accepting it says the guarantee was given.

## The nine

### 1. Charged twice, because a retry was safe and was not

A caller sets an idempotency key, the call times out, `Error::is_retryable`
answers true, and the caller sends it again. If the key reached the provider,
the retry is free. If the adapter dropped it, the retry is a second payment.

**This shipped.** `Provider::charge` dropped `ChargeRequest::idempotency_key`
at iyzico's classic API and at PayTR while three sibling adapters sent it and a
fourth refused it — so the same crate implemented both readings. Fixed in #168,
and the fix is a conformance check rather than three edits, because the class
had already been found once before on `capture`.

**What to check:** every method taking a key either puts it on the wire or
answers `Unsupported`. Never a third thing. `conformance.rs` asserts this; if
you add a method that takes a key, add it there in the same change.

### 2. Taken, and reported as not taken

The shop does not ship, or refunds money it never received, or a reconciliation
job decides the day is short.

**This shipped.** `Provider::lookup` answered `Status::Captured` with
`0.00 TRY` for a real payment, because the amount parser returned `None` for
three different reasons — absent, unknown currency, and *too many decimal
places* — and the caller of it treated all three as absent. iyzico writes
decimals as `20.00000000` elsewhere in the same API. Fixed in #172.

**What to check:** every `Option<Money>` that becomes a concrete amount. Ask
what the `None` meant. If one of the answers is "the provider sent something we
could not read", that is `Malformed`, not zero.

### 3. Not taken, and reported as taken

The worst one, because the shop ships and there is no money.

The shape is always an unknown value defaulting closed:

```rust
match value {
    Some(0)  => Status::Pending,
    Some(-1) => Status::Failed,
    _        => Status::Captured,   // ← every future value the provider adds
}
```

**This shipped.** iyzico's `fraud_status` was that block, arm for arm. Their
schemas give `fraudStatus` `enum: [0, -1, 1]` in six places and their prose
says to ship only on 1, and the wildcard sent every fourth value to `Captured`.
It was reachable through `Provider::charge` on the stored-card path — what a
subscription bills on. Fixed in #200. Of nine status fallbacks across five
adapters it was the only one landing on a settled state — seven land on
`Pending` and PayTR's refuses an undocumented status outright, which is the
best answer of the three. Being the only one is what made it invisible.

**What to check:** every status mapping's fallback arm. An unknown value is an
open state — `Pending`, or `Other` — never a settled one. If a provider's
documentation genuinely says the remaining values all mean success, that is a
reading, and it belongs in `UNVERIFIED.md` with the call that settles it.

### 4. Off by a factor of a hundred

`Money` counts minor units, so the exponent decides what the integer means. A
currency where the provider and ISO 4217 disagree turns 1,200 into 120,000.

Stripe reads the Icelandic króna as having no minor unit where ISO gives it
two, and wants three-decimal amounts as a multiple of ten. That is why
`Currency` names only currencies whose minor unit is exactly two places, plus
the nine it shipped with, whose readings are settled.

**What to check:** before naming a new currency, read that provider's own
documentation for its minor unit. `money.rs`'s own tests fail if one is added
without that reading.

### 5. Refunded twice

The same shape as #1 and worse, because a refund has no payer to notice.
**This shipped, twice.** `RefundRequest::idempotency_key` documented the
opposite rule to `Provider::refund` — *ignore* rather than *refuse* — until
#168, so the guarantee a caller read was the opposite of the one the adapters
kept. And `PayTr::refunds` summed a refund whose amount it could not find as
nothing, so a fully refunded payment read as unrefunded to the caller the
method exists for, until #210.

**What to check:** an adapter that cannot honour a refund key refuses. And
before resending a refund whose answer never arrived, read the provider's own
list of refunds already taken.

**That remedy is only safe if the list read is complete.** A paginated read
that stops early answers "no prior refund" and licenses exactly the duplicate
this section exists to prevent — #144 fixed one that could not terminate, and a
short answer is the quieter half of the same bug. A list whose entries can be
read as zero does the same thing: PayTR's refund records carry four field names
PayTR documents nowhere, and an absent amount used to sum as nothing, so a
fully refunded payment read as unrefunded (#187).

### 6. A hold nobody releases

Authorised money sits against a payer's limit until the provider expires it,
which can be days. `Capabilities` must not claim a hold-then-decide flow the
adapter cannot finish.

### 7. Shipped against a payment nobody made

A webhook that does not verify is not information. Two rules, and the second is
counter-intuitive:

- an unsigned or wrongly-signed delivery never becomes an `Event`;
- **answer `200` anyway.** A provider retries anything else for days, and a
  handler returning 500 while it works out what to do is one that gets called
  again.

**Unless `verify` reaches the network**, in which case its `Err` carries two
different facts and only one of them is a 200. Mollie signs nothing, so
verifying is a read-back — and `Err` there means either *this delivery is not
worth acting on* or *the check did not finish*. Answering 200 to the second
acknowledges a delivery nobody read, and on a payment method where the payer
never returns that is money taken and never learned about.
`Error::is_retryable` is the discrimination, and `kasapay-mollie`'s crate
documentation is where it is spelled out.

A delivery carrying *two* signature headers is refused rather than resolved —
two claims about one delivery mean something in front of the verifier read it
differently.

### 8. The wrong currency, sent successfully

A currency `match` may carry a wildcard arm **only where that arm refuses.**
Mapping an unknown currency onto something is the thing that was never allowed.
`conformance.rs` walks every currency past every adapter to prove each is
settled or refused before a socket opens.

### 9. Written into a log that outlives the request

None of this is money leaving. All of it is the thing a shop cannot take back
once it has left: an IBAN, a national identity number, a masked card, a payer's
address, a whole provider response body.

Two shapes. The first is a derived `Debug` on a type that holds a provider's
answer or a payer's details. The second is a value in a **URL path** rather
than a body — which reaches every proxy and access log on the way, rather than
sitting somewhere somebody has to go looking for.

**This has shipped four times.** `Raw` derived `Debug`, so one
`tracing::debug!("{charge:?}")` printed an IBAN, a masked card number, an
address and an identity number (#109) — *"the leak was not that module's: it
was every module's."* `mass::Recipient` did the same with an IBAN and an
identity number (#111), and its own commit message says *"the same defect as
`Raw`'s, found the same way."* A card number could reach Mollie's mandate path,
because the Luhn guard existed twice privately and a third adapter had neither
(#177). And Mollie's webhook took an unauthenticated stranger's string straight
into `/v2/payments/{id}`, where `..` walked out of that path into any other GET
the merchant's key could reach (#183).

The rule was earned at #111 and not written down, so #177 found the class again
in a worse sink four days later. That is the whole argument for writing it down
now.

**What to check:** every type holding a provider's answer or a payer's details
has a hand-written `Debug` — a length, or the last four, never the value.
Every value that becomes a URL path segment rather than a body field, and
whether the value came from a caller or from a stranger. And a guard that
exists twice privately is one crate away from existing nowhere:
`kasapay_core::looks_like_a_card_number` is public for that reason.

## Two habits that catch these and reading does not

**Count, do not read.** Every one of the two that shipped looked fine in
review. They were found by asking mechanically: does every method taking a key
either send it or refuse it; what does this `None` mean. See the `ratchets`
skill.

**When the same class appears twice, write the rule down.** The idempotency
class was found on `capture`, fixed as an instance, and reappeared on `charge`
and `refund`. The second finding is the signal to build the check.

## What none of this covers

Every claim in this workspace about what a provider *does* is read from that
provider's documentation. Nothing here has been run against a live account.
`UNVERIFIED.md` is the register, and the `sandbox-verification` skill is how an
entry leaves it.
