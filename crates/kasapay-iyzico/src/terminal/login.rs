//! The OAuth2 login, which is the Terminal API's authentication.
//!
//! Two calls to get in and one to stay in. `/authorize` proves who the user is
//! and answers a short-lived, single-use auth code; `/token` trades that code
//! for a bearer token; `/token/refresh` trades the refresh token for a new
//! bearer token when the first one runs out.
//!
//! All three sit at `/in-store/oauth2/…`, which is not a mistake and not
//! In-Store's login — see the module docs for why the path says one thing and
//! the documentation another.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kasapay_core::{Error, ErrorKind, Raw, Secret};
use url::Url;

use crate::terminal::client::Config;
use crate::terminal::wire;
use crate::terminal::{PROVIDER, transport_error};

/// The one scope iyzico documents, and the only value its `enum` allows.
const SCOPE: &str = "iyzipayApiGateway";
/// The one response type iyzico documents.
const RESPONSE_TYPE: &str = "code";

/// What a merchant proves themselves with to get a token.
///
/// Four secrets, in two pairs that do different jobs. The `client_id` and
/// `client_secret` identify the integration and are issued by iyzico; the
/// `username` and `password` identify the person or till the token will act
/// as. `/authorize` wants all four, and `/token` wants the first pair again as
/// HTTP Basic.
///
/// This is not [`crate::Credentials`], which holds the API key and secret key
/// the classic API signs requests with. The Terminal API uses neither of those.
#[derive(Debug, Clone)]
pub struct Credentials {
    client_id: Secret,
    client_secret: Secret,
    username: Secret,
    password: Secret,
}

impl Credentials {
    /// Holds the four secrets a login needs.
    #[must_use]
    pub fn new(
        client_id: impl Into<Secret>,
        client_secret: impl Into<Secret>,
        username: impl Into<Secret>,
        password: impl Into<Secret>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            username: username.into(),
            password: password.into(),
        }
    }
}

/// A single-use code, on the way to a token.
///
/// iyzico's own example is ten minutes wide — issued at `14:05:49`, expired at
/// `14:15:49` — and the overview page calls it "short-lived and single-use".
/// [`Login::log_in`] spends one immediately and never hands one back.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuthCode {
    /// The code itself.
    pub code: Secret,
    /// When iyzico says it was issued, exactly as they wrote it.
    ///
    /// Text rather than a timestamp: their example is ISO-8601 with a
    /// nanosecond fraction and a `+03:00` offset, and nothing in this
    /// dependency tree parses that. A caller who needs it can.
    pub issued_at: Option<Box<str>>,
    /// When iyzico says it stops working, exactly as they wrote it.
    pub expired_at: Option<Box<str>>,
    /// iyzico's own answer, untouched.
    pub raw: Raw,
}

/// A bearer token, and when it stops working.
///
/// The `access_token` goes on every Terminal Host call as
/// `Authorization: Bearer …`; the `refresh_token` is stored and buys a new one
/// when this expires.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Token {
    /// What [`Client::new`](crate::terminal::Client::new) authenticates with.
    pub access_token: Secret,
    /// What [`Login::refresh`] takes to get another.
    pub refresh_token: Option<Secret>,
    /// The scheme the token is used under. iyzico answers `Bearer`.
    pub token_type: Option<Box<str>>,
    /// The scope it was issued for. iyzico documents one.
    pub scope: Option<Box<str>>,
    /// How long it lives from [`Token::obtained_at`].
    ///
    /// `None` where iyzico sent no `expires_in`, which they do not document
    /// happening. iyzico's own example is `7199` — two hours less a second.
    pub expires_in: Option<Duration>,
    /// When this answer was read.
    ///
    /// The clock is the local one, so a machine whose time is wrong will
    /// believe the wrong thing about [`Token::is_expired`]. iyzico sends no
    /// absolute expiry to check it against.
    pub obtained_at: SystemTime,
    /// iyzico's own answer, untouched.
    ///
    /// It holds the token in clear text, because that is what arrived. Do not
    /// log it.
    pub raw: Raw,
}

impl Token {
    /// When the token stops working, where iyzico said how long it lives.
    #[must_use]
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_in
            .and_then(|lifetime| self.obtained_at.checked_add(lifetime))
    }

    /// Whether the token has run out.
    ///
    /// A token iyzico gave no `expires_in` for is never reported expired here:
    /// there is nothing to measure against, and guessing a lifetime would
    /// throw away a working token or keep a dead one.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_within(Duration::ZERO)
    }

    /// Whether the token runs out inside `margin`.
    ///
    /// This is what a caller checks before a sale. A token with four seconds
    /// left is not one to start a payment on: the payer has to present a card
    /// first, and the request will be refused halfway through with the
    /// terminal already waiting.
    #[must_use]
    pub fn expires_within(&self, margin: Duration) -> bool {
        match self.expires_at() {
            Some(expiry) => SystemTime::now()
                .checked_add(margin)
                .is_some_and(|deadline| deadline >= expiry),
            None => false,
        }
    }
}

/// Gets and renews the bearer token the Terminal Host services want.
///
/// Separate from [`Client`](crate::terminal::Client) on purpose: this is the
/// one part of the Terminal API that is not authenticated by a bearer token,
/// and it holds the four secrets a token is bought with rather than the token
/// itself. Cloning shares one connection pool.
#[derive(Debug, Clone)]
pub struct Login {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    http: reqwest::Client,
    config: Config,
    credentials: Credentials,
}

impl Login {
    /// Builds a login with its own HTTP connection pool.
    pub fn new(config: Config, credentials: Credentials) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout())
            .build()?;
        Ok(Self::with_http(http, config, credentials))
    }

    /// Builds a login over an HTTP client the caller already has.
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

    /// Logs in: authorises, then spends the code on a token.
    ///
    /// The two calls iyzico documents as one flow, and the one to reach for.
    /// The auth code is single-use and lives about ten minutes, so there is
    /// nothing a caller does with one between the two steps.
    pub async fn log_in(&self) -> Result<Token, Error> {
        let code = self.authorize().await?;
        self.token(&code).await
    }

    /// Proves who the user is and answers a single-use auth code.
    ///
    /// # `request_timestamp`
    ///
    /// iyzico asks for "the Unix timestamp value of the relevant request" and
    /// does not say in what unit. This sends **seconds**, which is what a Unix
    /// timestamp is; their classic API writes its own `systemTime` in
    /// milliseconds, so the two readings are both defensible and only one can
    /// be right. It has not been checked against a live account.
    pub async fn authorize(&self) -> Result<AuthCode, Error> {
        let credentials = &self.inner.credentials;
        let body = wire::AuthorizeRequest {
            scope: SCOPE,
            client_id: credentials.client_id.expose(),
            client_secret: credentials.client_secret.expose(),
            response_type: RESPONSE_TYPE,
            username: credentials.username.expose(),
            password: credentials.password.expose(),
            request_timestamp: unix_seconds()?,
        };
        let request = self
            .inner
            .http
            .post(self.endpoint("in-store/oauth2/authorize")?)
            .form(&body);

        let (status, bytes) = self.send(request).await?;
        if !status.is_success() {
            return Err(authorize_refused(status, &bytes));
        }
        let response: wire::AuthorizeResponse = parse(&bytes)?;
        let code = response
            .code
            .filter(|code| !code.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PROVIDER,
                    "the authorize answer carried no code",
                )
            })?;
        Ok(AuthCode {
            code: Secret::new(code),
            issued_at: response.issued_at.map(String::into_boxed_str),
            expired_at: response.expired_at.map(String::into_boxed_str),
            raw: raw(&bytes),
        })
    }

    /// Trades an auth code for a bearer token.
    pub async fn token(&self, code: &AuthCode) -> Result<Token, Error> {
        let body = wire::TokenByCode {
            grant_type: "authorization_code",
            code: code.code.expose(),
        };
        self.token_call("in-store/oauth2/token", &body).await
    }

    /// Trades a refresh token for a new bearer token.
    ///
    /// # Two endpoints do this
    ///
    /// This calls `/in-store/oauth2/token/refresh`, the one iyzico gives its
    /// own page section and its own Postman request. `/in-store/oauth2/token`
    /// documents a body of `grant_type=refresh_token` as well, so the same
    /// exchange is documented twice at two addresses, and iyzico nowhere says
    /// which is preferred or whether they differ.
    ///
    /// **The Turkish documentation has no renewal at all.** It documents
    /// `/authorize` and `/token`, and then says of an expired token that a new
    /// login must be performed — [`Login::log_in`] is that, and it is the path
    /// a caller reading only the Turkish pages would take.
    pub async fn refresh(&self, refresh_token: &Secret) -> Result<Token, Error> {
        let body = wire::TokenByRefresh {
            grant_type: "refresh_token",
            refresh_token: refresh_token.expose(),
        };
        self.token_call("in-store/oauth2/token/refresh", &body)
            .await
    }

    /// Both token calls: same Basic auth, same body encoding, same answer.
    async fn token_call<T: serde::Serialize>(&self, path: &str, body: &T) -> Result<Token, Error> {
        let credentials = &self.inner.credentials;
        let request = self
            .inner
            .http
            .post(self.endpoint(path)?)
            .basic_auth(
                credentials.client_id.expose(),
                Some(credentials.client_secret.expose()),
            )
            .form(body);

        let (status, bytes) = self.send(request).await?;
        // Read the clock before parsing, so a slow parse cannot make a token
        // look longer-lived than it is.
        let obtained_at = SystemTime::now();
        if !status.is_success() {
            return Err(token_refused(status, &bytes));
        }
        let response: wire::TokenResponse = parse(&bytes)?;
        let access_token = response
            .access_token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Malformed,
                    PROVIDER,
                    "the token answer carried no access_token",
                )
            })?;
        Ok(Token {
            access_token: Secret::new(access_token),
            refresh_token: response
                .refresh_token
                .filter(|token| !token.is_empty())
                .map(Secret::new),
            token_type: response.token_type.map(String::into_boxed_str),
            scope: response.scope.map(String::into_boxed_str),
            expires_in: response.expires_in.map(Duration::from_secs),
            obtained_at,
            raw: raw(&bytes),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, Error> {
        self.inner.config.endpoint(path)
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), Error> {
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

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|e| {
        Error::new(
            ErrorKind::Malformed,
            PROVIDER,
            "the login answer was not the JSON iyzico documents",
        )
        .with_source(e)
    })
}

fn raw(bytes: &[u8]) -> Raw {
    Raw::from_text(String::from_utf8_lossy(bytes).into_owned())
}

/// `/authorize` answers `errorCode` and `description` and no status field.
fn authorize_refused(status: reqwest::StatusCode, body: &[u8]) -> Error {
    let parsed: Option<wire::AuthorizeError> = serde_json::from_slice(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|e| e.description.clone())
        .unwrap_or_else(|| format!("iyzico refused the authorization (HTTP {status})"));
    let code = parsed.and_then(|e| e.error_code);
    // 401 is the only refusal iyzico documents here, and every reason for it
    // is a credential: the four secrets, or a timestamp too far from theirs.
    let error = Error::new(kind_for_status(status, ErrorKind::Auth), PROVIDER, message);
    match code {
        Some(code) => error.with_code(code),
        None => error,
    }
}

/// Both token services answer `{"error": "…"}` and nothing else.
///
/// No code, no description, no field naming which of the two credentials was
/// wrong — so an error here carries iyzico's one word and no
/// [`Error::code`](kasapay_core::Error::code).
fn token_refused(status: reqwest::StatusCode, body: &[u8]) -> Error {
    let parsed: Option<wire::OAuthError> = serde_json::from_slice(body).ok();
    let message = parsed
        .and_then(|e| e.error)
        .unwrap_or_else(|| format!("iyzico refused the token request (HTTP {status})"));
    // A 400 here is OAuth2's `invalid_grant` and its neighbours: a spent auth
    // code, a refresh token that has been used. That is an authentication
    // failure however it is numbered, and the fix is to log in again.
    Error::new(kind_for_status(status, ErrorKind::Auth), PROVIDER, message)
}

fn kind_for_status(status: reqwest::StatusCode, documented: ErrorKind) -> ErrorKind {
    match status.as_u16() {
        400 | 401 | 403 => documented,
        429 => ErrorKind::RateLimited,
        404 => ErrorKind::NotFound,
        _ => ErrorKind::Provider,
    }
}

/// Now, in whole seconds since the epoch.
fn unix_seconds() -> Result<String, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs().to_string())
        .map_err(|e| {
            Error::new(
                ErrorKind::InvalidRequest,
                PROVIDER,
                "this machine's clock is set before 1970, so no request_timestamp can be sent",
            )
            .with_source(e)
        })
}

#[cfg(test)]
mod tests {
    use super::{Token, unix_seconds};
    use kasapay_core::{Raw, Secret};
    use std::time::{Duration, SystemTime};

    fn token(expires_in: Option<Duration>, obtained_at: SystemTime) -> Token {
        Token {
            access_token: Secret::new("access"),
            refresh_token: None,
            token_type: None,
            scope: None,
            expires_in,
            obtained_at,
            raw: Raw::default(),
        }
    }

    #[test]
    fn a_token_iyzico_gave_no_lifetime_for_is_never_called_expired() {
        // There is nothing to measure against. Guessing a lifetime throws away
        // a working token or keeps a dead one, and both are worse than saying
        // nothing.
        let unknown = token(None, SystemTime::UNIX_EPOCH);
        assert!(!unknown.is_expired());
        assert!(!unknown.expires_within(Duration::from_secs(86_400)));
        assert!(unknown.expires_at().is_none());
    }

    #[test]
    fn a_token_that_has_run_out_says_so() {
        let stale = token(
            Some(Duration::from_secs(7199)),
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        assert!(stale.is_expired());
    }

    #[test]
    fn a_fresh_token_has_not_run_out_but_a_margin_can_still_catch_it() {
        let fresh = token(Some(Duration::from_secs(7199)), SystemTime::now());
        assert!(!fresh.is_expired());
        assert!(!fresh.expires_within(Duration::from_secs(60)));
        // Two hours from now, a two-hour token is gone.
        assert!(fresh.expires_within(Duration::from_secs(7200)));
    }

    #[test]
    fn the_secrets_do_not_print_themselves() {
        // A value no field name could contain: the first draft of this test
        // asserted on "access" and passed on the field name `access_token`
        // rather than on the token.
        let mut held = token(None, SystemTime::now());
        held.access_token = Secret::new("eyJraWQ-not-in-any-log");
        held.refresh_token = Some(Secret::new("eiHg-not-in-any-log-either"));

        let printed = format!("{held:?}");
        assert!(!printed.contains("not-in-any-log"), "{printed}");
        assert!(printed.contains("Secret(***)"), "{printed}");
    }

    #[test]
    fn a_timestamp_is_whole_seconds() {
        let stamp = unix_seconds().expect("this machine's clock is after 1970");
        assert!(stamp.bytes().all(|b| b.is_ascii_digit()));
        // Milliseconds would be thirteen digits until 2286; seconds are ten.
        assert_eq!(stamp.len(), 10);
    }
}
