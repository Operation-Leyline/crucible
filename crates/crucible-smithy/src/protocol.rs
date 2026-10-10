use crate::ir::TimestampFormat;
use crate::{Feature, ShapeId};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// `bigInteger` and `bigDecimal` travel as JSON strings (RPC v2 JSON).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub arbitrary_precision_strings: bool,
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
            arbitrary_precision_strings: false,
        }
    }
}

/// The selected wire protocol, never inferred separately by an emitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    #[serde(rename = "rpcv2Cbor")]
    SmithyRpcV2Cbor,
    #[serde(rename = "rpcv2Json")]
    SmithyRpcV2Json,
}

impl Protocol {
    /// Fixed response headers. Modeled header bindings can override Content-Type.
    pub fn response_headers(
        self,
        bindings: &crate::ir::Bindings,
        has_value: bool,
    ) -> std::collections::BTreeMap<String, String> {
        let mut headers = std::collections::BTreeMap::new();
        if self.is_rpc_v2() {
            headers.insert(
                "Smithy-Protocol".into(),
                if self == Self::SmithyRpcV2Json {
                    "rpc-v2-json"
                } else {
                    "rpc-v2-cbor"
                }
                .into(),
            );
        }
        if has_value || matches!(self, Self::AwsRestJson1 | Self::AwsJson10 | Self::AwsJson11) {
            let media_type = if bindings.event_stream.is_some() {
                "application/vnd.amazon.eventstream"
            } else if let Some(payload) = &bindings.payload {
                &payload.media_type
            } else {
                match self {
                    Self::AwsJson10 => "application/x-amz-json-1.0",
                    Self::AwsJson11 => "application/x-amz-json-1.1",
                    Self::SmithyRpcV2Cbor => "application/cbor",
                    Self::AwsRestXml | Self::AwsQuery | Self::Ec2Query => "application/xml",
                    _ => "application/json",
                }
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
    /// The error discriminator header, where the protocol defines one.
    pub fn error_header(self) -> Option<&'static str> {
        match self {
            Self::CrucibleJson => Some("X-Crucible-Error"),
            Self::AwsRestJson1 | Self::AwsJson10 | Self::AwsJson11 => Some("X-Amzn-Errortype"),
            _ => None,
        }
    }
    /// Required request headers, resolved with protocol identity rather than
    /// reconstructed independently by documentation or client emitters.
    pub fn request_headers(
        self,
        service: &str,
        operation: &str,
    ) -> std::collections::BTreeMap<String, String> {
        let mut headers = std::collections::BTreeMap::new();
        match self {
            Self::AwsJson10 | Self::AwsJson11 => {
                headers.insert("X-Amz-Target".into(), format!("{service}.{operation}"));
                headers.insert(
                    "Content-Type".into(),
                    if self == Self::AwsJson10 {
                        "application/x-amz-json-1.0".into()
                    } else {
                        "application/x-amz-json-1.1".into()
                    },
                );
            }
            Self::SmithyRpcV2Json | Self::SmithyRpcV2Cbor => {
                headers.insert(
                    "Smithy-Protocol".into(),
                    if self == Self::SmithyRpcV2Json {
                        "rpc-v2-json".into()
                    } else {
                        "rpc-v2-cbor".into()
                    },
                );
                headers.insert(
                    "Content-Type".into(),
                    if self == Self::SmithyRpcV2Json {
                        "application/json".into()
                    } else {
                        "application/cbor".into()
                    },
                );
                headers.insert("Accept".into(), headers["Content-Type"].clone());
            }
            _ => {}
        }
        headers
    }
    pub const ALL: &[Self] = &[
        Self::CrucibleJson,
        Self::AwsRestJson1,
        Self::AwsJson10,
        Self::AwsJson11,
        Self::AwsRestXml,
        Self::AwsQuery,
        Self::Ec2Query,
        Self::SmithyRpcV2Cbor,
        Self::SmithyRpcV2Json,
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
            Self::SmithyRpcV2Cbor => ("smithy.protocols", "rpcv2Cbor"),
            Self::SmithyRpcV2Json => ("smithy.protocols", "rpcv2Json"),
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
            Self::SmithyRpcV2Cbor => Some(Feature::SmithyRpcV2Cbor),
            Self::SmithyRpcV2Json => Some(Feature::SmithyRpcV2Json),
        }
    }

    pub fn uses_http_bindings(self) -> bool {
        matches!(
            self,
            Self::CrucibleJson | Self::AwsRestJson1 | Self::AwsRestXml
        )
    }

    /// Smithy RPC v2 (CBOR or JSON): `POST /service/{Service}/operation/{Operation}`
    /// with a `Smithy-Protocol` header, epoch-second timestamps that ignore
    /// `@timestampFormat`, and errors identified only by the body's `__type`.
    pub fn is_rpc_v2(self) -> bool {
        matches!(self, Self::SmithyRpcV2Cbor | Self::SmithyRpcV2Json)
    }

    /// Whether `@timestampFormat` customizes this protocol's timestamps.
    pub fn respects_timestamp_format(self) -> bool {
        !self.is_rpc_v2()
    }

    pub fn uses_json_names(self) -> bool {
        matches!(self, Self::CrucibleJson | Self::AwsRestJson1)
    }

    pub fn member_timestamp_format(
        self,
        traits: &crate::ir::Traits,
        input: bool,
        output: bool,
    ) -> TimestampFormat {
        if self.uses_http_bindings() && (input || output) {
            if traits.contains_key(&ShapeId::new("smithy.api", "httpHeader")) {
                return self.timestamp_format(true);
            }
            if input
                && (traits.contains_key(&ShapeId::new("smithy.api", "httpQuery"))
                    || traits.contains_key(&ShapeId::new("smithy.api", "httpLabel")))
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
            Self::AwsRestJson1
            | Self::AwsJson10
            | Self::AwsJson11
            | Self::SmithyRpcV2Cbor
            | Self::SmithyRpcV2Json => TimestampFormat::EpochSeconds,
            _ => TimestampFormat::DateTime,
        }
    }
}
