//! The provider's own answer, kept whole.

use std::fmt;

/// What a provider actually sent, for everything kasapay does not model.
///
/// Every [`Charge`](crate::Charge) carries one. It is the escape hatch: a
/// provider will always have a field somebody needs and this crate has not
/// heard of, and the alternative to keeping the body is losing it.
///
/// # Why this is not a `serde_json::Value`
///
/// It used to be. That put serde_json in the public API of every provider
/// adapter, including ones written outside this workspace — the day serde_json
/// goes to 2.0, every one of them breaks for a reason its author did not cause.
/// A provider that answers XML, or a form body, or a protobuf had nowhere to
/// put it either.
///
/// So the body is held as text and parsed on request. [`Raw::json`] is the one
/// place serde_json appears, and a provider that has no JSON can still build a
/// `Raw` with [`Raw::from_text`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Raw(Box<str>);

impl Raw {
    /// Keeps a response body exactly as it arrived.
    pub fn from_text(body: impl Into<Box<str>>) -> Self {
        Self(body.into())
    }

    /// Keeps a body a provider has already parsed.
    #[must_use]
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self(value.to_string().into_boxed_str())
    }

    /// The body as it arrived.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether there is a body at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Parses the body as JSON.
    ///
    /// Returns `None` for a body that is not JSON, including an empty one.
    /// This is the only method that names serde_json; reach for it when a
    /// field kasapay does not model is worth reading.
    #[must_use]
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_str(&self.0).ok()
    }

    /// Reads one string out of a JSON body by [RFC 6901] pointer.
    ///
    /// `raw.text_at("/transactionDetail/currencyCode")`. Returns `None` if the
    /// body is not JSON, the pointer finds nothing, or what it finds is not a
    /// string. Costs a parse, so [`Raw::json`] is better for reading several.
    ///
    /// [RFC 6901]: https://datatracker.ietf.org/doc/html/rfc6901
    #[must_use]
    pub fn text_at(&self, pointer: &str) -> Option<String> {
        self.json()?
            .pointer(pointer)?
            .as_str()
            .map(ToOwned::to_owned)
    }
}

impl fmt::Display for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&serde_json::Value> for Raw {
    fn from(value: &serde_json::Value) -> Self {
        Self::from_json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::Raw;

    #[test]
    fn a_json_body_is_readable_by_pointer_and_as_a_value() {
        let raw = Raw::from_text(r#"{"transactionDetail":{"currencyCode":"TRY"}}"#);
        assert_eq!(
            raw.text_at("/transactionDetail/currencyCode").as_deref(),
            Some("TRY")
        );
        assert!(raw.json().is_some());
        assert!(!raw.is_empty());
    }

    #[test]
    fn a_body_that_is_not_json_is_still_kept() {
        let raw = Raw::from_text("<result><status>ok</status></result>");
        assert!(raw.json().is_none());
        assert!(raw.text_at("/status").is_none());
        assert!(raw.as_str().starts_with("<result>"));
    }

    #[test]
    fn a_pointer_at_something_that_is_not_a_string_finds_nothing() {
        let raw = Raw::from_text(r#"{"amount":1499,"nested":{"a":1}}"#);
        assert!(raw.text_at("/amount").is_none());
        assert!(raw.text_at("/nested").is_none());
        assert!(raw.text_at("/missing").is_none());
    }

    #[test]
    fn an_empty_body_is_empty_and_not_json() {
        let raw = Raw::default();
        assert!(raw.is_empty());
        assert!(raw.json().is_none());
    }
}
