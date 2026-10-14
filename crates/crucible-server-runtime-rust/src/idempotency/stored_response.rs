use serde_json::{Value, json};

/// What a finished attempt leaves for its retries. Usually the operation's
/// successful output, in its JSON form, labelled with the API version it
/// was written at, and, when `Permissions` covered the request, the handler
/// checks the Authorizer allowed (`authorization`), which a replay asks for
/// again. A record from before this field replays with the operation's
/// declared checks alone. A retry gets it encoded afresh, by the protocol and at
/// the API version the retry uses, with the retry's own request id, so the
/// output reads the same way whichever way the retry came.
///
/// Otherwise a note that there is nothing to replay: the attempt failed
/// past its point of no return, or succeeded with a response too large to
/// keep, and a retry is refused saying which. Opaque to a store, which keeps
/// the text as it is: the output in plaintext, `@sensitive` members
/// included.
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

    /// `output` as the service's current API version `version` wrote it,
    /// with the handler checks it was allowed, if `Permissions` recorded
    /// them.
    pub(crate) fn wrap(version: &str, output: Value, authorization: Option<Value>) -> Self {
        let mut stored = json!({ "apiVersion": version, "output": output });
        if let Some(authorization) = authorization {
            stored["authorization"] = authorization;
        }
        Self::new(stored.to_string())
    }

    /// The note that the attempt failed after its point of no return, so
    /// whether it took effect is unknown and a retry must not run it again.
    pub(crate) fn failed(version: &str) -> Self {
        Self::new(json!({ "apiVersion": version, "outcome": "failed" }).to_string())
    }

    /// The note that the attempt succeeded but its response was not kept.
    pub(crate) fn unkept(version: &str) -> Self {
        Self::new(json!({ "apiVersion": version, "outcome": "unkept" }).to_string())
    }

    /// What the record says, for a retry at the service's current API
    /// version `version`.
    pub(crate) fn read(&self, version: &str) -> Recorded {
        self.read_output(version).unwrap_or(Recorded::Unreadable)
    }

    fn read_output(&self, version: &str) -> Option<Recorded> {
        let mut stored: Value = serde_json::from_str(&self.text).ok()?;
        match stored.get("outcome").and_then(Value::as_str) {
            Some("failed") => return Some(Recorded::Failed),
            Some("unkept") => return Some(Recorded::Unkept),
            Some(_) => return None,
            None => {}
        }
        // An output written before a deploy that changed the API version
        // could answer in a shape the current model no longer has.
        if stored.get("apiVersion")?.as_str()? != version {
            return None;
        }
        let authorization = stored.get_mut("authorization").map(Value::take);
        Some(Recorded::Output {
            output: stored.get_mut("output")?.take(),
            authorization,
        })
    }
}

/// What a [`StoredResponse`] holds for a retry.
#[derive(Debug, PartialEq)]
pub(crate) enum Recorded {
    /// The success's output, and its recorded handler checks (`None` for a
    /// record without them).
    Output {
        output: Value,
        authorization: Option<Value>,
    },
    /// The attempt failed past its point of no return.
    Failed,
    /// The attempt succeeded, but its response was not kept.
    Unkept,
    /// An output from another API version, or a record that does not read.
    Unreadable,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Recorded, StoredResponse};

    #[test]
    fn an_output_replays_only_at_the_version_it_was_written_at() {
        let stored = StoredResponse::wrap("v2", json!({"city": {"id": "c-1"}}), None);
        let kept = StoredResponse::new(stored.as_str());
        assert_eq!(
            kept.read("v2"),
            Recorded::Output {
                output: json!({"city": {"id": "c-1"}}),
                authorization: None
            }
        );
        let checked = StoredResponse::wrap("v2", json!({}), Some(json!([])));
        assert_eq!(
            checked.read("v2"),
            Recorded::Output {
                output: json!({}),
                authorization: Some(json!([]))
            }
        );
        assert_eq!(kept.read("v3"), Recorded::Unreadable);
        assert_eq!(
            StoredResponse::new("not json").read("v2"),
            Recorded::Unreadable
        );
        assert_eq!(
            StoredResponse::new(r#"{"city":{}}"#).read("v2"),
            Recorded::Unreadable
        );
    }

    #[test]
    fn a_note_reads_at_any_version() {
        assert_eq!(StoredResponse::failed("v1").read("v2"), Recorded::Failed);
        assert_eq!(StoredResponse::unkept("v1").read("v2"), Recorded::Unkept);
        let unknown = StoredResponse::new(r#"{"apiVersion":"v2","outcome":"later"}"#);
        assert_eq!(unknown.read("v2"), Recorded::Unreadable);
    }
}
