//! Documentation-only annotations (`@deprecated`, `@since`, `@unstable`,
//! `@externalDocs`), settled once so no generator re-reads them raw.
//!
//! `@deprecated` is the one a consumer must see, so every native generator
//! renders it; the others are documentation and may be ignored by a target
//! with no natural place for them.

use std::collections::BTreeMap;

/// `@deprecated`: both the message and the version are optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Deprecation {
    pub message: Option<String>,
    pub since: Option<String>,
}

impl Deprecation {
    /// The message when it says something: an empty one counts as absent.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref().filter(|m| !m.trim().is_empty())
    }

    /// One sentence for targets that can only say it in prose; the text is
    /// the model's, so a generator escapes it for its own comment syntax.
    pub fn sentence(&self) -> String {
        let mut text = String::from("Deprecated");
        if let Some(since) = &self.since {
            text.push_str(&format!(" since {since}"));
        }
        match self.message() {
            Some(message) => format!("{text}: {message}"),
            None => format!("{text}."),
        }
    }

    /// The message alone, or the generic sentence when the model gave none:
    /// what a native attribute's message argument carries.
    pub fn note(&self) -> String {
        match (self.message(), &self.since) {
            (Some(message), Some(since)) => format!("{message} (since {since})"),
            (Some(message), None) => message.to_owned(),
            (None, Some(since)) => format!("Deprecated since {since}."),
            (None, None) => "Deprecated.".to_owned(),
        }
    }
}

/// The documentation-only traits on one shape, member, operation or service.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DocNotes {
    pub deprecated: Option<Deprecation>,
    /// `@since`: the version that introduced the element.
    pub since: Option<String>,
    /// `@unstable`: the element may change incompatibly.
    pub unstable: bool,
    /// `@externalDocs`, sorted by name.
    pub external_docs: BTreeMap<String, String>,
}

impl DocNotes {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Short prose lines for generators that emit doc notes: "Unstable." and
    /// "Since X.". `@deprecated` is separate since each target has a native form.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.unstable {
            lines.push("Unstable.".to_owned());
        }
        if let Some(since) = &self.since {
            lines.push(format!("Since {since}."));
        }
        lines
    }

    /// `docs` followed by [`Self::lines`] and a "Deprecated" sentence, as
    /// paragraphs: the fallback for a target with no native form. `None` when
    /// there is nothing to say.
    pub fn prose(&self, docs: Option<&str>) -> Option<String> {
        let mut paragraphs: Vec<String> = docs
            .map(str::trim)
            .filter(|docs| !docs.is_empty())
            .map(str::to_owned)
            .into_iter()
            .collect();
        paragraphs.extend(self.lines());
        paragraphs.extend(self.deprecated.as_ref().map(Deprecation::sentence));
        (!paragraphs.is_empty()).then(|| paragraphs.join("\n\n"))
    }
}
