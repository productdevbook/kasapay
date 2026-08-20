# kasapay

One payment API over any payment provider.

This file settles which word to use where several are in circulation. It does
not define behaviour: each entry names the type or module that does, and that
one is the authority.

## The people

**Payer**:
The person paying. The role, in prose.
_Avoid_: shopper, user

**Buyer**:
The payer's details a provider demands before it will take a payment. The
type, `kasapay_core::Buyer` — never the role.
_Avoid_: customer, purchaser

**Customer**:
A provider's own opaque handle for a payer it holds on file — Stripe's
`customer`, Mollie's `customerId`, iyzico In-Store's `userId`. Never a person,
and never a record kasapay keeps. See `ChargeRequest::customer`.
_Avoid_: account, user id

**Cardholder**:
The payer, where what is being said is true of cards and not of other
instruments.

## The acts

**Charge**:
Taking money, and the record of having asked. `Provider::charge` starts one;
`kasapay_core::Charge` is how a provider currently sees it. A `Charge` is not
a completed payment.
_Avoid_: transaction, sale

**Payment**:
The object a provider names with a `PaymentId`. What kasapay asked for is a
charge; what the provider holds is a payment.

**Hold**:
Money an authorisation reserves and has not taken.
_Avoid_: blocked funds, pending amount

**Capture**:
Taking money a hold reserved. It has no inverse — captured money is refunded,
never un-captured.

**Release**:
Giving up a hold that will never be captured. `Provider::cancel` is the call;
`kasapay_core::Release` is what comes back.
_Avoid_: void (PayPal's word), reversal (iyzico's), cancellation

**Refund**:
Sending captured money back. Its own object with its own life, never a status
the payment arrives in.

## The code

**Provider**:
A payment service — Stripe, iyzico, Mollie, PayPal, PayTR — and the trait an
adapter implements for one.

**Adapter**:
The crate that implements `Provider` for one provider. A provider is who takes
the money; an adapter is the code that asks them to.

**Order reference**:
The caller's own name for an order, held by `OrderRef`. Not a `PaymentId`, even
where the two carry the same characters.
_Avoid_: order id, merchant reference

## Spelling

Prose uses **-ise**: authorise, authorisation, cancelling.

**-ize** only where it names something somebody else spelled that way — the
HTTP `Authorization` header, PayPal's `authorization` object, iyzico's Authorize
service, or a Rust identifier such as `Status::Authorized`, whose variants carry
the provider's spelling rather than this one.
