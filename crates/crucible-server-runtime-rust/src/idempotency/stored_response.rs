use serde_json::{Value, json};

/// What a completed attempt leaves for its retries: the operation's
/// successful output, in its JSON form, labelled with the API version it
/// was written at. A retry gets it encoded afresh, by the protocol and at
/// the API version the retry uses, with the retry's own request id, so the
/// output reads the same way whichever way the retry came. Opaque to a
/// store, which keeps the text as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredResponse {
    text: String,
}

impl StoredResponse {
    /// A response a store kept, as [`StoredResponse::as_str`] gave it.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// `output` as the service's current API version `version` wrote it.
    pub(crate) fn wrap(version: &str, output: Value) -> Self {
        Self::new(json!({ "apiVersion": version, "output": output }).to_string())
    }

    /// The output, when it was written at `version`. A response written
    /// before a deploy that changed the API version, or one that does not
    /// read, is `None`: replaying it could answer in a shape the current
    /// model no longer has.
    pub(crate) fn unwrap(&self, version: &str) -> Option<Value> {
        let mut stored: Value = serde_json::from_str(&self.text).ok()?;
        if stored.get("apiVersion")?.as_str()? != version {
            return None;
        }
        Some(stored.get_mut("output")?.take())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::StoredResponse;

    #[test]
    fn an_output_replays_only_at_the_version_it_was_written_at() {
        let stored = StoredResponse::wrap("v2", json!({"city": {"id": "c-1"}}));
        let kept = StoredResponse::new(stored.as_str());
        assert_eq!(kept.unwrap("v2"), Some(json!({"city": {"id": "c-1"}})));
        assert_eq!(kept.unwrap("v3"), None);
        assert_eq!(StoredResponse::new("not json").unwrap("v2"), None);
        assert_eq!(StoredResponse::new(r#"{"city":{}}"#).unwrap("v2"), None);
    }
}
