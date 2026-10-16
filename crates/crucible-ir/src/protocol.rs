use crate::ir::TimestampFormat;
use crate::{Feature, ShapeId};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolSettings {
    pub http: Vec<String>,
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self {
            http: vec!["http/1.1".into()],
        }
    }
}

/// The wire protocol. Crucible's own JSON protocol is the only one: the
/// enum stays so the IR still records which protocol the bindings use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Protocol {
    #[serde(rename = "crucibleJson")]
    CrucibleJson,
}

impl Protocol {
    /// Fixed request headers: the body's `Content-Type`, a payload's media
    /// type or JSON, unless a modelled header binding sends its own.
    pub fn request_headers(
        self,
        bindings: &crate::ir::Bindings,
    ) -> std::collections::BTreeMap<String, String> {
        let mut headers = std::collections::BTreeMap::new();
        if let Some(payload) = &bindings.payload {
            headers.insert("Content-Type".into(), payload.media_type.clone());
        } else if !bindings.body.is_empty() {
            headers.insert("Content-Type".into(), "application/json".into());
        }
        if bindings
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
        {
            headers.remove("Content-Type");
        }
        headers
    }

    /// Whether a response with `output` and these bindings carries a value:
    /// what [`Protocol::response_headers`] takes as `has_value`.
    pub fn response_has_value(
        self,
        output: Option<&ShapeId>,
        bindings: &crate::ir::Bindings,
    ) -> bool {
        output.is_some() && (!bindings.body.is_empty() || bindings.payload.is_some())
    }

    /// Fixed response headers. Modeled header bindings can override Content-Type.
    pub fn response_headers(
        self,
        bindings: &crate::ir::Bindings,
        has_value: bool,
    ) -> std::collections::BTreeMap<String, String> {
        let mut headers = std::collections::BTreeMap::new();
        if has_value {
            let media_type = if let Some(payload) = &bindings.payload {
                &payload.media_type
            } else {
                "application/json"
            };
            if !bindings
                .headers
                .iter()
                .any(|h| h.name.eq_ignore_ascii_case("Content-Type"))
            {
                headers.insert("Content-Type".into(), media_type.to_owned());
            }
        }
        headers
    }
    /// The error discriminator header.
    pub fn error_header(self) -> Option<&'static str> {
        Some("X-Crucible-Error")
    }
    pub const ALL: &[Self] = &[Self::CrucibleJson];

    pub fn trait_id(self) -> ShapeId {
        ShapeId::new("crucible.protocols", "json")
    }

    pub fn feature(self) -> Option<Feature> {
        None
    }

    pub fn member_timestamp_format(
        self,
        traits: &crate::ir::Traits,
        input: bool,
        output: bool,
    ) -> TimestampFormat {
        if input || output {
            if traits.contains_key(&ShapeId::prelude("httpHeader")) {
                return self.timestamp_format(true);
            }
            if input
                && (traits.contains_key(&ShapeId::prelude("httpQuery"))
                    || traits.contains_key(&ShapeId::prelude("httpLabel")))
            {
                return TimestampFormat::DateTime;
            }
        }
        self.timestamp_format(false)
    }

    pub fn timestamp_format(self, _header: bool) -> TimestampFormat {
        TimestampFormat::DateTime
    }
}
