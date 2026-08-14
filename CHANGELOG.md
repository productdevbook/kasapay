# Changelog

What changed, and what it costs a caller who upgrades. Kept by hand, in the
order releases happen, newest first.

## Unreleased

These changes break code written against 0.0.1. All are cheap to follow and
all are the kind 0.0.x exists to make.

### Breaking

- **`Currency` gains `Rub`, `Chf` and `Nok`.** It is exhaustive on purpose, so a
  `match` on it in code outside this workspace has to answer for three more
  arms. They are here because providers already document them: francs and
  kroner on iyzico's payment, card-storage and reporting APIs, all three on an
  iyzico Link, and roubles at PayTR. Which of them a given iyzico product will
  take is not one list — see `specs/README.md`.

- **`PaymentId::new` is gone; an identifier says where it came from.**
  `PaymentId::issued(x)` is one the provider issued, and
  `PaymentId::derived(x, &["field"])` one kasapay composed because the provider
  issues none — PayTR has no payment id at all and names a payment by the
  `merchant_oid` the merchant chose. `PaymentId::source` reads the answer back
  as an `IdSource`, so a caller writing an identifier into a unique index can
  tell the provider's guarantee from their own. Every `PaymentId::new(x)`
  becomes `PaymentId::issued(x)`; for PayTR it becomes
  `kasapay_paytr::payment_id(&order)`, and the same string as before goes on
  the wire either way.

- **An identifier says what it names as well as who issued it.** `PaymentId` is
  now `Id<kind::Payment>`, one kind of `kasapay_core::Id`, and iyzico's classic
  API names a hosted checkout form by another: `classic::FormToken`. Both are
  iyzico's own strings, so `PaymentId::source` could never separate them, and
  handing a form's token to `refund` compiled and failed only against a live
  account. Three things change for a caller. `classic::Client::checkout_result`
  takes a `&classic::FormToken` rather than a `&str`, so
  `checkout_result("cf-token-1")` becomes
  `checkout_result(&FormToken::issued("cf-token-1"))`.
  `classic::Client::start_checkout_form` leaves `Charge::id` `None`, because
  iyzico has issued no payment id yet and the token is not one — it is where it
  always was, in the redirect's `continuation`, and the charge that
  `checkout_result` answers is the first to carry a payment id.
  `Provider::charge_status` on a `classic::Client` still reads a payment back,
  now through `/payment/detail` rather than by passing a form token off as a
  payment id. `PaymentId::issued`, `derived`, `as_str` and
  `source` are unchanged; an adapter outside this workspace names a kind of its
  own by implementing `IdKind` on a unit struct and writing one type alias over
  `Id`.

- **`Charge::id` is an `Option<PaymentId>`.** A payment nothing has named yet —
  an iyzico checkout form the payer has not finished — was a `PaymentId` with
  an empty string in it, which reads as a handle to a payment nobody made.
  Logging it is `charge.id.as_ref()`, passing it on is one `ok_or`, and a
  provider that names a payment by nothing at all now has somewhere honest to
  say so.

- **`classic::Reversal::payment` is an `Option<PaymentId>`**, for the same
  reason: iyzico documents a `paymentId` on a refund, a transaction refund and
  a cancel alike, and an answer that carries none is `None` rather than an
  identifier with nothing in it.

- **`classic::Client::refund`, `refund_transaction` and `cancel` take a
  `reason`.** `Option<&classic::Reason>`, and `None` sends what they sent
  before — every existing call keeps its meaning by gaining one argument.

- **`Provider` gains `capture`, `cancel` and `capabilities`, none with a
  default.** A provider outside this workspace has to answer all three. No
  default on purpose: an adapter that cannot capture has to say so rather than
  inherit an answer that happens to be wrong for it, and a capability that says
  yes over a call that then fails is a bug in the adapter.

- **`Charge` carries `order_amount`.** `Charge::amount` now means what the
  payer is charged, and `order_amount` what the goods came to — they differ
  under an instalment surcharge, and two adapters were dropping one of the
  pair. `None` means the provider does not say, not that the two are equal.

- **`Charge::raw` is a `Raw`, not a `serde_json::Value`.** Its old type put
  serde_json in the public API of every provider adapter, including ones
  written outside this workspace, and left a provider that answers XML nowhere
  to put its body. `charge.raw["field"]` becomes
  `charge.raw.text_at("/field")`, or `charge.raw.json()` for the whole thing.
- **`kasapay_iyzico::{Iyzico, Config}` are `kasapay_iyzico::in_store::{Client, Config}`.**
  The crate speaks two iyzico APIs now and one flat namespace could not name
  both.

### Added

- **`paytr::Notice`, the payment notice as a type — and the only place PayTR
  reports a refusal.** PayTR's status query answers a payment that succeeded or
  an error, so a refused payment was reachable only as an `ErrorKind::NotFound`
  from `charge_status`, and a caller had to read a commercial outcome out of an
  error kind. `Notice::charge(&credentials, currency)` checks PayTR's hash and
  answers the `Charge` the notice reports: `Status::Captured` for a payment,
  `Status::Failed` carrying the amount attempted for one PayTR refused, and
  `ErrorKind::Untrusted` for a notice PayTR did not sign — which must still be
  answered `OK`, or PayTR retries it for days.

  It takes the currency rather than reading it off the notice, because PayTR's
  hash covers `merchant_oid`, `status` and `total_amount` and nothing else. The
  amounts on a notice are in minor units where a refund takes a decimal string;
  this is what keeps those two formats apart.

  Costs an upgrading caller nothing. Code that verified a notice by hand with
  `Credentials::verify_callback` still compiles and still works.

  `charge_status` continues to answer `ErrorKind::NotFound` for a payment PayTR
  refused: PayTR sends nothing that separates it from an order it has never
  heard of, and rather than invent a distinction, the type now says so — in
  `Status`'s per-provider table, in the crate documentation, and in the error's
  own message.

- **`kasapay_iyzico::mass`, all six iyzico Mass Payout operations.** Money
  going out rather than coming in: create a payout of many recipients,
  authorize it, cancel one that has not been authorized, read the merchant's
  payout balance, and read a payout or one of its lines back. Built over
  `classic::Client` like `iyzilink` and `subscription`, so
  `mass::Client::new(classic)` is the whole setup. iyzico gates the product
  behind their own approval; none of it answers anything else until they switch
  it on.

  **A `success` from `start` is not an acceptance of every line.** iyzico
  reports the ones it would not take in the same body, as
  `Started::invalid_items`, and never mentions them again. That list is the
  only warning a caller gets before `authorize` spends the money, and
  `authorize` is documented with nothing that undoes it.

  Who is paid is one value rather than three loose fields:
  `Recipient::{Phone, Iban, IdentityNumber, MemberId}`, where the IBAN variant
  carries the account holder's name because that is the one case iyzico makes
  it mandatory. Nothing computes an IBAN checksum or matches a name to an
  account — iyzico documents neither — so a well-formed identifier for the
  wrong person is paid rather than refused.

  **iyzico documents no response signature for any of the six**, in either
  language, so nothing here is verified — including the answers that report
  where money already gone has got to. The request signature covers the path
  without the query string, as everywhere else in this crate, and mass payout
  is the one part that carries `locale` in the query on a `POST`.

  A line priced in anything but `TRY`, `USD` or `EUR` cannot be built: that is
  the only currency list either documentation language gives, and it is on the
  Turkish pages only. Amounts go out as bare JSON numbers written from `Money`'s
  own digits, which is what iyzico's worked examples send; their schemas type
  every money field `decimal`, which is not a JSON type at all. Reading is
  permissive, and tolerant of the eight decimal places their example writes a
  commission with — those zeros carry no value and are dropped, while a
  non-zero digit past the currency's minor unit answers `None` rather than
  being rounded into an amount nobody sent.

  `mass`'s module documentation lists what iyzico's two documentation
  languages contradict each other about, and what neither of them says — what
  `totalAmount` on a line means, who bears a line that fails, what currency the
  balance is counted in, and whether `externalId` is really idempotent.

- **`kasapay_iyzico::subscription`, the subscription catalogue: ten of iyzico's
  twenty-four subscription operations.** Create, read, list, replace and delete
  a product, and the same five for the pricing plans that hang off it — the
  half a merchant sets up once, before anybody subscribes. Built over
  `classic::Client` like `iyzilink`, so `subscription::Client::new(classic)` is
  the whole setup.

  The other fourteen are not here and are not guessed at: starting a
  subscription over the API takes a card number on the request, the hosted-form
  way to start one needs a subscriber shape this crate does not have yet, and
  the rest — the subscriber calls, activate, cancel, upgrade, retry, search —
  all follow from a subscription existing. `subscription`'s module
  documentation lists them one by one with the reason for each.

  **iyzico documents no response signature for any of the twenty-four**, in
  either language, so nothing here is verified. The request signature covers
  the path without the query string, as in `iyzilink`; here iyzico's PHP SDK
  names this API in the code that decides it.

  A plan priced in anything but `TRY`, `USD` or `EUR` cannot be built. That is
  what both documentation languages say, and it is narrower than the rest of
  iyzico — narrower than an iyzico Link, which is documented in seven. Being a
  currency `Currency` names is not the same as being one iyzico will take a
  subscription in, and roubles, francs and kroner are now exactly that case.
  Reading stays permissive: a plan that comes back in one of them still reads
  as money, and one in a code `Currency` cannot name at all has `price: None`
  with the amount still in `raw`.

- **`kasapay_iyzico::iyzilink`, all seven iyzico Link operations.** Create a
  link or a one-off fast link, read one back, list them, replace one, turn one
  on or off, delete one. It is built over `classic::Client` — same host, same
  `IYZWSv2` signing, same connection pool — so `iyzilink::Client::new(classic)`
  is the whole setup.

  Two things a caller has to know. **iyzico documents no response signature for
  any of these**: their signature page lists no iyzilink endpoint and no
  iyzilink schema carries the field, so nothing here is verified and a link's
  details are only as trustworthy as the connection. And the request signature
  covers the path without the query string — undocumented, but what iyzico's
  own PHP and Python SDKs both do.

  A link priced in yen or dinars cannot be built: iyzico documents a link in
  seven currencies and those two are not among them. `Currency` names all seven
  as of the entry above, so nothing iyzico does document a link in is refused
  here any more. One read back in a code `Currency` cannot name has
  `price: None` and the amount still in `raw`.

- **`kasapay_paytr::payment_id`**, which builds what PayTR reads a payment back
  by out of the order reference it was opened with. It is the one call that
  knows PayTR names payments by `merchant_oid`, so a caller never writes that
  field name and never passes their own reference off as PayTR's.

- **`PayTr::bin_details`**, PayTR's BIN service: the bank, network, company-card
  flag, non-3-D permission and instalment programme behind the first 6 or 8
  digits of a card number. A BIN PayTR has no record of is `Ok(None)`, not an
  error. Its token hashes `bin_number + merchant_id` before the salt — the BIN
  first, unlike every other PayTR call. `/odeme/taksit-oranlari` is deliberately
  not wrapped alongside it: PayTR documents the request and its hash but never
  what one entry of the `oranlar` payload looks like.
- Every GitHub action is pinned to a commit rather than a tag. A tag can be
  moved, and one workflow holds the crates.io publish token.
- A dependency audit. `deny.toml` refuses a known vulnerability, a non-permissive
  licence, a wildcard version or a source outside crates.io, checked on every
  push and again daily — an advisory lands against a tree that has not changed.

- **`kasapay-stripe` says which `async-stripe` it is built against**, and why it
  is pinned exactly rather than ranged. `=1.0.0-rc.8` is still the newest
  candidate published and there is no 1.0.0; a candidate has changed generated
  types before, so the version is chosen here rather than by resolution. The
  cost to a caller is that a crate depending on `async-stripe` itself must be on
  the same candidate — two exact pins at different candidates do not resolve
  together — and `Stripe::client` exists so that reaching past what this crate
  models needs no second dependency at all.

- **`Stripe::capture`, whole or partial.** Stripe leaves `amount` at what was
  authorised and reports a partial capture in `amount_received`, so
  `Charge::amount` reads the latter where the two differ — the trait promises
  the amount captured.

- **`kasapay-paytr`, a third provider.** PayTR's hosted form, status query,
  refund and payment-notice verification. It has no payment id of its own — a
  payment is named by the merchant's order reference — so that reference must
  never be reused.

- Two examples under `crates/kasapay/examples/`, built by CI so they cannot
  drift from the API.
- **A reason on an iyzico refund and cancel.** `classic::ReasonCode` is the
  four iyzico documents — `Other`, `Fraud`, `BuyerRequest`, `DoublePayment` —
  and `classic::Reason` pairs one with the optional free text beside it.
  iyzico only accepts a description alongside a reason, so the description
  hangs off the reason rather than sitting in a second `Option` a caller could
  fill on its own. `Fraud` and `DoublePayment` are what a shop tells its
  acquirer, and they land in chargeback and reconciliation reporting.
- `classic::Reversal` is exported. It is what all three of those answer, and
  it was a public type in a private module that no caller could name.
- `classic::Client` implements `Provider` for reading: `charge_status` takes
  the checkout form's token. `charge` answers `Unsupported` and names
  `start_checkout_form`, because the form needs more than `ChargeRequest`
  carries.
- `Stripe::refunds`, the refunds taken off a payment. A refunded PaymentIntent
  still reads `succeeded`, so "how much has come back" is this list summed
  rather than a status — and every provider has to be able to answer it. It
  follows Stripe's cursor to the end, because the default page is ten and
  stopping there would undercount.
- `Stripe::refund` and `Stripe::cancel`, which had no counterpart to iyzico's.
  Refunding in a currency the payment was not in cannot be refused before
  sending — Stripe takes a bare integer — so it is caught against the answer,
  and the error says the money has already moved.
- `classic`, iyzico's other API — the hosted checkout form, refunds, cancel,
  stored cards, BIN lookup. Twelve of iyzico's ninety-six documented
  operations, and none of them touches a card number.
- `IYZWSv2` request signing, and verification of the signature iyzico puts on
  every money-moving response. An unsigned response is refused unless
  `classic::Config::allow_unsigned` says otherwise.
- `in_store::Client::decrypt_callback`, which is how an In-Store payment
  actually finishes. Before it, the only way to learn an outcome was to poll.
- `Money::checked_add`, `checked_sub`, `is_zero`, and `PartialOrd`. No `+` or
  `-` operators: they would have to panic on a currency mismatch.
- `Currency::Jpy` and `Currency::Kwd`, which have zero and three decimal
  places. Every currency before them had two, so nothing exercised the rest of
  the arithmetic.
- `ErrorKind::Untrusted`, for a response that cannot be shown to be the
  provider's. Never retryable, and never to be acted on.
- `kasapay::async_trait`, so implementing `Provider` does not mean guessing
  which version of `async-trait` the trait was defined with.
- A request timeout on both adapters, 30 seconds by default. There was none,
  so a provider that stopped answering hung the caller forever.

### Fixed

- **A PayTR payment settled in roubles could not be read back.** The adapter
  sends `RUB` when it opens one, and the reverse mapping had no arm for it, so
  `charge_status` and `refunds` answered `ErrorKind::Unsupported` saying kasapay
  has no currency for PayTR's `RUB` — for a payment this crate had opened.

- **PayTR sent an empty currency rather than refusing one it does not take.**
  `paytr_currency` answered `""` for yen and dinar, and nothing checked it — the
  empty string was signed into the token and posted, so a payment PayTR cannot
  settle went out with no currency on it. It is `ErrorKind::Unsupported` before
  a socket opens now, which is what the iyzico adapters already did.

- **`specs/iyzico/` was missing fields iyzico documents.** The merge script kept
  one fragment per operation and preferred the English one, on the belief that
  the two languages differed only in prose. They differ in substance: the
  cancel-and-refund page carries `reason` and `description` only in Turkish,
  and the In-Store refund field is `refundAmount` in one language and
  `refundPrice` in the other — the two names issue #60 is about. The script now
  grafts every field one fragment documents and the chosen one does not, per
  operation rather than per page, and records each graft in the dated index.
  Twenty-seven fields returned; no operation or field was lost.

- **An In-Store callback said `"currencyCode": "0949"` and the crate called it
  malformed.** That is ISO 4217's numeric code for lira, and it is the only
  value iyzico publishes in a full example response — so every real decrypted
  callback and every status query was rejected where the amount should have
  been read. Both `0949` and `TRY` are read now; no other numeric code is
  guessed at, since this API settles in lira only.

- **A payment the payer did not complete came back as an error rather than a
  failed charge.** Such a callback carries `paymentFailedResult` where a
  settled one carries `transaction`, and `decrypt_callback` demanded the
  transaction. It now answers a `Failed` charge with the amount that was
  attempted.

- The In-Store module documentation said it was unsettled which version of
  `/crypt/decrypt` is current, and suggested pointing `Config` at the v2 base
  to reach the other one. It is v3, and that suggestion was wrong: v2 is a
  separate older integration whose other paths differ, so a client reconfigured
  that way would 404 on everything else.

- Stripe errors carry the decline code now — `insufficient_funds` rather than
  nothing — and the message is Stripe's own sentence rather than a Debug dump
  of their error struct.

- iyzico reported every refusal as a bad request. A declined card is now
  `Declined` and a bank timeout is `Provider` and retryable, from iyzico's own
  code list. Their `Retry` column is deliberately not used: it says `true` for
  "Email is mandatory", which means the shopper can correct it, not that the
  same request may succeed.

- PayTR reported every refund refusal as a flat rejection. Two of its
  documented codes say to try again later — the refund service being locked,
  and an insufficient balance — and are now retryable, so a caller's retry
  loop no longer gives up on a refund that would have gone through.

- `ChargeRequest::idempotency_key` was accepted and dropped by every adapter.
  Stripe sends it now; iyzico refuses the request rather than pretending.
- `Currency` no longer maps blindly onto Stripe's. Stripe settles in no
  three-decimal currency at all, so a Kuwaiti dinar is `Unsupported` rather
  than quietly turned into something else.

## 0.0.1 — 2026-08-14

First release. `Money`, `Charge`, `Error` and the `Provider` trait, with
Stripe over `async-stripe` and iyzico's In-Store API behind them.
