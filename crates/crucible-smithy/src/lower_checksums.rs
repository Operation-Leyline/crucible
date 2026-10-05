use crate::ast::{self, Kind};
use crate::ir::{Checksum, ChecksumAlgorithm};
use crate::lower::Lowerer;
use crate::{Feature, ShapeId};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Configuration {
    #[serde(default)]
    request_checksum_required: bool,
    request_algorithm_member: Option<String>,
    request_validation_mode_member: Option<String>,
    #[serde(default)]
    response_algorithms: Vec<ChecksumAlgorithm>,
}

impl Lowerer<'_> {
    pub(crate) fn checksum(
        &mut self,
        id: &ShapeId,
        operation: &ast::Shape,
        input: Option<&ShapeId>,
    ) -> Option<Checksum> {
        let legacy = ShapeId::new("smithy.api", "httpChecksumRequired");
        let flexible = ShapeId::new("aws.protocols", "httpChecksum");
        let Some(value) = operation.traits.get(&flexible) else {
            if operation.traits.contains_key(&legacy) {
                self.require(id, Feature::HttpChecksum, legacy);
                return Some(Checksum::Md5);
            }
            return None;
        };
        self.require(id, Feature::HttpChecksum, flexible);
        let config: Configuration = match serde_json::from_value(value.clone()) {
            Ok(config) => config,
            Err(error) => {
                self.diagnostics
                    .push(id, format!("invalid @aws.protocols#httpChecksum: {error}"));
                return None;
            }
        };
        if !config.request_checksum_required
            && config.request_algorithm_member.is_none()
            && config.request_validation_mode_member.is_none()
        {
            self.diagnostics.push(
                id,
                "@aws.protocols#httpChecksum must configure request or response checksums",
            );
        }
        if config.request_validation_mode_member.is_some() != !config.response_algorithms.is_empty()
        {
            self.diagnostics.push(id, "response checksums require both requestValidationModeMember and responseAlgorithms");
        }
        let mut request_algorithms = Vec::new();
        for (member, validation) in config
            .request_algorithm_member
            .iter()
            .map(|m| (m, false))
            .chain(
                config
                    .request_validation_mode_member
                    .iter()
                    .map(|m| (m, true)),
            )
        {
            let target = input
                .and_then(|id| self.model.get(id))
                .and_then(|s| s.members.get(member))
                .and_then(|m| self.model.get(&m.target));
            let Some(target) = target.filter(|t| t.kind == Kind::Enum) else {
                self.diagnostics.push(
                    id,
                    format!("checksum member {member:?} must name an input enum member"),
                );
                continue;
            };
            let values: Vec<_> = target
                .members
                .iter()
                .map(|(name, m)| {
                    m.traits
                        .get(&ShapeId::new("smithy.api", "enumValue"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(name)
                })
                .collect();
            if validation {
                if !values.contains(&"ENABLED") {
                    self.diagnostics.push(
                        id,
                        format!("checksum validation member {member:?} must contain ENABLED"),
                    );
                }
            } else {
                for value in values {
                    match serde_json::from_value(serde_json::json!(value)) {
                        Ok(algorithm) => request_algorithms.push(algorithm),
                        Err(_) => self.diagnostics.push(id, format!("checksum member {member:?} contains unsupported algorithm {value:?}")),
                    }
                }
            }
        }
        Some(Checksum::AwsFlexible {
            request_required: config.request_checksum_required,
            request_algorithm_member: config.request_algorithm_member,
            request_algorithms,
            validation_mode_member: config.request_validation_mode_member,
            response_algorithms: config.response_algorithms,
        })
    }
}
