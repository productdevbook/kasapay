# Changelog

What changed, and what it costs a caller who upgrades. Kept by hand, in the
order releases happen, newest first.

## Unreleased

### Added

- **`ChargeRequest` carries a buyer, addresses and a basket**, so
  `Provider::charge` works on every provider in the workspace. It did not:
  iyzico's classic API and PayTR both answered `ErrorKind::Unsupported`,
  which made the library's central claim — that which provider takes the
  money is a deployment decision rather than a rewrite — false for exactly the
  two a Turkish shop would swap between. A caller had to reach past the trait
  and build a `classic::CheckoutForm` or a `paytr::Payment` by hand.

  New in `kasapay-core`, and re-exported from `kasapay`: `Buyer`, `Address`,
  `BasketItem` and `ItemKind`. New on `ChargeRequest` and its builder:
  `buyer`, `billing_address`, `shipping_address`, `basket` and `failure_url`.
  All optional, all ignored by the providers that do not ask for them, so no
  existing caller changes.

  An adapter that is not given a field it needs answers
  `ErrorKind::InvalidRequest` **naming the field**, before a socket opens.
  What iyzico requires: a surname, an identity number, a phone number, a
  billing address or one on the buyer, a `return_url`, and a category on every
  basket line. What PayTR requires: an email, a phone number, an address, and
  the IP the payer's own request came from.

  `ChargeRequest::amount` stays what the card is charged. For iyzico the basket
  is `price` and the amount is `paidPrice`, which is iyzico's own distinction
  and how an instalment surcharge is expressed.

  `classic::Client::start_checkout_form` and `PayTr::start_payment` are still
  there, for the settings the shared request has no word for: holding the
  money rather than taking it, which instalment counts to offer, a
  `cardUserKey` that is not the customer reference, and refusing instalments
  altogether.

- **`Stripe::at`**, a base URL and a secret key. Every other adapter takes a
  base; Stripe's did not, so pointing it at a mock server or a logging proxy
  meant depending on `async-stripe` directly and going through
  `Stripe::with_client`. That escape hatch stays, for the things it is
  actually for — a timeout, a pinned API version, an account to act on behalf
  of.

- **`Money::checked_mul`**, a unit price times a count, refusing the overflow
  that would wrap a line total round to a negative one.
  `BasketItem::line_total` is what uses it: `BasketItem::price` is what *one*
  costs, because PayTR takes the unit price and the count as two fields while
  iyzico takes one figure and has nowhere to put a count.

## 0.0.4 — 2026-08-19

### Added

- **`terminal::gmu`** — VUK 507, the Terminal API's other integration, all
  nine operations of it. A merchant chooses this one or VUK 509 with iyzico,
  and the difference is what a sale is: here the device issues the fiscal
  document, so the sale carries every line with its unit code and VAT group,
  the document type, and the buyer's tax office and number when the buyer is a
  business.

  Which makes its refund a different act too: `gmu::Client::refundable_sale`
  answers what is still returnable per line, and a refund names the lines
  coming back rather than an amount. And its partial payment holds a sale open
  across three calls — a sale never completed is one nobody has been charged
  for and the device is still holding.

  With this, all fourteen `terminal-host` operations iyzico documents are
  implemented.

- **`terminal::Client::end_of_day`**, `POST /v2/terminal-host/eod`. A till runs
  this once a day and the bank expects it: until the batch is closed the day's
  sales are authorised and not settled. It answers a batch number and a total
  per acquiring bank, because a device can be set up with more than one.

  Both figures are text. iyzico types them as strings and names no currency for
  the amount anywhere in that answer, so reading either as a number would be
  inventing a unit they did not send.

- **iyzico's subscription module can subscribe somebody.** It had the
  catalogue — products and plans — and nothing that sold one. Thirteen
  operations: `start_subscription_form` and `subscription_form_result`,
  `subscribe` for a customer iyzico already holds a card for,
  `start_card_update_form`, `subscription`, `subscriptions`, `activate`,
  `cancel`, `upgrade`, `retry_payment`, `subscriber`, `subscribers` and
  `update_subscriber`.

  Twenty-three of iyzico's twenty-four. The one left out is
  `POST /v2/subscription/initialize`, which takes a card number — so every way
  into a subscription here goes through a form iyzico hosts, including
  replacing the card an existing subscription is charged to.

  `Upgrade` is where the decisions with money in them are: when the new plan
  applies, whether its trial is given to somebody who has already paid, and
  whether the count of payments starts again. All three default to the answer
  that does not give anything away.

- **iyzico's marketplace can pay a sub-merchant now.**
  `onboarding::Client::approve_item`, `disapprove_item` and
  `update_item_payout` — iyzico holds a split line's share until the platform
  says the buyer got what they paid for, and this module could open a
  sub-merchant's account and never release its money. All three are keyed by
  `paymentTransactionId`, the split's own id, which is a basket line rather
  than a payment: a payment with three lines has three of them, and approving
  the wrong one pays the wrong seller, so the id iyzico echoes is checked
  against the one asked about.

- **`PayPal::void_authorization`**, `POST /v2/payments/authorizations/{id}/void`.
  The one call in `kasapay-paypal` that gives a payer their limit back without
  money having moved — and the gap the crate had been documenting rather than
  closing: `Provider::cancel` refuses because Orders v2 withdraws nothing by an
  order id, and a caller who placed a hold and decided not to take it had
  nothing here.

  It is still not `Provider::cancel`, and cannot be: PayPal voids the
  authorization and that signature takes a `PaymentId`, which names the order.

  PayPal answers `204 No Content` by default and the authorization when
  `Prefer: return=representation` is sent. This sends the header, the way every
  call in the crate does, and reads the empty body as the success it is —
  `Voided::amount` is `None` there rather than a figure invented from a request
  that carries none either.

  `specs/paypal/` keeps the operation now, which is why its dated meta moves.

- **`Mollie::refunds`**, and `Mollie::mandates` walks to the end now. Listing
  refunds was left out because "that call paginates, and half a list is worse
  than none" — which was the right reason and the wrong conclusion: a
  half-read list of refunds undercounts what has gone back, and a shop acting
  on the total refunds money it has already returned. Both calls now ask for
  Mollie's largest page and follow `_links.next` until there is none.

  `Provider::instruments` is `Mollie::mandates`, so a customer with more than
  fifty mandates was quietly getting fifty.

  The cursor is followed and the address it arrived in is not: `_links.next` is
  a whole URL, and this reads the `from` out of it and asks its own base. A
  response that can send the next request anywhere decides where a merchant's
  API key goes. A cursor that does not move ends the walk rather than looping.

- **`classic::Client::start_pay_with_iyzico`**, iyzico's
  `/payment/pay-with-iyzico/initialize`. The payer signs in to their own iyzico
  account and pays with a card already there. The same `CheckoutForm` opens it
  and `classic::Client::checkout_result` reads it back — not an approximation:
  iyzico documents Pay with iyzico's retrieve at the same path the checkout
  form's own result is read from. Their answer carries the member's email and
  phone number, on `Charge::raw`.

- **`classic::Client::instalments`**, iyzico's `/payment/iyzipos/installment`.
  What a Turkish checkout needs before it draws its payment page: which
  instalment counts a card's bank allows for an amount, and what the payer pays
  for each. Every count is typed, including the single payment, because unlike
  PayTR's own instalment service iyzico documents the entry shape.

  `Instalment::total` is what the payment is opened for — the surcharge is the
  merchant's arrangement with the bank and appears on nothing else — and
  `Instalment::surcharge` is the difference from the basket.

  Amounts on this endpoint are JSON **numbers**, where the same API writes
  decimal strings everywhere else. Both directions go through the literal text
  rather than an `f64`: `33.33` through a float and back is
  `33.329999999999998`, which `Money::parse` would refuse — correctly, and for
  the wrong reason.

### Fixed

- **A paginated read cannot loop for ever.** `Stripe::refunds` and
  `Stripe::stored_cards` walk Stripe's cursor and stopped only on an empty
  page, so a page repeating its own last id would have hung — and `refunds` is
  how "how much has gone back" gets answered, which makes a hang a checkout
  that hangs. Every cursor followed is remembered now, and one seen twice ends
  the walk with what it has. Mollie's walks carry the same guard.

## 0.0.3 — 2026-08-19

### Breaking

- **`Provider` gains `lookup`, and `Capabilities` a `lookup_by_order`.**
  `lookup(&OrderRef) -> Result<Option<Charge>, Error>` answers what became of a
  request the caller sent under their own reference — the question a charge
  that timed out leaves, and the one `charge_status` cannot answer because it
  takes an identifier the lost reply never delivered. A `Provider` written
  outside this workspace has one more method with no default, and a
  `Capabilities` built as a struct literal one more field.

- **`kasapay_paytr::PayTr` implements `kasapay_core::Webhook`.** A method named
  `verify` on a trait now in scope can shadow an inherent one in code that
  imports both; nothing else changes.

- **`Provider` gains `refund`.** Every adapter in this workspace implements it;
  a `Provider` written outside has one more method to answer, and there is no
  default because a provider that cannot refund has to say
  `ErrorKind::Unsupported` itself rather than have it assumed. The shape is
  `refund(&RefundRequest) -> Result<Refund, Error>` — a request built the way a
  `ChargeRequest` is, and an answer with its own identifier, its own
  `RefundStatus` and, at one provider, its own `NextAction`.

  The provider-specific refunds are all still there and still the way to reach
  what the trait cannot say: iyzico's per-line `refund_transaction`, PayPal's
  refund against a capture id, Stripe's and PayTR's lists of what has already
  gone back.

- **`kasapay_mollie::RefundId` and `kasapay_paypal::RefundId` are core's.**
  Both were `Id<kind::Refund>` over a kind of their own; both are now
  `kasapay_core::RefundId`, because the shared trait answers a refund and the
  concept is no longer one provider's. The name, the methods and the string on
  the wire are unchanged — only code that named `kasapay_mollie::id::kind::Refund`
  or its PayPal twin has anything to change, and it can name
  `kasapay_core::kind::Refund` instead.

- **Mollie's refund carries a description and metadata.** Internal to the
  crate: `Mollie::refund` is unchanged, and `Provider::refund` is what fills
  them in.

### Added

- **`terminal::Config::timestamps`**, and `terminal::Timestamps`. iyzico asks
  the Terminal API's login for "the Unix timestamp value of the relevant
  request" and never says in what unit, while their own classic API writes
  `systemTime` in milliseconds. Seconds is still what is sent; this is the
  switch, and it exists because that particular guess fails *quietly* — a
  rejected timestamp reads as a login that did not work. The other two things
  #96 could not settle fail loudly and have no switch.

- **`PayTr::instalment_rates`**, `/odeme/taksit-oranlari` — what a checkout
  reads before it offers "pay in three", since the surcharge is the store's own
  and is on no payment response. The four fields PayTR documents are typed; the
  rates themselves stay on `InstalmentRates::raw`, because PayTR describes
  `oranlar` as "the rates … in array format" and nowhere says what one entry
  holds. A struct for it would be a shape invented here, and #73 is what a real
  response body finishes.

- **iyzico holds money as well as taking it.**
  `classic::Client::start_checkout_form_preauth` opens the hosted form in
  pre-authorisation mode, `classic::Client::preauth_with_saved_card` is
  `/payment/auth`'s own request sent to `/payment/preauth`, and
  `Provider::capture` is `/payment/postauth` — so `separate_capture` and
  `partial_capture` are now true for the classic API, and a shop can authorise
  when the order is placed and capture when the parcel leaves. Sixty of
  iyzico's ninety-six documented operations; `scripts/coverage.py` says which.

  `classic::Client::checkout_result_preauth` reads a pre-auth form's result:
  the same endpoint as `checkout_result` and the same token, but the caller
  says which form they opened, because iyzico answers `SUCCESS` for a payment
  taken and for one only held and documents no values for the `phase` field
  that would separate them. `Provider::charge_status` has the same limit and
  says so.

- **Two providers can say whether a call landed.** iyzico's `classic` reads a
  payment back through reporting by the `conversationId` it was made with;
  PayTR's status query is keyed by `merchant_oid`, which is the reference
  itself. The other four answer `ErrorKind::Unsupported` and say what to do
  instead on their own `lookup` — for all four it is retrying with the same
  idempotency key, which those providers do honour. Stripe's is the one worth
  reading: its search API is the only call that finds an intent by metadata,
  and Stripe documents it as too far behind to be used in a read-after-write
  flow, so answering `Ok(None)` from it would be how a caller charges twice.

- **`Webhook`, `Delivery`, `Event`, `EventKind` and `EventId` in
  `kasapay-core`, and four implementations.** `Webhook::verify` takes the
  headers and the bytes of a delivery, shows they are the provider's, and says
  what they mean. It is a separate trait from `Provider` because verifying
  needs a secret the API credentials do not carry, and it is `async` because
  two of the four providers verify over the network rather than with a hash.

  `kasapay_stripe::Webhooks` checks the `Stripe-Signature` HMAC over
  `timestamp.body`, constant-time, with a five-minute tolerance a correctly
  signed replay falls outside. `kasapay_paytr::PayTr` implements it directly —
  PayTR's notice hash is keyed with the credentials the client already holds.
  `kasapay_mollie::Mollie` reads the payment back, which is what Mollie's own
  documentation says to do with an unsigned delivery.
  `kasapay_paypal::Webhooks` asks PayPal, at
  `/v1/notifications/verify-webhook-signature`.

  `kasapay-iyzico` implements none, and its crate documentation says why: the
  In-Store callback is an encrypted blob that only opens with the
  `paymentSessionToken` of the payment it belongs to, which the delivery does
  not carry and `verify(headers, body)` has nowhere to take.

- **`RefundRequest`, `Refund`, `RefundStatus`, `RefundReason` and `RefundId` in
  `kasapay-core`.** A refund is its own object with its own life, which is why
  `Status` still has no `Refunded` — no provider reports one as a payment
  status, so the variant would be a branch that never runs for most of them.
  `Refund::id` is an `Option` for the provider that issues none, the same
  reason `Charge::id` is.

- **`RefundReason` is exhaustive.** Not `#[non_exhaustive]`, for the reason
  `Currency` is not: it travels out to a provider rather than back to a caller,
  and an adapter meeting a reason it has no word for would send none at all.
  Adding one later is a breaking change that makes every adapter answer.

- **`RefundRequest` carries a `customer` and a `return_url`.** iyzico's
  In-Store API wants a `userId` and a callback address on a refund exactly as
  it does on a payment, and its refund is the one in the workspace that answers
  a `NextAction`: the payer approves it in iyzico's app and the money moves
  there. Every other adapter ignores both fields.

- **`kasapay_stripe::REFUND_REASON_METADATA_KEY`.** Stripe's `reason` takes
  three values and none of them is free text, so `RefundReason::Other`'s own
  words travel as refund metadata under this key rather than being dropped.

## 0.0.2 — 2026-08-15

Three more providers — PayTR, Mollie and PayPal — and a shared trait shaped by
all five rather than by two. `kasapay-paytr`, `kasapay-mollie` and
`kasapay-paypal` are published here for the first time.

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

- **`Capabilities` gains `saved_instruments`.** Whether a card the provider
  already holds can be charged through this adapter with the payer entering
  nothing — what a checkout reads before it offers "use my saved card", for the
  same reason `separate_capture` exists. A `Capabilities { .. }` written out in
  full needs one more field; one built from `..Capabilities::default()` needs
  nothing. True for `iyzico::classic` and false for the rest, and false means
  this adapter has no call that charges one rather than that the provider has
  no vault.

- **`classic::CheckoutForm` gains `card_user_key`.** A struct literal needs one
  more field; `CheckoutForm::builder(..)` needs nothing, and
  `CheckoutFormBuilder::card_user_key` is how it is set. `None` sends no field
  and is what every existing form already did.

- **`classic::StoredCard::token` is an `InstrumentId`, and
  `classic::Client::forget_card` takes one.** iyzico's `cardToken` and its
  `paymentId` are both iyzico's own strings, so nothing but the type separates
  them; now the compiler does. `forget_card(&key, "tok-1")` becomes
  `forget_card(&key, &InstrumentId::issued("tok-1"))`, and a token read off
  `stored_cards` passes straight through. Reading the text back is
  `card.token.as_str()`.

- **`Provider` gains `instruments`, with no default.** A provider outside this
  workspace has to answer it, the same reason `capture`, `cancel` and
  `capabilities` do before it. `customer` is the provider's own name for
  whoever the instruments are saved against — the same string
  `ChargeRequest::customer` carries, and for `iyzico::classic` its
  `cardUserKey`. An adapter with nothing to list — no vault, or one this crate
  has no working call against — answers `ErrorKind::Unsupported`, the way
  `PayTr` now does, rather than an empty list that would read as "this
  customer has nothing saved".

- **`Provider::capture` gains `idempotency: Option<&IdempotencyKey>`.**
  Repeating a capture takes the money twice; repeating `Provider::cancel`
  meets a hold already released and answers `ErrorKind::InvalidRequest`, which
  is why only `capture` changes. Every existing call needs one `None` added at
  the end: `provider.capture(id, amount)` becomes
  `provider.capture(id, amount, None)`. `ErrorKind::is_retryable`'s own doc
  table now says what a timeout means for a capture with a key and without
  one, provider by provider.

  Stripe and Mollie send it as `Idempotency-Key` on the capture call the same
  way they do on opening a charge — Mollie's on the captures endpoint
  specifically, answered from its usual hour-long cache. PayPal's
  `capture_order` already took its own `request_id` for exactly this reason;
  the trait method now feeds that parameter instead of fixing it to `None`, so
  a caller going through `Provider` alone gets the same guarantee against a
  duplicate capture that PayPal documents for `PayPal-Request-Id` on this
  call. iyzico refuses an idempotency key generally (#54) but implements no
  capture at all, so both `classic` and `in_store` ignore the argument rather
  than refuse it — there is no request for a key to travel with either way.
  PayTR has no capture step; its hosted form takes the money as it goes.

### Added

- **`kasapay_iyzico::reporting`, iyzico's reporting service** — #8's two
  remaining operations: `payment_details` (`GET /v2/reporting/payment/details`)
  and `daily_transactions` (`GET /v2/reporting/payment/transactions`), a
  payment's status, fraud result, cancels and refunds read back after the
  fact. Left out once because `classic`'s own payment-status mapping was being
  rewritten at the time and a second, independent copy of it would have
  quietly diverged; that reason is gone, so `PaymentDetail::fraud_status`
  reuses `classic`'s own `fraudStatus` interpretation rather than repeat it.
  `paymentStatus` could not get the same treatment — iyzico's own `2` means
  *"Failure / INIT_THREEDS"*, one code for two states `classic` tells apart —
  so it stays its own `PaymentStatus`, plainly not `kasapay_core::Status`,
  rather than guess which of the two it was.

  No worked example exists for either operation, in either language — the same
  situation `kasapay_iyzico::softpos` was implemented under, and for the same
  reason nothing here stood in with an invented one: every field is read from
  iyzico's own schema, and every test is about a mapping this crate controls,
  not bytes iyzico was never shown to send.

  `cardstorage`'s and `in-store`'s own remaining gaps were investigated and
  left alone rather than guessed at. `POST /cardstorage/card` genuinely needs
  a card number in every documented request shape — `classic`'s module
  documentation already said so, and #97's `cardUserKey`-filled checkout form
  is a different endpoint, not this one. `in_store`'s `POST
  /v3/in-store/payment/query` shares its path with the `GET` this crate
  already implements and answers in a different, `PascalCase` shape found
  nowhere else in the API; `in_store`'s module documentation now says why that
  reads as a stale reference page rather than a second implementation to add.

- **`kasapay-paypal`, a fifth provider — the first that is neither card-first
  nor a redirect-and-forget checkout.** `kasapay = { features = ["paypal"] }`.
  PayPal's Orders v2 API behind the same trait, deliberately scoped to its
  spine: `charge` creates an order with `intent: CAPTURE` and answers a
  `NextAction::Redirect` to PayPal's approval page, `charge_status` reads it
  back by the order id PayPal issued, and `capture` takes the funds once the
  payer has approved it. Tests run against `wiremock` with PayPal's own
  documented example bodies from `paypal/paypal-rest-api-specifications`.

  **`Provider::cancel` always refuses, and it is not a gap in this crate.**
  `/v2/checkout/orders` itself has no cancel or void operation — no `DELETE`,
  nothing that withdraws an order by its own id — so the trait method this
  workspace's other four providers each answer for real has nothing to call
  here. An order the payer never approves is simply left to age off PayPal's
  own side. (PayPal's Authorizations resource does document a void, for a
  hold rather than an order — see the refund/`AUTHORIZE` entry below for why
  that still leaves this refused.)

  **Every PayPal order needs an explicit capture regardless of intent**, which
  is not true of Mollie's automatic-capture payment or a succeeding Stripe
  PaymentIntent. So `capabilities().separate_capture` is `true`
  unconditionally, `partial_capture` is `false` — the Orders-level capture
  takes no amount at all, so `Provider::capture` refuses `Some` before a
  socket opens — and `Provider::charge` never creates an order with
  `intent: AUTHORIZE`: that intent buys a longer hold through a separate
  Authorizations resource this crate does not implement, not a way to skip the
  capture call.

  **OAuth2 client-credentials, and this client renews its own token** — a
  deliberately different choice from `kasapay_iyzico::terminal`, whose caller
  owns the token and which never renews automatically. Getting a bearer token
  has no real-world action in it the way presenting a card at a terminal does,
  so there is no replay ambiguity in refreshing one early; `PayPal` checks a
  cached token's expiry before every call rather than after one fails, and
  still does not retry a failed *business* call on its own. Replaying a
  capture without care can capture twice — PayPal takes a `PayPal-Request-Id`
  on that call the same as it does on opening a charge, and `Provider::capture`
  now carries its own `idempotency` to send it with (below); at the time this
  crate shipped it did not, and `PayPal::capture_order` was the one place a
  caller working directly against this crate could pass one.

  **PayPal takes neither Turkish lira nor Kuwaiti dinar**, the same two Mollie
  refuses, because both are simply absent from PayPal's twenty-five-currency
  list. `ChargeRequest::customer` and `::metadata` are not read — Orders v2
  names no payer identity outside its separate Vault API and has no free-form
  key/value bag — and `::return_url`, when set, is sent as both PayPal's
  `return_url` and `cancel_url`, the same simplification Mollie's one
  `redirectUrl` makes, because `ChargeRequest` has no field for the second.

  **PayPal's own documented examples never show a created, read or captured
  order's top-level `status`**, despite the schema declaring one. This crate
  reads a status instead from whichever of a capture's own status, an
  authorization's own status, the top-level status when it is finally present,
  or an `approve`/`payer-action` link actually carries one — unverified
  against a live sandbox account; the crate documentation says which order and
  why. Its capture status also folds `PARTIALLY_REFUNDED` and `REFUNDED` into
  the same enum as `COMPLETED`, the first provider here to do that; both read
  as `Status::Captured`, and `Status`'s own table now carries PayPal's row and
  says so.

  `specs/paypal/` records PayPal's OpenAPI document — Apache-2.0, permissive
  like Stripe's, so the subset is kept rather than thrown away the way
  Mollie's is. `scripts/fetch_paypal.py` cuts `checkout_orders_v2.json` to the
  three operations this crate maps and rolls the subset forward in
  `latest.yaml`, the same as `fetch_stripe.py`.

- **`kasapay-paypal` gains refunds and `intent: AUTHORIZE`**, the two gaps
  #113 named on purpose. `PayPal::refund` is `POST
  /v2/payments/captures/{id}/refund` — **against the capture, not the
  order**, the same split `kasapay_mollie::Mollie::refund` draws — whole or
  partial, and repeatably up to what was captured; `CaptureId` is its own
  kind of `kasapay_core::Id`, the way Mollie's own `CaptureId` and `RefundId`
  are, and `kasapay_paypal::capture_id` reads one off a `Charge` that
  `PayPal::capture_order` or `Provider::charge_status` answered, since
  PayPal nests it rather than carrying it on a field of its own.
  `Capabilities::partial_refund` and `::repeated_refund` are `true` now —
  PayPal's own `capture_status` enum has named `PARTIALLY_REFUNDED` since
  before this crate existed, and #113 said plainly that was about this
  crate rather than the API.

  `PayPal::authorize` opens an order with `intent: AUTHORIZE`, mirroring
  `Mollie::authorize`; `PayPal::authorize_order` places the hold once the
  payer approves, through PayPal's own `/authorize` operation on Orders v2;
  `PayPal::capture_authorization` takes the funds, through the
  Authorizations resource's `POST
  /v2/payments/authorizations/{id}/capture` — keyed by the new
  `AuthorizationId`, not the order id, because that call answers a bare
  capture object with no order id or authorization id in its own body, only
  in a `links[].href`. None of the three is reachable through `Provider`:
  the trait has no `authorize` method and `Provider::capture` stays wired to
  `PayPal::capture_order`, which still refuses an `AUTHORIZE`-intent order
  with PayPal's own `ACTION_DOES_NOT_MATCH_INTENT`.

  **Left out, and said so rather than guessed at:**
  `POST /v2/payments/authorizations/{id}/void`, which releases a hold
  without capturing it. PayPal documents it, and it is keyed by the
  authorization's own id the same way capturing one is — but
  `Provider::cancel` takes only an order id, so this crate does not call it;
  a caller who places a hold and decides not to take it has no release call
  here. Reading a capture, an authorization or a refund back by id, listing
  refunds, and reauthorizing an expired hold are the same kind of gap.

  `scripts/fetch_paypal.py` now fetches two of PayPal's documents rather
  than one — `checkout_orders_v2.json` for `authorize` alongside the
  original three operations, and the new `payments_payment_v2.json` for the
  refund and authorization-capture operations — into `specs/paypal/latest.yaml`
  and the new `specs/paypal/payments-latest.yaml`.

- **`kasapay_iyzico::agent` and `kasapay_iyzico::softpos`, all five PayPOS
  operations from #8.** `specs/` records these five as declaring neither a
  `securityScheme` nor a classic-shaped `Authorization` parameter — one of
  only two ways that happens in the whole of iyzico's documentation, the
  other being In-Store's plain headers. Establishing which of three things
  that meant — an omitted classic signature, a different scheme the
  fragments still carry, or truly nothing — turned out to matter twice over:
  both languages' prose (not just the parameter list) describe a dealer
  secret key trading for a mobile session key, and every one of the five
  fragments, in both languages, declares its own `servers` block pointing at
  `api.paynet.com.tr` / `pts-api.paynet.com.tr` — Paynet's own hosts, not
  `api.iyzipay.com`. `scripts/merge_iyzico.py` drops a fragment's own
  `servers` and always writes iyzico's pair instead, so
  `specs/iyzico/agent/latest.yaml` and `specs/iyzico/softpos/latest.yaml`
  both show the wrong host at the top level; `kasapay_iyzico::agent`'s module
  documentation carries the full evidence, including the integration
  overview page's own prose `BaseUrl` section confirming it outside any
  OpenAPI fragment.

  `agent::Client::get_auth_key` and `agent::Client::logout` speak that
  scheme: `Authorization: Basic {secret_key}` and a fixed `PaynetMobile: 2`
  header, neither of which is `IYZWSv2` signing. The `Session` it answers is
  what `softpos::Client::new` authenticates with, over `Client::init_sale_transaction`,
  `Client::init_reversal_transaction` and `Client::check_transaction`.
  `softpos::InitSale::new` takes only `Currency::Try`: PayPOS's schema types
  its amount as a bare `number` with no `currency` field anywhere beside it,
  and `specs/README.md`'s per-product currency table names no enum for
  `softpos` at all, so the restriction is inference from context rather than
  a documented enum, and is said as such. Reading is the permissive
  direction this crate uses everywhere else: `Client::check_transaction`
  reads a `Transaction`'s amount in whatever `Currency` its `currency` names,
  not only lira, and only falls back to `Transaction::raw` for a code
  `Currency` cannot name at all.

  `PayPOS` and `PayPos` join `clippy.toml`'s `doc-valid-idents` — vendor
  proper nouns next to `PayTR`, not identifiers this crate can link to.

  Neither language's page for any of the five operations carries a worked
  example, so `tests/agent.rs` and `tests/softpos.rs` are stand-ins built
  from PayPOS's own field names, the same position `mass`'s undemonstrated
  operations are in — no live PayPOS account was available to check any of
  it against. What a success answer means is unverified too: both `agent`
  operations answer the identical shape whether iyzico calls it a success or
  a failure, with no documented meaning for `code`, so both modules read
  HTTP status alone and carry the body's `code` on `Error::code` unread.

- **`kasapay_iyzico::onboarding`, all three iyzico sub-merchant operations.**
  Creating, updating and reading back a marketplace sub-merchant — a different
  legal person taking money through the platform's own iyzico integration, so
  what this sends is compliance data rather than payment data: a name, an
  email, a national ID or tax number, an IBAN. `onboarding::Client::new(classic)`
  is the whole setup, the same as `iyzilink` and `mass`.

  iyzico carries a sub-merchant's kind — personal, private company, or limited
  or joint-stock company — as one `subMerchantType` string next to a bag of
  fields that are conditionally required depending on it. `NewSubmerchant`'s
  three variants — `PersonalSubmerchant`, `PrivateCompanySubmerchant`,
  `LimitedJointSubmerchant` — each carry only the fields their own kind
  requires, so a personal sub-merchant missing a contact surname, or a limited
  company missing a tax number, is a compile error. `SubmerchantUpdate` does
  the same for `PUT /onboarding/submerchant`, with one wrinkle iyzico's own
  documentation creates: the private-company and limited/joint-stock update
  schemas are byte-for-byte identical, so both of `SubmerchantUpdate`'s
  matching variants hold one `CompanyUpdate` rather than two structs that would
  only ever differ by name.

  An IBAN is not a card number, but it is still somebody's banking detail:
  every place one travels through this module is a `kasapay_core::Secret`
  rather than a plain string, so `{:?}` on a sub-merchant, an update or a read
  answer never puts an account number in a log by accident. A national ID and
  a tax number travel as a plain string, the same as everywhere else in this
  crate.

  Onboarding responses carry no signature — none of the three schemas
  documents one — so none of this is checked against a forged answer; TLS is
  what stands between a caller and iyzico here. And neither documentation
  language gives a worked example for any of the three operations, only the
  schema, so the tests are built from field names with stand-in values, the
  same as `mass`'s undemonstrated operations — no live marketplace account was
  available to check any of this against.

  Left out: the Agent API's two operations (`/v1/agent/get_auth_key`,
  `/v1/agent/logout`), which are not a sub-merchant operation at all — a
  mobile "app2app" session for the PayPOS product, authenticating with a plain
  `Authorization: Basic sck_…` header rather than `IYZWSv2` signing. Bolting a
  second authentication scheme onto a client built for a different one, for
  two calls unrelated to a sub-merchant's paperwork, was not "fitting cleanly".

- **`kasapay_stripe::saved`, and `Stripe::capabilities().saved_instruments` becomes
  `true`.** The second provider written against #97's `InstrumentId`, and the
  one #97 said would be the case that is not half a name: Stripe's `pm_…`
  stands alone, made by Stripe.js or Elements in the payer's own browser, so
  `saved::Payment` carries the customer beside it rather than needing a second
  handle the way iyzico's `cardUserKey`/`cardToken` pair does.

  `Stripe::stored_cards` is `GET /v1/customers/{customer}/payment_methods`,
  filtered to `type=card` and paged the way `Stripe::refunds` already is.
  `Stripe::charge_saved_card` confirms a PaymentIntent with `customer` and
  `payment_method` set — Stripe's own documented shape for charging a saved
  card. Left on session, a card that needs 3-D Secure comes back as an
  ordinary stalled `Charge` for the payer to complete; `saved::PaymentBuilder::off_session`
  marks the payer absent instead, and Stripe skips the challenge where the
  card's rules allow it — where they do not, it answers an error carrying
  `authentication_required` rather than a `Charge`, because there is nobody
  present to complete one, and a caller has to handle that. `Stripe::forget_card`
  detaches one.

  `detach` has no generated form in `stripe_core` — it lives in
  `async-stripe-payment`, a resource crate this workspace does not otherwise
  need. Rather than pull in every other payment-method type for one call,
  `Stripe::forget_card` is a `StripeRequest` written by hand against
  `async-stripe-client-core`, the request-builder plumbing every generated
  request already sits on and which this crate already carried transitively
  through `async-stripe`. `async-stripe-core` gains the `customer` feature,
  already-generated and just switched on, for `Stripe::stored_cards`.

- **`kasapay_core::InstrumentId`, and `iyzico::classic::Client::pay_with_saved_card`
  — charging a card the provider holds, without a card number anywhere.**
  `Id<kind::Instrument>`, the same shape as `PaymentId`: it names one saved
  instrument, and `IdSource` still says whose uniqueness that rests on. It is
  half the name at two of the three providers shipped here — iyzico's
  `cardToken` needs the `cardUserKey` beside it, PayTR's `ctoken` its `utoken`
  — and the other half is the payer, which `ChargeRequest::customer` already
  carries. Stripe's `pm_…` stands alone.

  The iyzico call is `POST /payment/auth` with `paymentCard` filled by
  `classic::saved::Card`, a pair that has no field for a card number. That is
  the endpoint an ordinary card payment uses, so **iyzico's most-used payment
  operation turns out to be reachable without a pan** — for a card they already
  hold. `classic::saved::Payment` carries what iyzico wants around it: the
  buyer, both addresses, the itemised basket, and an optional instalment count.

  `saved::Card::new` refuses a value that is a card number by shape — twelve to
  nineteen digits and nothing else, passing the Luhn check — in either half, so
  a field wired to the wrong source is a `CardError` rather than a pan on the
  wire. It is a check against a mistake, not a security control.

  **Nothing here stores a card through an API call**, because both ways of
  doing that carry the number: `POST /cardstorage/card` wants `cardNumber`,
  `expireMonth`, `expireYear` and `cardHolderName`, and `registerCard: 1`
  stores the card a payment is already carrying. The hosted checkout form is
  the pan-free way in, and it now closes the loop —
  `checkout::CheckoutFormBuilder::card_user_key` sends the key so the form
  offers a payer their saved cards and files a new one under the same key, and
  `Charge::raw` at `/cardUserKey` and `/cardToken` is where the pair comes back
  off `checkout_result`. **Neither field is in `specs/`**: iyzico's
  documentation of that request and that response mentions neither, and their
  own SDKs send and read both, so this follows the SDKs and says so.

  There is no 3-D Secure variant, because this crate implements neither 3-D
  Secure call — not because a stored card could not go through one. iyzico's
  own description of `/payment/auth` says a stored card can be charged "NON3D
  or 3DS". What that means today is that a payment taken through
  `pay_with_saved_card` is unauthenticated and the chargeback liability sits
  with the merchant.

  The response is verified against the same six signed fields as any other
  payment. Its status comes from `fraudStatus`, which is the one thing left
  that can hold a payment up: iyzico documents 1 as approved, 0 as under review
  and -1 as rejected, and a payment under review is `Status::Pending` rather
  than `Status::Captured`. That mapping has not been checked against a live
  account.

- **`kasapay-mollie`, a fourth provider — and the first that is neither
  card-first nor Turkish.** `kasapay = { features = ["mollie"] }`. Mollie's
  Payments API behind the same trait: `charge` opens a hosted checkout and
  answers a `NextAction::Redirect`, `charge_status` reads it back by the
  `tr_…` Mollie issued, `capture`, `cancel`, and `Mollie::refund` beside
  Stripe's and iyzico's. Tests run against `wiremock` with Mollie's own
  documented example bodies.

  `capabilities()` answers the `saved_instruments` #97 added, and answers
  `false`. Mollie's saved instrument is a mandate — `mdt_…`, held against a
  customer, charged with `sequenceType: recurring` — and no call in this crate
  sends one, so a checkout must not offer use-my-saved-card here. A boundary of
  the adapter rather than of Mollie, which is the distinction that field's
  documentation draws.

  Nothing existing changes. What is worth reading before writing against it is
  the four places Mollie does not fit the shape three card-first providers gave
  the trait.

  **Mollie takes neither lira nor Kuwaiti dinar.** Seven of `Currency`'s nine —
  USD, EUR, GBP, JPY, RUB, CHF, NOK — and TRY and KWD are
  `ErrorKind::Unsupported` before a socket opens. Amounts go as
  `{"currency": "EUR", "value": "10.00"}`, written from `Money`'s integer minor
  units; yen goes as `1200`, with no decimal point, which is what Mollie
  documents for it.

  **A hold is `Mollie::authorize`, not `charge`.** Mollie decides at creation
  whether a payment is captured automatically or held for a later capture, and
  `ChargeRequest` has no field that says which. So `Provider::charge` opens a
  payment captured the moment the payer finishes, and `authorize` opens the
  same one with `captureMode: manual`. `capabilities().separate_capture` is
  still true: it describes Mollie, and `authorize` is the way to it.

  **Releasing a hold is not `cancel`.** `Provider::cancel` is Mollie's
  `DELETE /v2/payments/{id}`, which withdraws a payment the payer has not
  finished and answers the cancelled payment. Releasing an authorisation is
  their `release-authorization`, and it cannot be on the trait: Mollie answers
  it `202 Accepted` **with no body at all**, and says the issuing bank decides
  if and when the hold lifts. `Mollie::release_authorization` returns
  `Ok(())` and the payment is read back afterwards.

  **Mollie's `expired` has no `Status`.** A payment the payer abandoned until
  it could no longer be paid is neither refused nor withdrawn. It arrives as
  `Status::Failed` — final, no money moved — and which of Mollie's two it was
  is in `Charge::raw`. `Status`'s own table now carries Mollie's row and says
  so.

  Two smaller things a caller meets. A capture and a refund are Mollie's own
  objects with their own identifiers, so `mollie::CaptureId` and
  `mollie::RefundId` are kinds of their own beside `PaymentId` — `cpt_…`,
  `re_…` and `tr_…` are all Mollie's strings and `IdSource` could never
  separate them. And **Mollie's errors carry no code**: their error object is
  `status`, `title`, `detail` and sometimes `field`, so `Error::code` is `None`
  for every failure this adapter reports, where PayTR's `err_no` and iyzico's
  `errorCode` are not, and the offending field is named in the message.

  Mollie's webhook posts one form field — the payment's id — and nothing that
  proves it came from Mollie. No verification is built here, and the crate
  documentation says what a handler must do instead: read the payment back and
  act on nothing else in the request.

- **`specs/mollie/` records Mollie's OpenAPI document without keeping a copy of
  it**, which no other provider here does. Mollie licenses theirs
  CC-BY-NC-SA-4.0 — non-commercial, share-alike — and this repository is MIT: a
  file under those terms inside an MIT tree is a restriction on exactly the
  commercial users the licence invites, and one they would not notice.
  Subsetting does not change that, because a cut-down copy is still a copy.

  So `scripts/fetch_mollie.py` fetches the document, cuts it to the five paths
  kasapay maps, checks it, writes `<date>.meta.json` and throws the rest away.
  The meta carries the version, the licence, the paths and `operationId`s, the
  repairs, and two hashes: `upstream_sha256` moves when anything in Mollie's
  1.9MB does, and `subset_sha256` only when one of the five paths does.
  `--write-document` writes the subset for reading, to a path `.gitignore`
  covers.

  What that costs is the field-level diff the other three get. `compare_specs.py`
  now names the providers it cannot speak for — Mollie and PayTR — rather than
  saying nothing about them, because silence reads as "nothing changed".

  **Mollie's own document is not quite valid OpenAPI**, which is worth recording
  either way: five parameters on those paths are a `$ref` with a `schema`
  beside it, and OpenAPI 3.1 lets a Reference Object carry `summary` and
  `description` and nothing else. The fetcher inlines the referenced parameter,
  overlays the siblings, drops nothing, and names each repair in the meta. The
  same pattern inside a schema is left alone — legal in JSON Schema 2020-12,
  and thirty-three of Mollie's are that. Because nothing is committed, the
  validation moved into the fetcher, which exits non-zero rather than recording
  a document it could not check.

- **`kasapay-mollie` gains customers, mandates, and `capabilities().saved_instruments`
  flips to true.** `Mollie::create_customer` registers a `cst_…`.
  `Mollie::charge_first` opens a payment with `sequenceType: first`, which
  establishes a mandate as a side effect — Mollie's own documented example
  carries the new `mdt_…` on the payment while it is still `open`, before the
  payer has done anything. `Mollie::mandates` and `Mollie::mandate` list a
  customer's mandates and read one back, each as a `mollie::Mandate` whose
  `id` is a `kasapay_core::InstrumentId` — the same kind Stripe's `pm_…` and
  iyzico's `cardToken` are — and whose `customer` is the `mollie::CustomerId`
  it is hung on.

  `Mollie::charge_with_mandate` is the charge itself: `sequenceType:
  recurring` with the `mandateId`, and `ChargeRequest::customer` carrying the
  customer, the other half of the name the same way iyzico's `cardUserKey`
  sits beside its `cardToken`. **It answers no redirect.** Mollie's own
  documented example for a recurring payment carries `redirectUrl: null` and
  no `checkout` link in `_links` — only `changePaymentState`, which this
  crate does not read — so `Charge::next_action` is `None` here, where
  `Provider::charge` would carry a `NextAction::Redirect`.

  `mollie::MandateStatus` is `Valid`, `Pending`, `Invalid` or `Other` for
  whatever Mollie starts sending later. Their own enum has no fourth value
  for "revoked": a mandate that was revoked and one that never signed both
  read `invalid`, and nothing in the status alone tells the two apart.
  `charge_with_mandate` does not check this before sending — a status read
  moments earlier can go stale before the charge lands, so Mollie's own
  answer is the one that has not, and a pending or invalid mandate is
  Mollie's ordinary refusal rather than a fault in this crate.

  `scripts/fetch_mollie.py`'s `KEEP` grows to eight paths — `/v2/customers`
  and both mandate paths join the five payment ones — so the same fetch,
  meta and `--write-document` flow above now covers what this adds too.

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

- **`kasapay_iyzico::terminal`, iyzico's Terminal API: the OAuth2 login and
  four of the fourteen `terminal-host` operations.** A cash register driving a
  physical POS device over the counter — `terminal::Client::pay`, `payment`,
  `refund` and `void`, the VUK 509 payment lifecycle. No shopper, no browser,
  no callback: a call returns when somebody has presented a card to the
  terminal named by `deviceUniqueId`, which is why `Config::DEFAULT_TIMEOUT`
  here is ninety seconds where the rest of the crate waits thirty.

  **The first module in this crate with an authentication of its own.**
  `terminal::Login` performs iyzico's OAuth2 flow — `/authorize` for a
  single-use auth code, `/token` for a bearer token, `/token/refresh` to renew
  one — and `terminal::Client` sends the token it is given. Those three paths
  begin `/in-store/oauth2/`, and they are the Terminal API's rather than
  In-Store's; `specs/README.md` and the module documentation both say how that
  was established. A caller supplies four secrets, in two pairs: the
  `client_id` and `client_secret` iyzico issues, and the `username` and
  `password` of the till.

  **The client does not refresh the token by itself, and that is the design.**
  It reports an expired one as `ErrorKind::Auth` — iyzico's `100311` — rather
  than fetching another and replaying the request, because replaying
  `Client::pay` means putting a terminal back into its card-reading state for a
  sale that may already have been taken. `Token::expires_within` and
  `Client::set_access_token` are what a caller renews with; the latter takes
  `&self`, so a background task and a till can hold the same client.

  The other ten operations are not here. Nine are VUK 507, which iyzico says
  outright must not be used in the same integration as VUK 509 — a fiscal cash
  register with sale line items, VAT groups and the buyer's tax number, not a
  fallback within this. The tenth is VUK 509's end of day, which settles rather
  than pays. The module documentation lists all ten with the reason for each.

  **iyzico documents no response signature for any of the fourteen**, in either
  language, so nothing here is verified — the same position `iyzilink` and
  `subscription` are in. Terminal API refusals do not use the classic error
  codes either: they carry an `errorGroup` beside a `380`-series code, and this
  reads both, falling back to the classic table for the one group that forwards
  it.

  A transaction in anything but `TRY`, `USD` or `EUR` cannot be built. And
  `terminal::Payment` is deliberately not a `Charge`: iyzico does not echo
  `salesType` on the answer, so a successful sale and a successful
  pre-authorisation are the same bytes, and `Captured` against `Authorized`
  would be a guess about money that is either taken or only held.

  None of it has been checked against a live account — there is no Terminal API
  sandbox without a merchant agreement and a Pavo device. The module
  documentation names the three things worth checking first.

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
- `classic`, iyzico's other API — the hosted checkout form, a payment read back
  by its id, refunds, cancel, stored cards and charging one, BIN lookup. Ten
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

- **`kasapay_core::Instrument`, and `Provider::instruments` — listing a
  customer's saved cards is now one call, the same shape at every adapter.**
  #61: three adapters already held saved instruments, and comparing their
  signatures found one thing the same at all of them — name the payer, get a
  list back — and two that were not: forgetting one needs iyzico's
  `cardUserKey` *and* its token where Stripe's needs only the instrument, and
  charging one takes a buyer and a basket at iyzico, an `off_session` flag at
  Stripe, a `sequenceType` at Mollie. Only the first goes on the trait.

  `Instrument` is `id`, an `InstrumentId`; `label`, `Option<Box<str>>`,
  something to show a person choosing between saved instruments; and `raw`,
  the provider's own answer. It does not assume a card on purpose — Mollie's
  saved instrument is a mandate against a bank account, not one — so there is
  no brand, no expiry, no last four here. The richer, provider-specific type
  stays exactly where it was: `classic::StoredCard`, `saved::StoredCard`,
  `Mandate`, each still answering everything `Instrument` leaves out.

  `iyzico::classic::Client` answers its own `/cardstorage/cards`, `customer`
  being the `cardUserKey`, and keeps the per-card JSON so `Instrument::raw` is
  that card's own object rather than empty. `Stripe` delegates to
  `Stripe::stored_cards`, the brand and last four becoming the label; Stripe
  drops the original response bytes by the time this crate sees them, so
  `Instrument::raw` is reconstructed the same way `Charge::raw` already is for
  a PaymentIntent. `Mollie` delegates to `Mollie::mandates`, with `method` —
  `directdebit`, `creditcard`, `paypal` — as the label, because this crate
  models nothing more specific for a mandate. `iyzico::in_store::Client` and
  `PayTr` both answer `ErrorKind::Unsupported`, for different reasons that
  land on the same result: In-Store has no vault at all, a payer taps a card
  at a counter; PayTR has one — its hosted form stores a card against a
  `utoken` — but nothing here can sign a request against it, the same reason
  `Capabilities::saved_instruments` was already false for it.

  `Capabilities::saved_instruments`'s documentation now says plainly that it
  describes *charging*, not *listing*: every adapter answers
  `Provider::instruments` regardless of this flag, and the two need not agree
  — PayTR's do not, because a vault existing and this crate being able to
  reach it are different facts.

### Fixed

- **A payout line printed the account it was paying.** `mass::Recipient`
  derived `Debug`, so an IBAN and a national identity number went wherever a
  `NewPayout` was printed — and a payout is exactly the thing somebody debugs
  by printing it. It shows which kind of recipient it is and the last four
  characters now: enough to tell two lines apart, not enough to send money.
  Same defect as `Raw`'s, in a module that landed before that was found.

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
