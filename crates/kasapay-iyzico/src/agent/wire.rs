//! The bytes `agent::Client` sends and reads.

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct AuthRequest<'a> {
    pub(crate) agent_id: &'a str,
    pub(crate) user_id: &'a str,
}

#[derive(Deserialize, Default)]
pub(crate) struct AuthResponse {
    pub(crate) session_key: Option<String>,
    pub(crate) expired_date: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) company_code: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) user_unique_id: Option<String>,
    pub(crate) is_okc_inquiry: Option<bool>,
}

#[derive(Serialize)]
pub(crate) struct LogoutRequest<'a> {
    pub(crate) session_key: &'a str,
}

/// Paynet's own `{"object_name", "code", "message"}` shape, on a 400.
#[derive(Deserialize, Default)]
pub(crate) struct ErrorResponse {
    pub(crate) code: Option<i64>,
    pub(crate) message: Option<String>,
}
