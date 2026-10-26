//! Error documentation semantics, resolved once from lowered, audience-projected IR.

use crate::ShapeId;
use crate::ir::{DocNotes, Service, Shape};

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorDocumentation {
    pub url: Option<String>,
    pub reason_member: Option<String>,
    pub reasons: Vec<ErrorReason>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorReason {
    pub name: String,
    pub value: String,
    pub docs: Option<String>,
    pub notes: DocNotes,
    pub url: Option<String>,
}

/// Validates and normalizes a documentation base, without making a network request.
pub fn base(value: &str) -> Result<String, &'static str> {
    let invalid_char = value.chars().any(|c| {
        c.is_whitespace() || c.is_control() || c == '\\'
            || matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    });
    let bytes = value.as_bytes();
    let invalid_escape = bytes.iter().enumerate().any(|(index, byte)| {
        *byte == b'%'
            && (bytes.get(index + 1).is_none_or(|b| !b.is_ascii_hexdigit())
                || bytes.get(index + 2).is_none_or(|b| !b.is_ascii_hexdigit()))
    });
    let problem = "errorDocs requires an absolute HTTP or HTTPS URL with a host, no credentials, query, fragment, whitespace, controls, backslashes or invalid percent escapes";
    if invalid_char || invalid_escape {
        return Err(problem);
    }
    let parsed = url::Url::parse(value).map_err(|_| problem)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !value
            .to_ascii_lowercase()
            .starts_with(&format!("{}://", parsed.scheme()))
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || value.split_once("://").is_some_and(|(_, rest)| {
            rest.split('/')
                .next()
                .is_none_or(|authority| authority.is_empty() || authority.contains('@'))
        })
    {
        return Err(problem);
    }
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}

fn segment(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~') {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(result, "%{byte:02X}").expect("writing a string");
        }
    }
    result
}

impl Service {
    /// Static pages for an error's literal wire code and explicitly designated reasons.
    /// Call this on the audience projection so removed enum values never enter an SDK.
    pub fn error_documentation(&self, id: &ShapeId) -> Option<ErrorDocumentation> {
        let Shape::Structure(shape) = self.shape(id)? else {
            return None;
        };
        let info = shape.error.as_ref()?;
        let url = self
            .error_docs_base
            .as_ref()
            .map(|base| format!("{base}/{}", segment(&info.wire_code)));
        let reason_member = info
            .reason_member
            .as_ref()
            .and_then(|name| shape.member(name));
        let mut reasons = match reason_member.and_then(|member| self.shape(&member.target)) {
            Some(Shape::Enum(values)) => values
                .values
                .iter()
                .map(|reason| ErrorReason {
                    name: reason.name.clone(),
                    value: reason.value.clone(),
                    docs: reason.docs.clone(),
                    notes: reason.notes.clone(),
                    // The prefix prevents empty and dot-only values becoming URL path traversal.
                    url: url
                        .as_ref()
                        .map(|url| format!("{url}/reason-{}", segment(&reason.value))),
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        reasons.sort_by(|a, b| a.value.cmp(&b.value).then(a.name.cmp(&b.name)));
        Some(ErrorDocumentation {
            url,
            reason_member: reason_member.map(|m| m.name.clone()),
            reasons,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{base, segment};

    #[test]
    fn bases_validate_before_normalizing() {
        for value in [
            "https://docs.example.test/errors/",
            "http://localhost:8000/errors/",
        ] {
            assert_eq!(base(value).unwrap(), value.trim_end_matches('/'));
        }
        for value in [
            "https:example.test",
            "https:///example.test",
            "https://",
            "mailto:x@y.test",
            "//docs.test",
            "https://u:p@docs.test",
            "https://@docs.test",
            "https://docs.test?q=x",
            "https://docs.test#x",
            "https://docs.test/a b",
            "https://docs.test/\\x",
            "https://docs.test/%",
            "https://docs.test/%GG",
            "https://docs.test/\u{202e}",
        ] {
            assert!(base(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn prefixed_reason_components_round_trip_without_dot_normalization() {
        for value in ["", ".", "..", "a/b", "a?b#c", "café", "%2e%2e"] {
            let path = format!("https://docs.test/errors/Denied/reason-{}", segment(value));
            let parsed = url::Url::parse(&path).unwrap();
            assert_eq!(parsed.as_str(), path);
            assert_eq!(parsed.path_segments().unwrap().count(), 3);
        }
    }
}
