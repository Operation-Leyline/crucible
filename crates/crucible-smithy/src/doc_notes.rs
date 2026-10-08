//! Documentation-only traits (`@deprecated`, `@since`, `@unstable`,
//! `@externalDocumentation`), settled once so no generator re-reads them raw.
//!
//! `@deprecated` is the one a consumer must see, so every native generator
//! renders it; the others are documentation and may be ignored by a target
//! with no natural place for them.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::diagnostics::Diagnostics;
use crate::shape_id::ShapeId;

/// `smithy.api#deprecated`: both fields are optional in Smithy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocNotes {
    pub deprecated: Option<Deprecation>,
    /// `smithy.api#since`: the version that introduced the element.
    pub since: Option<String>,
    /// `smithy.api#unstable`: the element may change incompatibly.
    pub unstable: bool,
    /// `smithy.api#externalDocumentation`, sorted by name.
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

    /// Lower the traits on `id`, collecting a diagnostic for each malformed one.
    pub(crate) fn lower(
        id: &ShapeId,
        traits: &BTreeMap<ShapeId, Value>,
        diagnostics: &mut Diagnostics,
    ) -> Self {
        let get = |name: &str| traits.get(&ShapeId::new("smithy.api", name));
        let mut notes = Self::default();
        if let Some(value) = get("deprecated") {
            notes.deprecated = deprecation(id, value, diagnostics);
        }
        if let Some(value) = get("since") {
            match value.as_str() {
                Some(since) => notes.since = Some(since.to_owned()),
                None => diagnostics.push(id, "@since must be a string"),
            }
        }
        match get("unstable") {
            None => {}
            Some(Value::Object(fields)) if fields.is_empty() => notes.unstable = true,
            Some(_) => diagnostics.push(id, "@unstable must be an empty structure"),
        }
        if let Some(value) = get("externalDocumentation") {
            match value.as_object() {
                Some(links) => {
                    for (name, url) in links {
                        match url.as_str() {
                            Some(url) => {
                                notes.external_docs.insert(name.clone(), url.to_owned());
                            }
                            None => diagnostics.push(
                                id,
                                format!(
                                    "@externalDocumentation entry {name:?} must be a string URL"
                                ),
                            ),
                        }
                    }
                }
                None => {
                    diagnostics.push(id, "@externalDocumentation must be a map of names to URLs")
                }
            }
        }
        notes
    }

    /// A legacy `@enum` definition marks deprecation with a boolean.
    pub(crate) fn lower_legacy_enum_value(
        id: &ShapeId,
        definition: &Value,
        diagnostics: &mut Diagnostics,
    ) -> Self {
        let mut notes = Self::default();
        match definition.get("deprecated") {
            Some(Value::Bool(true)) => notes.deprecated = Some(Deprecation::default()),
            Some(Value::Bool(false)) | None => {}
            Some(_) => diagnostics.push(id, "@enum definition deprecated must be boolean"),
        }
        notes
    }
}

fn deprecation(id: &ShapeId, value: &Value, diagnostics: &mut Diagnostics) -> Option<Deprecation> {
    let Some(fields) = value.as_object() else {
        diagnostics.push(id, "@deprecated must be a structure");
        return None;
    };
    let mut field = |name: &str| match fields.get(name) {
        None => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(_) => {
            diagnostics.push(id, format!("@deprecated {name} must be a string"));
            None
        }
    };
    Some(Deprecation {
        message: field("message"),
        since: field("since"),
    })
}
