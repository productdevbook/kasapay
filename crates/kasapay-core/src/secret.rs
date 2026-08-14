//! A string that does not print itself.

use std::fmt;

/// Holds a credential and keeps it out of `Debug` output and logs.
///
/// The value is still readable through [`Secret::expose`]; this guards against
/// a stray `{:?}` on a config struct, not against a determined caller.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a credential.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Reads the credential, for the one place that has to send it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    #[test]
    fn debug_output_omits_the_value() {
        let secret = Secret::new("sandbox-key-value");
        assert!(!format!("{secret:?}").contains("sandbox-key-value"));
        assert_eq!(secret.expose(), "sandbox-key-value");
    }
}
