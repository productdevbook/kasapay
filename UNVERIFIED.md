# What this library trusts a document for

Every provider behaviour kasapay implements comes from what that provider
documents. Most of it can be checked without an account — a signature against a
worked example, a request body against a field table — and where it can, it is,
in a test.

This file is the rest: **the places where a document is all there is**, and one
call against a live account would replace a reading with an observation.

It is a file rather than a set of issues because that is what it is: a standing
property of a library nobody has run against a live account, not a backlog of
work somebody here can do. Every entry says what the library does today, why it
might be wrong, and exactly what to send and paste back. An issue is the right
place for each one *after* somebody has the account and the observation
disagrees.

Each entry names the issue it came from, so the argument that produced it is
still readable.

**None of these is a guess where a safe answer was available.** Where being
wrong could take money twice or refund it twice, the implementation chose the
side that fails loudly or does nothing, and says so in its own documentation.
That is why this is a list of readings to confirm rather than a list of bugs.

---

## A. An iyzico sandbox account

Free and self-service, and it settles most of this file.

### A1. `/payment/query`'s status mapping — from #2

`in_store::query_into_charge` decides between `Status::Captured` and
`Status::Pending` from `transactionDetail.receipt.approved` and
`transactionDetail.isRefundable`. **Neither is a status field.** The documented
response body has no status of its own, so the mapping is a reading no test can
falsify: the wiremock test asserts the mapping we wrote, not the one iyzico
implements.

`Status::Failed` and `Status::Canceled` are unreachable from a query, which is
almost certainly wrong.

**To settle it**, query one payment in each state and paste the four bodies:

- never completed
- completed and captured
- refunded in full, and refunded in part
- cancelled the same day

The documented response also carries `statusCode`, `transactionCode` and
`isVoidable`, and iyzico names no values for any of them. If those four bodies
show what the values are, the mapping can read a status instead of inferring
one.

### A2. The In-Store refund's field name — from #60

iyzico documents the partial-refund amount as `refundAmount` in prose and
`refundPrice` in the OpenAPI fragment **on the same page**. The field is
optional and an absent amount means a full refund, so **sending only the wrong
name is not an error — it is a full refund where a part was asked for.**

`in_store::Client::refund` sends both names, which is safe under either answer.

**To settle it**, refund half of a payment sending only `refundAmount`, then
repeat sending only `refundPrice`. One of the two will refund everything. Then
refund an already-refunded payment and record what comes back:
`Capabilities::repeated_refund` is `false` for In-Store because iyzico
documents nothing about doing it twice, and that may be understating what the
API allows.

In-Store needs the CepPOS app, so this one may not be reachable from a plain
sandbox. If it is not, the guard stays and costs nothing.

### A3. Is replaying a charge safe — from #54

iyzico refuses an idempotency key and says nothing about a reused
`conversationId`.

**To settle it**, send the same `/payment/auth` twice with an identical
`conversationId` and `basketId`. Two payments or one?

`ErrorKind::is_retryable` returns true for a timeout on the assumption that
nobody has checked. What makes that survivable today is `Provider::lookup`:
iyzico's classic API answers by `conversationId`, so a caller can ask instead of
guessing. An answer here would turn "ask first" from a necessity into a choice.

### A4. `fraudStatus` on a rejection — from #102

`classic::Client::pay_with_saved_card` maps `0` to `Status::Pending` (under
review) and `-1` to `Status::Failed`. What has never been seen is whether a
fraud **rejection** arrives as `status: "failure"` — an error before any of this
is read — or as a success carrying `-1`.

**To settle it**, trip their fraud rules with any payment and paste the whole
envelope.

### A5. The `/payment/detail` signature — from #102

Its six signed fields come from iyzico's own documentation and the algorithm is
pinned against their worked example, but no live response has ever been
verified. One real body confirms it or finds the gap.

### A6. A capture in a currency the response forbids — from #88

`specs/iyzico/payment/latest.yaml` documents six currencies on the
authorisation — TRY, USD, EUR, GBP, CHF, NOK — and **three** on
`PostAuthResponse`. One of the two is wrong and iyzico does not say which.

`Provider::capture` reads the currency iyzico sends and does not check it
against the response schema, so this costs nothing today. It still means one of
their two documents is wrong.

**To settle it**, authorise in GBP, capture, and paste the response.

### A7. Does a capture take less than was authorised — from #126

`Capabilities::partial_capture` is `true` for the classic API on the strength of
`paidPrice` being documented as *"the final amount to be collected from the
card"* rather than as the authorised one. **iyzico nowhere writes that a smaller
figure is accepted.**

**To settle it**, authorise 100.00, capture 40.00, and paste the answer. If it
refuses, the capability is wrong rather than the code.

### A8. What a held payment reads back as — from #126

`/payment/detail` answers `paymentStatus: SUCCESS` for a payment taken **and**
for one only authorised. The field that would separate them is `phase`, which
iyzico documents as "the transaction phase" and for which they name no values
anywhere. So `Provider::charge_status` reports `Captured` for money that is only
held, and says so rather than guessing a word iyzico has not written down.
`classic::Client::checkout_result_preauth` works around it by having the caller
say which form they opened.

**To settle it**, authorise a payment, read it back without capturing it, and
paste the body. If `phase` comes back as something like `PRE_AUTH`, the mapping
can use it and the workaround can go.

### A9. What reporting answers for a conversation id it has never seen — from #125

`Provider::lookup` reads `GET /v2/reporting/payment/details` by
`paymentConversationId` and treats an **empty `payments` array** as "no record —
the charge never landed, and sending it again is safe". That is iyzico's
documented shape rather than something observed.

If they answer a refusal instead, `lookup` answers `Err` where it should answer
`Ok(None)`: a caller would poll instead of retrying, which is the safe direction
to be wrong in, and still wrong.

**To settle it**, query a `conversationId` you have never used and paste
whatever comes back.

---

## B. A PayTR merchant account

### B1. The instalment rates' shape — from #73

`PayTr::instalment_rates` calls `/odeme/taksit-oranlari` and types the four
fields PayTR documents. `oranlar` — the rates themselves — stays on
`InstalmentRates::raw`, because PayTR describes it as *"the rates of the
instalment counts defined for your store, by card type … returned in array
format"* and never says what one entry contains. Checked and silent: both
languages' field tables, the PDF in their sample zip, the PHP, Python, .NET and
Node samples (each of which prints the result and stops), their Postman
collection, all seven repositories under `github.com/paytr`, the OpenCart,
PrestaShop and WooCommerce modules, and wayback snapshots to 2023.

**To settle it**, call it once and paste the raw body. One real response gives
the typed shape.

### B2. Is replaying a charge safe — from #54

**To settle it**, call `/odeme/api/get-token` twice with the same
`merchant_oid`. Is the second refused, a second payment, or the first one's
answer?

As with iyzico, `Provider::lookup` makes this survivable — PayTR's status query
is keyed by `merchant_oid`, so a caller can ask. An answer would still be worth
having: it decides whether `Ok(None)` is the only safe licence to retry or
merely the simplest one.

---

## C. A Terminal API merchant agreement and a Pavo device — from #96

Not a sandbox and not quick, which is why all three were decided from the
documentation. Two of them fail loudly if the reading is wrong, which is why
only the third has a switch.

### C1. `salesType` or `saleType` — loud

The OpenAPI fragment says `salesType` in both languages; the worked sample on
the overview page sends `saleType`. `terminal` sends the fragment's spelling.
The field is required, so being wrong is a refusal on the first call rather
than a silent wrong outcome — which is also why both are not sent: an
unexpected field is the kind of thing a strict server rejects, and sending both
would risk every call to save one loud failure.

### C2. `request_timestamp`'s unit — quiet, and switchable

iyzico asks for "the Unix timestamp value of the relevant request" and names no
unit; their own classic API writes `systemTime` in milliseconds.
`Login::authorize` sends seconds.

This is the one whose failure is **not** loud: a rejected timestamp comes back
as a login that did not work, which reads like bad credentials, on the first
call a merchant makes. `terminal::Config::timestamps(Timestamps::Milliseconds)`
is the switch, so finding out costs a line rather than a fork.

### C3. May a query omit `deviceUniqueId` — loud

The schema marks `paymentId`, `deviceUniqueId` and `transactionReferenceId` all
required; the warning printed beside it says they are "not all mandatory at the
same time" and lists three working combinations, and error `380111` exists for
getting it wrong. `terminal::Query` is an enum of exactly those three, so a
fourth cannot be built.

Two more from the same page, lower stakes: `locale`'s enum is `tr`/`en` while
the sample sends `"TR"` and the end-of-day sample sends `"TR-EN"`, which is not
a value anywhere; and a void answers 422 where a refund answers 400 for the
same documented failure shape.

---

## D. Read off prose rather than an example

These need no account — they need a maintainer to decide the reading is wrong.

### D1. Mollie's cancel on an authorised payment

`Provider::cancel` is `DELETE /v2/payments/{id}`, and the crate documents an
authorised payment as Mollie's 422 there. Their words are that releasing a hold
is what `release-authorization` is for, not that the delete refuses one.

### D2. Yen on the wire

Mollie's multicurrency table gives JPY zero decimal places, so `1200` goes out
with no decimal point. No example anywhere shows a payment in a currency that
has none.

---

## What to do with an answer

Paste the body into a new issue that names the entry — "A3: iyzico opens a
second payment for a reused conversationId" — and change the code and this file
together. An entry that has been observed leaves this file: it belongs in a
test and a comment by then, not in a list of things nobody has checked.
