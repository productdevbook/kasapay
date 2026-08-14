//! PayPOS's `Authorize` service — the session a [`crate::softpos`] call needs.
//!
//! Two operations, both of iyzico's `agent` group:
//!
//! - [`Client::get_auth_key`] — `POST /v1/agent/get_auth_key`
//! - [`Client::logout`] — `POST /v1/agent/logout`
//!
//! # Which of the three ways of declaring nothing this is
//!
//! `specs/README.md` counts nine iyzico operations whose fragment declares no
//! `securityScheme` and no `Authorization` **parameter shaped like the classic
//! API's** — five In-Store, one Terminal API login filed under In-Store, and
//! `agent`'s and `softpos`'s five between them. For softpos specifically,
//! three possibilities were worth telling apart before writing a client: the
//! fragments simply omit the classic signature that in fact applies; the
//! endpoints authenticate some other way the fragments do not carry; or they
//! are genuinely open.
//!
//! **It is the second.** Both languages' pages state it in prose, not just in
//! the parameter list: Create Session's own paragraph says a request "must
//! include the dealer-specific secret key in the request header", and every
//! `/v1/softpos/*` page repeats "a mobile session token (`session_key`) must
//! be used." The fragments *do* carry the mechanism — `Authorization` and
//! `PaynetMobile` here, `Session-Key` on [`crate::softpos`] — as a plain
//! header parameter each, just not one iyzico's own tooling recognises as a
//! `securityScheme` and not shaped like the classic API's `Authorization`
//! parameter, which is why `specs/README.md`'s count reads it as declaring
//! neither. `crates/kasapay-iyzico/src/onboarding/mod.rs` reached the same
//! conclusion first, while explaining why sub-merchant onboarding was not the
//! place to add it: "closer to [`in_store`](crate::in_store)'s plain headers
//! than to anything here."
//!
//! # This is not iyzico's own API
//!
//! Every fragment behind `/v1/agent/*` and `/v1/softpos/*` — in both
//! languages, all five operations — titles itself `"PayPOS (Paynet) API"` and
//! declares `https://api.paynet.com.tr` (production) and
//! `https://pts-api.paynet.com.tr` (sandbox) as its servers. That is not
//! `api.iyzipay.com`. The integration overview page says the same thing twice
//! more, outside any OpenAPI fragment: its glossary calls the two products
//! `Paynet API` and PayPos, as if naming two different things, its warning
//! box says obtaining a secret key means registering an IP address
//! "in the Paynet panel", and it carries its own prose `BaseUrl` section
//! naming the identical pair of hosts. iyzico documents PayPOS on its own
//! site because it resells it, not because it runs it — and a client for it
//! that quietly pointed at `api.iyzipay.com` would be a client for the wrong
//! service.
//!
//! `scripts/merge_iyzico.py`'s `merge()` always writes iyzico's own `servers`
//! pair onto the document it assembles rather than a fragment's own one —
//! its `base_path()` helper only recognises a server that starts with
//! iyzico's host — so `specs/iyzico/agent/latest.yaml` and
//! `specs/iyzico/softpos/latest.yaml` both show `api.iyzipay.com` at the top
//! level despite every fragment inside them declaring otherwise. Read
//! `x-iyzico-source` and the page it points at rather than trusting the
//! merged document's `servers` block for this group.
//!
//! # No worked example anywhere
//!
//! Neither language's page for either operation here, nor for any of
//! [`crate::softpos`]'s three, carries a curl example or a request/response
//! body with real values — only the schema. `Session` and the request types
//! are built from the field names PayPOS documents and nothing more, and the
//! `tests/agent.rs` fixtures are stand-ins the same way
//! [`mass`](crate::mass)'s `authorize`, `cancel`, `balance` and single-item
//! read are, for the same reason: PayPOS leaves them undemonstrated. No live
//! PayPOS account was available to check any of it against.
//!
//! # What a success looks like is not documented either
//!
//! Both operations answer the identical shape — `object_name`, `code`,
//! `message`, plus the data on a 200 — whether iyzico calls it a success or a
//! failure; the only schema difference between the 200 and the 400 is which
//! extra fields ride along. Nothing says what `code` holds on either path, so
//! this module reads HTTP status alone: 2xx is answered, anything else is
//! [`Error`](kasapay_core::Error). The body's `code` still travels on
//! [`Error::code`](kasapay_core::Error::code) when a call is refused, unread
//! and unverified.
//!
//! # Example
//!
//! ```no_run
//! use kasapay_iyzico::agent;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let config = agent::Config::sandbox();
//! let dealer = agent::Client::new(config, agent::Credentials::new("sck_test_xxx"))?;
//!
//! let session = dealer.get_auth_key("agent-1", "till-7").await?;
//! // session.session_key is what kasapay_iyzico::softpos::Client::new wants.
//! dealer.logout(&session.session_key).await?;
//! # Ok(())
//! # }
//! ```

mod client;
mod wire;

#[doc(inline)]
pub use crate::agent::client::{Client, Config, Credentials, Session};
