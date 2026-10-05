use crate::ir::TimestampFormat;
use crate::{Feature, ShapeId};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolSettings {
    pub http: Vec<String>,
    pub event_stream_http: Vec<String>,
    pub no_error_wrapping: bool,
    /// JSON protocol structure null handling; document contents and sparse entries are unaffected.
    pub omit_null_structure_members: bool,
    pub ignore_unknown_union_type: bool,
    /// Non-finite floats travel as the strings NaN, Infinity and -Infinity
    /// (AWS JSON and all XML-family protocols).
    pub non_finite_float_strings: bool,
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self {
            http: vec!["http/1.1".into()],
            event_stream_http: vec!["http/1.1".into()],
            no_error_wrapping: false,
            omit_null_structure_members: false,
            ignore_unknown_union_type: false,
            non_finite_float_strings: false,
        }
    }
}

/// The selected wire protocol, never inferred separately by an emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum Protocol {
    #[serde(rename = "crucibleJson")]
    CrucibleJson,
    #[serde(rename = "awsRestJson1")]
    AwsRestJson1,
    #[serde(rename = "awsJson1_0")]
    AwsJson10,
    #[serde(rename = "awsJson1_1")]
    AwsJson11,
    #[serde(rename = "awsRestXml")]
    AwsRestXml,
    #[serde(rename = "awsQuery")]
    AwsQuery,
    #[serde(rename = "ec2Query")]
    Ec2Query,
}

impl Protocol {
    pub const ALL: &[Self] = &[
        Self::CrucibleJson,
        Self::AwsRestJson1,
        Self::AwsJson10,
        Self::AwsJson11,
        Self::AwsRestXml,
        Self::AwsQuery,
        Self::Ec2Query,
    ];

    pub fn trait_id(self) -> ShapeId {
        let (namespace, name) = match self {
            Self::CrucibleJson => ("crucible.protocols", "json"),
            Self::AwsRestJson1 => ("aws.protocols", "restJson1"),
            Self::AwsJson10 => ("aws.protocols", "awsJson1_0"),
            Self::AwsJson11 => ("aws.protocols", "awsJson1_1"),
            Self::AwsRestXml => ("aws.protocols", "restXml"),
            Self::AwsQuery => ("aws.protocols", "awsQuery"),
            Self::Ec2Query => ("aws.protocols", "ec2Query"),
        };
        ShapeId::new(namespace, name)
    }

    pub fn feature(self) -> Option<Feature> {
        match self {
            Self::CrucibleJson => None,
            Self::AwsRestJson1 => Some(Feature::AwsRestJson1),
            Self::AwsJson10 => Some(Feature::AwsJson10),
            Self::AwsJson11 => Some(Feature::AwsJson11),
            Self::AwsRestXml => Some(Feature::AwsRestXml),
            Self::AwsQuery => Some(Feature::AwsQuery),
            Self::Ec2Query => Some(Feature::Ec2Query),
        }
    }

    pub fn uses_http_bindings(self) -> bool {
        matches!(
            self,
            Self::CrucibleJson | Self::AwsRestJson1 | Self::AwsRestXml
        )
    }

    pub fn uses_json_names(self) -> bool {
        matches!(self, Self::CrucibleJson | Self::AwsRestJson1)
    }

    pub fn member_timestamp_format(self, traits: &crate::ir::Traits) -> TimestampFormat {
        if self.uses_http_bindings() {
            if traits.contains_key(&ShapeId::new("smithy.api", "httpHeader")) {
                return self.timestamp_format(true);
            }
            if traits.contains_key(&ShapeId::new("smithy.api", "httpQuery"))
                || traits.contains_key(&ShapeId::new("smithy.api", "httpLabel"))
            {
                return TimestampFormat::DateTime;
            }
        }
        self.timestamp_format(false)
    }

    pub fn timestamp_format(self, header: bool) -> TimestampFormat {
        if header && matches!(self, Self::AwsRestJson1 | Self::AwsRestXml) {
            return TimestampFormat::HttpDate;
        }
        match self {
            Self::AwsRestJson1 | Self::AwsJson10 | Self::AwsJson11 => TimestampFormat::EpochSeconds,
            _ => TimestampFormat::DateTime,
        }
    }
}
