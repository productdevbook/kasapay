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

### A10. Does `/payment/preauth` accept a tokenised card — from #198

`Client::preauth_with_saved_card` sends `/payment/preauth` the same body
`/payment/auth` takes, `{cardUserKey, cardToken}` inside `paymentCard`. The
specs disagree with that: `/payment/auth` is documented with
`PaymentCardSaved`, which defines both fields, and `/payment/preauth`'s inline
body uses `PaymentCard`, which **requires** `cardHolderName`, `cardNumber`,
`expireYear`, `expireMonth` and `cvc` and defines neither token field.

This is not spec silence — it is a different schema. And every hold this crate
can take goes through here: `Capabilities::separate_capture`,
`partial_capture`, and entries A7 and A8 all rest on it succeeding.

**To settle it**, one pre-authorisation against a stored card. Paste the
request and the answer.

### A11. `paidPrice` on a hold nobody has captured — from #198

`Provider::capture` with no amount reads the payment back and captures what
`/payment/detail` reports. iyzico's word for that field is **"Total collected
amount"**, not the authorised one — and for a hold nobody has captured, "total
collected" plausibly reads zero.

**To settle it**, authorise without capturing, read the payment back, and paste
`paidPrice`. This settles alongside A8 in the same pre-authorisation.

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

### B3. The token hash PayTR documents as "see the sample code" — from #148

Eleven operations are blocked on one formula: the card vault's five calls,
the Link API's four, and the Havale/EFT iframe's two are each documented to the
field and then say of `paytr_token` only *"örnek kodları inceleyin"*. A
signature guessed from a field table passes a mock and fails every real
merchant, so none of them is implemented — `kasapay-paytr`'s own documentation
says so where a reader meets it.

**To settle it**, send one signed request from a merchant account for any of
them — the Link API's create is the smallest — and paste the fields and the
token it carried. One formula unblocks all eleven, and PayTR's own support can
answer it without an account being used at all.

### B4. What the status query calls the fields inside `returns` — from #187

`PayTr::refunds` reads four names off each entry — `return_amount`,
`return_date`, `date_completed` and `return_ref_num` — and PayTR documents none
of them. Their status-query tables give the array a single row, `returns(Array)`
/ *"Eğer ilgili sipariş içerisinde iade varsa dönecek değer"*, and break out no
fields in either language. `return_amount` appears in their documentation only
on the refund endpoint's own tables, which is a different call.

So all four come from their sample responses. `wire::ReturnItem` says so where
a reader meets it.

A wrong name now fails loudly: an entry with no `return_amount` is
`ErrorKind::Malformed` rather than a refund of zero. That matters because the
method exists to be summed — a fully refunded payment that sums to zero reads
as unrefunded, and the caller refunds it again. The three date and reference
fields fail quietly, as `None`, which is the right side for a field nothing is
decided on.

**To settle it**, read back one order that has a refund against it with
`/odeme/durum-sorgu` and paste the whole `returns` array into `specs/paytr/`.
One response settles all four.

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

### D1. Mollie's `DELETE` on an authorised payment — from #192

`Mollie::cancel_payment` is `DELETE /v2/payments/{id}`, and the crate documents
an authorised payment as Mollie's 422 there. Their words are that releasing a
hold is what `release-authorization` is for, not that the delete refuses one.

**Check the call before spending anything on this.** The entry used to name
`Provider::cancel`, which has been `POST .../release-authorization` since #176 —
so verifying what it said would have settled the wrong function and left the
one that actually releases a payer's hold unobserved.

### D2. Yen on the wire — from #134

Mollie's multicurrency table gives JPY zero decimal places, so `1200` goes out
with no decimal point. No example anywhere shows a payment in a currency that
has none.

### D3. A recurring payment is refused here without a `redirectUrl` — from #161

`Mollie::charge_with_mandate` — and so `Provider::charge` with
`ChargeRequest::instrument` set — refuses a request with no
`ChargeRequest::return_url`, because `create` requires one on every payment it
sends. A recurring charge has nobody to redirect: no checkout opens and the
answer carries no `NextAction`.

If Mollie's own rule is that `redirectUrl` is required except on a recurring
payment, this crate is stricter than Mollie and a caller billing a subscription
has to invent a URL that is never used. It was left strict rather than loosened
on a recollection: sending a field Mollie ignores costs nothing, and dropping a
field it turns out to want fails every renewal.

What settles it: create a payment with `sequenceType: recurring`, a `mandateId`
and no `redirectUrl`, against a sandbox key. A `201` means the requirement here
can go; a `422` naming `redirectUrl` means it stays and this entry becomes a
comment.

### D4. What Mollie does with the answer to a webhook — from #196

`kasapay-mollie`'s crate documentation tells a handler to answer 200 for a
delivery it read and a non-2xx for one where the read-back did not finish, so
Mollie redelivers. Both halves rest on one unsourced sentence: that Mollie
retries a webhook that is not acknowledged, and therefore that a 200 stops it.
Nothing in `specs/mollie/` says so — by licence it is a dated meta and two
hashes — and Mollie's own retry schedule is not recorded anywhere here.

If a 200 does **not** stop redelivery, the guidance costs nothing. If a non-2xx
does not start one, a transient failure is still a payment the shop never hears
about, and the answer has to be a stored delivery replayed by hand instead.

**What settles it:** one sandbox payment whose webhook endpoint answers 500
once and 200 once, recording which of the two is delivered again and how long
Mollie waits.

### D5. What a Mollie webhook may post as an `id` — from #183

`Webhook::verify` refuses a posted `id` carrying anything outside
`[A-Za-z0-9_-]`, because that string becomes a path segment on a request
carrying the merchant's key and a `/` in it leaves `/v2/payments/`.

The character class is a reading. Mollie's common-data-types page says their
identifiers are a prefix and are at most 32 characters, and every prefix in
evidence — `tr_`, `ord_`, `re_`, `cst_`, `sub_`, `chb_` — is followed by
alphanumerics. What they do not publish is the character set. If the reading is
too narrow the cost is a legitimate delivery refused before a socket opens,
which is the opposite failure to the one the guard was written for.

**To settle it**, one sandbox delivery of each resource type Mollie posts to a
webhook address, with the `id` recorded verbatim.

---

## E. A Stripe test key

### E1. Whether a hosted-form create may carry a `return_url` — from #194

`Provider::charge` with no `instrument` creates an unconfirmed PaymentIntent
and sends `ChargeRequest::return_url` with it. Stripe's own document gives that
property as one that "can only be used with `confirm=true`", which this create
does not send.

What the document never says is what happens when it is sent anyway. The same
sentence is Stripe's boilerplate for confirm-gated parameters and appears four
times in `specs/stripe/latest.yaml` — on `error_on_requires_action`, inside
`mandate_data`, on `off_session` and here — never with a stated consequence.
The document does say "is ignored" elsewhere when it means it, which is
suggestive and is not evidence.

If Stripe **rejects** it, every Stripe charge written the way `README.md`
teaches returns `invalid_request_error` and no payment is created. If Stripe
**ignores** it, the field is accepted and dropped, which costs nothing but
should be said out loud.

The confirmed path is settled and needs no call: `Stripe::charge_saved_card`
sends `confirm: true`, so `return_url` belongs there and is sent there now.

**To settle it**, `POST /v1/payment_intents` with `amount`, `currency` and
`return_url`, no `confirm`, against a test key. A `200` means Stripe ignores it
and this becomes a sentence in the crate docs; a `400` means the hosted-form
path must stop sending it, and the README example needs rewriting.

---

## F. A PayPal sandbox account

Every one of these is in `kasapay-paypal`'s own crate documentation, where
`kasapay-verify`'s work list cannot see it. This section is that list, in the
order `sandbox-verification` asks for: retry safety first.

### F1. A retried refund with no amount, against a partly refunded capture — from #198

PayPal documents each half and never the combination. Their `refund_request`
schema says an omitted `amount` refunds *captured amount minus previous
refunds*, **computed when the request is processed**. Their `PayPal-Request-Id`
prose says a repeated key answers the same cached response rather than
processing again.

So when a caller retries an omitted-amount refund after some other refund has
landed in between, one of two things happens and PayPal says which nowhere: the
retry replays the first request's own computed amount, or it is treated as a
fresh request and recomputes against a smaller remainder. `PayPal::refund`
passes `request_id` straight through either way and does not guess.

This is the most expensive entry in this file: the wrong reading is money out
twice, and it is reachable by an ordinary retry loop.

**To settle it**, capture, refund part of it, then send an omitted-amount
refund and repeat it with the same `PayPal-Request-Id`. Paste both answers.

### F2. The order in which a status is resolved — from #198

`resolve_status` reads the order, then its capture, then its authorization,
then falls back. That order is read off PayPal's prose rather than any
documented example of an order carrying more than one of them.

**To settle it**, read back an order that has been authorised and then
partially captured, and paste the whole body.

### F3. `403` reading as `ErrorKind::Auth` — from #198

PayPal's error document does not say what a 403 means for these endpoints, and
the adapter reads it as a credentials problem rather than a refusal of its own
kind. A caller retrying on `Auth` after rotating a secret would be retrying
something that is not going to change.

**To settle it**, ask for an operation the sandbox account is not entitled to
and paste the status and body.

### F4. The account-level default `return_url` and `cancel_url` — from #198

Leaving `ChargeRequest::return_url` unset sends no `experience_context`, on the
reading that PayPal then uses the account's own configured URLs. Nothing in
their document says what happens when neither is set.

**To settle it**, create an order with no `experience_context` and follow the
approval link.

### F5. Four calls nobody has made — from #198

`PayPal::authorize`, `PayPal::authorize_order`, `PayPal::capture_authorization`
and `PayPal::refund` use request and response shapes that are documented
examples, so this is weaker than the entries above — but none of the four has
been called against a sandbox account. `Capabilities::separate_capture` is
`true` on the strength of them.

**To settle it**, one authorise-then-void and one authorise-then-capture.

---

## What to do with an answer

Paste the body into a new issue that names the entry — "A3: iyzico opens a
second payment for a reused conversationId" — and change the code and this file
together. An entry that has been observed leaves this file: it belongs in a
test and a comment by then, not in a list of things nobody has checked.
