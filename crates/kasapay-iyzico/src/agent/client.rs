//! The agent client: `PayPOS`'s `Authorize` service, and its logout.

use std::sync::Arc;
use std::time::Duration;

use kasapay_core::{Error, ErrorKind, ProviderId, Raw, Secret};
use url::Url;

use crate::agent::wire;

const PROVIDER: ProviderId = ProviderId::IYZICO;

/// The header `PayPOS` asks every `/v1/agent/*` call to carry.
///
/// Documented as "Indicates a mobile request", with `2` as the only value
/// either language's example ever shows. Nothing on either page names another,
/// so this is sent as a constant rather than something a caller supplies.
const PAYNET_MOBILE: &str = "2";

/// Where the client points, and how long it waits.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: Url,
    timeout: Duration,
}

impl Config {
    /// How long a request waits before it is given up on.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// The production base, `https://api.paynet.com.tr`.
    ///
    /// **Not iyzico's own host.** Every fragment behind `/v1/agent/*` and
    /// `/v1/softpos/*` — in both languages — declares `info.title: "PayPOS
    /// (Paynet) API"` and this pair of servers, not `api.iyzipay.com` /
    /// `sandbox-api.iyzipay.com`. `scripts/merge_iyzico.py` does not carry a
    /// fragment's own `servers` into the merged document — it always writes
    /// iyzico's own pair — so `specs/iyzico/agent/latest.yaml` and
    /// `specs/iyzico/softpos/latest.yaml` both show the wrong host at the top
    /// level. See the module docs for the rest of the evidence, including the
    /// integration overview page's own prose `BaseUrl` section, which states
    /// the same two addresses outside any OpenAPI fragment.
    pub const PRODUCTION: &'static str = "https://api.paynet.com.tr/";
    /// The sandbox base, `https://pts-api.paynet.com.tr`. Same caveat.
    pub const SANDBOX: &'static str = "https://pts-api.paynet.com.tr/";

    /// Points at the sandbox.
    #[must_use]
    pub fn sandbox() -> Self {
        Self::new(Self::SANDBOX).unwrap_or_else(|_| unreachable!("the sandbox constant parses"))
    }

    /// Points at production.
    #[must_use]
    pub fn production() -> Self {
        Self::new(Self::PRODUCTION)
            .unwrap_or_else(|_| unreachable!("the production constant parses"))
    }

    /// Points at an arbitrary base — a mock server in tests, mostly.
    ///
    /// The base is joined against, so it must end in a slash.
    pub fn new(base_url: &str) -> Result<Self, url::ParseError> {
        Ok(Self {
            base_url: Url::parse(base_url)?,
            timeout: Self::DEFAULT_TIMEOUT,
        })
    }

    /// Changes how long a request waits, from [`Config::DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn endpoint(&self, path: &str) -> Result<Url, Error> {
        self.base_url.join(path).map_err(|e| {
            Error::new(ErrorKind::InvalidRequest, PROVIDER, "endpoint is not a URL").with_source(e)
        })
    }
}

/// What a dealer authenticates `/v1/agent/*` with.
///
/// Not [`crate::Credentials`], which signs the classic API `IYZWSv2`. This is
/// a single secret, sent as a plain `Authorization: Basic …` header — no
/// `IYZWSv2` signature, no request body or path involved in it at all. The
/// integration overview page's own glossary calls it "a key specifically
/// created for your reseller to send requests to the Authorize service", and
/// its warning box says obtaining one means registering a static server IP
/// "in the Paynet panel" — not iyzico's merchant panel.
#[derive(Debug, Clone)]
pub struct Credentials {
    secret_key: Secret,
}

impl Credentials {
    /// Holds the dealer's secret key.
    ///
    /// # The literal header value is not standard HTTP Basic auth
    ///
    /// iyzico's own example is `Authorization: Basic sck_xxx` — the whole
    /// secret key, prefix and all, written straight after `Basic `, with no
    /// base64 encoding and no `:` separating a user from a password the way
    /// [RFC 7617](https://www.rfc-editor.org/rfc/rfc7617) Basic auth does.
    /// Nothing on either page says otherwise, so [`Client::get_auth_key`] and
    /// [`Client::logout`] send exactly that: `secret_key` reused as `Basic
    /// {secret_key}`. Pass the value with its `sck_` prefix included.
    #[must_use]
    pub fn new(secret_key: impl Into<Secret>) -> Self {
        Self {
            secret_key: secret_key.into(),
        }
    }
}

/// Gets and invalidates a `PayPOS` mobile session key.
///
/// Cloning shares one connection pool. The session key [`Client::get_auth_key`]
/// answers is what [`crate::softpos::Client`] is built with — this module and
/// `softpos` are two halves of one flow, split because `PayPOS` documents them
/// as separate services rather than because they belong to different products.
#[derive(Debug, Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
    credentials: Credentials,
}

impl Client {
    /// Builds a client with its own HTTP connection pool.
    pub fn new(config: Config, credentials: Credentials) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        Ok(Self::with_http(http, config, credentials))
    }

    /// Builds a client over an HTTP client the caller already has.
    ///
    /// The caller's own timeout applies; [`Config::timeout`] is ignored here.
    #[must_use]
    pub fn with_http(http: reqwest::Client, config: Config, credentials: Credentials) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config,
                credentials,
            }),
        }
    }

    /// Generates a mobile session key.
    ///
    /// `PayPOS`'s own warning: "Only servers whose IP addresses are defined by
    /// Paynet can call this service." A request from anywhere else is refused
    /// before it reaches whatever iyzico or Paynet would otherwise have said
    /// about `agent_id` and `user_id` — this crate cannot check that in
    /// advance, so a call from an unregistered address surfaces as whatever
    /// HTTP status Paynet answers with, read the same as any other refusal.
    pub async fn get_auth_key(&self, agent_id: &str, user_id: &str) -> Result<Session, Error> {
        let agent_id = non_empty(agent_id, "agent_id")?;
        let user_id = non_empty(user_id, "user_id")?;
        let body = wire::AuthRequest { agent_id, user_id };
        let (status, bytes) = self.call("v1/agent/get_auth_key", &body).await?;
        if !status.is_success() {
            return Err(refused(status, &bytes, "iyzico refused to start a session"));
        }
        let response: wire::AuthResponse = parse(&bytes)?;
        let session_key = response
            .session_key
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PROVIDER,
                    "the auth answer carried no session_key",
                )
            })?;
        Ok(Session {
            session_key: Secret::new(session_key),
            expired_date: response.expired_date.map(String::into_boxed_str),
            agent_id: response.agent_id.map(String::into_boxed_str),
            company_code: response.company_code.map(String::into_boxed_str),
            user_id: response.user_id.map(String::into_boxed_str),
            user_unique_id: response.user_unique_id.map(String::into_boxed_str),
            is_okc_inquiry: response.is_okc_inquiry,
            raw: raw(&bytes),
        })
    }

    /// Invalidates a mobile session key before it expires on its own.
    pub async fn logout(&self, session_key: &Secret) -> Result<(), Error> {
        let body = wire::LogoutRequest {
            session_key: session_key.expose(),
        };
        let (status, bytes) = self.call("v1/agent/logout", &body).await?;
        if !status.is_success() {
            return Err(refused(status, &bytes, "iyzico refused to end the session"));
        }
        Ok(())
    }

    async fn call<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), Error> {
        let config = &self.inner.config;
        let request = self
            .inner
            .http
            .post(config.endpoint(path)?)
            .header(
                "Authorization",
                format!("Basic {}", self.inner.credentials.secret_key.expose()),
            )
            .header("PaynetMobile", PAYNET_MOBILE)
            .json(body);
        let response = request
            .send()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|e| transport_error(&e).with_source(e))?;
        Ok((status, bytes.to_vec()))
    }
}

/// A mobile session key, and what came back with it.
///
/// Spend it as [`crate::softpos::Client::new`]'s session key, and give it back
/// with [`Client::logout`] when the flow is done rather than letting it run
/// until `expired_date`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Session {
    /// What [`crate::softpos::Client`] authenticates with.
    pub session_key: Secret,
    /// When `PayPOS` says the session stops working, exactly as it was written.
    ///
    /// Text rather than a timestamp: neither page states a format, and
    /// nothing in this dependency tree should guess one for a value this
    /// module cannot demonstrate.
    pub expired_date: Option<Box<str>>,
    /// The dealer id this session belongs to, echoed back.
    pub agent_id: Option<Box<str>>,
    /// `PayPOS`'s id for the parent company.
    pub company_code: Option<Box<str>>,
    /// The user id this session was requested for, echoed back.
    pub user_id: Option<Box<str>>,
    /// `PayPOS`'s own unique id for that user.
    pub user_unique_id: Option<Box<str>>,
    /// Result of an "ÖKC exemption" check — a Turkish fiscal-device waiver.
    /// `PayPOS` documents no more about it than the one field.
    pub is_okc_inquiry: Option<bool>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// A request that never got a usable answer.
fn transport_error(error: &reqwest::Error) -> Error {
    let kind = if error.is_decode() {
        ErrorKind::Malformed
    } else {
        ErrorKind::Transport
    };
    Error::new(kind, PROVIDER, error.to_string())
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "the answer was not the JSON iyzico documents",
        )
        .with_source(e)
    })
}

fn raw(bytes: &[u8]) -> Raw {
    Raw::from_text(String::from_utf8_lossy(bytes).into_owned())
}

/// Reads a refusal off whatever body came back, or from the status alone.
///
/// # No error-code registry for this to consult
///
/// The classic API's numeric codes — [`crate::errors::kind_for`] — describe a
/// different service and are not read here. `PayPOS`'s own `ErrorResponse` names
/// `code` and `message` with no meaning documented for either, and no page
/// under `paypos-app2app` lists what a `code` value means the way iyzico's
/// [error-codes page](https://docs.iyzico.com/en/add-ons/error-codes) does for
/// the classic API. So only the HTTP status is read for `ErrorKind`, and the
/// body's `code` travels on [`Error::code`](kasapay_core::Error::code)
/// unread — not verified against a live account either way.
fn refused(status: reqwest::StatusCode, body: &[u8], fallback: &str) -> Error {
    let parsed: Option<wire::ErrorResponse> = serde_json::from_slice(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|e| e.message.clone())
        .unwrap_or_else(|| format!("{fallback} (HTTP {status})"));
    let error = Error::new(kind_for_status(status), PROVIDER, message);
    match parsed.and_then(|e| e.code) {
        Some(code) => error.with_code(code.to_string()),
        None => error,
    }
}

fn kind_for_status(status: reqwest::StatusCode) -> ErrorKind {
    match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        404 => ErrorKind::NotFound,
        429 => ErrorKind::RateLimited,
        400 | 422 => ErrorKind::InvalidRequest,
        _ => ErrorKind::Provider,
    }
}

/// Refuses a blank identifier before it opens a socket.
fn non_empty<'a>(value: &'a str, field: &'static str) -> Result<&'a str, Error> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            PROVIDER,
            format!("PayPOS requires {field}, and none was given"),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{Config, kind_for_status, non_empty};
    use kasapay_core::ErrorKind;

    #[test]
    fn the_paynet_bases_parse_and_join() {
        for config in [Config::sandbox(), Config::production()] {
            let url = config
                .endpoint("v1/agent/get_auth_key")
                .expect("the path joins");
            assert_eq!(url.path(), "/v1/agent/get_auth_key");
        }
        assert_eq!(Config::PRODUCTION, "https://api.paynet.com.tr/");
        assert_eq!(Config::SANDBOX, "https://pts-api.paynet.com.tr/");
    }

    #[test]
    fn a_blank_identifier_is_refused() {
        assert!(non_empty("", "agent_id").is_err());
        assert!(non_empty("   ", "agent_id").is_err());
        assert!(non_empty("dealer-1", "agent_id").is_ok());
    }

    #[test]
    fn a_refusal_with_no_body_is_read_from_its_status() {
        let kind = |code| kind_for_status(reqwest::StatusCode::from_u16(code).expect("a status"));
        assert_eq!(kind(401), ErrorKind::Auth);
        assert_eq!(kind(404), ErrorKind::NotFound);
        assert_eq!(kind(400), ErrorKind::InvalidRequest);
        assert_eq!(kind(500), ErrorKind::Provider);
    }
}
