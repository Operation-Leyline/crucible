//! Explicit, target-owned opt-ins to semantics beyond the original JSON subset.
use std::collections::BTreeSet;
use std::{fmt, str::FromStr};

use crate::{Diagnostics, ShapeId};

macro_rules! features {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
        pub enum Feature { $($variant),+ }
        impl Feature {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }
        }
    };
}

features! {
    JsonName => "json-name", GreedyLabels => "greedy-labels",
    QueryParams => "query-params", PrefixHeaders => "prefix-headers",
    ResponseCode => "response-code", Sparse => "sparse", Sets => "sets",
    IntEnum => "int-enum", BigInteger => "big-integer", BigDecimal => "big-decimal",
    NonBlobPayload => "non-blob-payload", ErrorPayload => "error-payload",
    MediaTypeHeaders => "media-type-headers", HttpChecksum => "http-checksum",
    Streaming => "streaming", EventStreams => "event-streams",
    EndpointDiscovery => "endpoint-discovery",
    EndpointHostPrefix => "endpoint-host-prefix", EndpointRules => "endpoint-rules",
    Waiters => "waiters", BasicAuth => "basic-auth", ApiKeyAuth => "api-key-auth",
    OptionalAuth => "optional-auth",
    DigestAuth => "digest-auth", SigV4 => "sigv4", SigV4a => "sigv4a",
    AwsRestJson1 => "aws-rest-json-1", AwsJson10 => "aws-json-1-0",
    AwsJson11 => "aws-json-1-1", AwsRestXml => "aws-rest-xml",
    AwsQuery => "aws-query", Ec2Query => "ec2-query",
    SmithyRpcV2Cbor => "smithy-rpc-v2-cbor", SmithyRpcV2Json => "smithy-rpc-v2-json",
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Feature {
    type Err = String;
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|feature| feature.name() == name)
            .ok_or_else(|| format!("unknown client capability {name:?}"))
    }
}

/// A generator's tested capabilities. An empty set is the original subset.
/// Listing a capability does not enable unfinished lowering implementations.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Capabilities(BTreeSet<Feature>);

impl Capabilities {
    pub fn new(features: impl IntoIterator<Item = Feature>) -> Self {
        Self(features.into_iter().collect())
    }
    pub fn supports(&self, feature: Feature) -> bool {
        self.0.contains(&feature)
    }
    pub fn iter(&self) -> impl Iterator<Item = Feature> + '_ {
        self.0.iter().copied()
    }
    pub fn check(&self, uses: &[FeatureUse]) -> Result<(), Diagnostics> {
        let mut diagnostics = Diagnostics::default();
        for usage in uses {
            if !self.supports(usage.feature) {
                diagnostics.push(
                    &usage.shape,
                    format!(
                        "{} requires unsupported target capability {}",
                        usage.source, usage.feature
                    ),
                );
            }
        }
        if diagnostics.is_empty() {
            Ok(())
        } else {
            Err(diagnostics)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct FeatureUse {
    pub feature: Feature,
    pub shape: ShapeId,
    /// Fully-qualified trait (or prelude type) that introduces the requirement.
    pub source: ShapeId,
}
