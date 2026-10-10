//! XML element and query-key names are resolved before any emitter sees them.
use crate::ast::{self, Kind, Traits};
use crate::{Diagnostics, Protocol, ShapeId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmlNamespace {
    pub uri: String,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct XmlShape {
    pub name: String,
    pub namespace: Option<XmlNamespace>,
    pub members: BTreeMap<String, XmlMember>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XmlMember {
    pub name: String,
    pub query_name: String,
    pub namespace: Option<XmlNamespace>,
    pub flattened: bool,
    pub attribute: bool,
    /// The root element name when this member is a restXml `@httpPayload`:
    /// the member's own `@xmlName`, else the target's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_name: Option<String>,
}

pub(crate) fn namespace(
    id: &ShapeId,
    traits: &Traits,
    diagnostics: &mut Diagnostics,
) -> Option<XmlNamespace> {
    let value = traits.get(&ShapeId::new("smithy.api", "xmlNamespace"))?;
    match serde_json::from_value::<XmlNamespace>(value.clone()) {
        Ok(namespace)
            if !namespace.uri.is_empty() && namespace.prefix.as_deref().is_none_or(identifier) =>
        {
            Some(namespace)
        }
        _ => {
            diagnostics.push(
                id,
                "@xmlNamespace requires a URI and optional XML identifier prefix",
            );
            None
        }
    }
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn name(id: &ShapeId, traits: &Traits, default: &str, diagnostics: &mut Diagnostics) -> String {
    let Some(value) = traits.get(&ShapeId::new("smithy.api", "xmlName")) else {
        return default.into();
    };
    if let Some(value) = value
        .as_str()
        .filter(|value| value.split(':').count() <= 2 && value.split(':').all(identifier))
    {
        return value.into();
    }
    diagnostics.push(
        id,
        "@xmlName must be an XML name with at most one namespace prefix",
    );
    default.into()
}

pub(crate) fn lower(
    model: &BTreeMap<ShapeId, ast::Shape>,
    reached: &[ShapeId],
    protocol: Protocol,
    diagnostics: &mut Diagnostics,
) -> BTreeMap<ShapeId, XmlShape> {
    if !matches!(
        protocol,
        Protocol::AwsRestXml | Protocol::AwsQuery | Protocol::Ec2Query
    ) {
        return BTreeMap::new();
    }
    let mut result = BTreeMap::new();
    for id in reached {
        let shape = &model[id];
        let mut members = BTreeMap::new();
        for (member_name, member) in shape
            .members
            .iter()
            .map(|(name, member)| (name.as_str(), member))
            .chain(shape.member.iter().map(|member| ("member", member)))
            .chain(shape.key.iter().map(|member| ("key", member)))
            .chain(shape.value.iter().map(|member| ("value", member)))
        {
            let member_id = id.with_member(member_name);
            let name = name(&member_id, &member.traits, member_name, diagnostics);
            let target = model.get(&member.target);
            let query_name = if protocol == Protocol::Ec2Query
                && matches!(shape.kind, Kind::Structure | Kind::Union)
            {
                if let Some(value) = member
                    .traits
                    .get(&ShapeId::new("aws.protocols", "ec2QueryName"))
                {
                    match value.as_str().filter(|name| !name.is_empty()) {
                        Some(value) => value.into(),
                        None => {
                            diagnostics.push(&member_id, "@ec2QueryName must be a nonempty string");
                            name.clone()
                        }
                    }
                } else {
                    let mut value = name.clone();
                    if let Some(first) = value.get_mut(..1) {
                        first.make_ascii_uppercase();
                    }
                    value
                }
            } else {
                name.clone()
            };
            let flattened = member
                .traits
                .contains_key(&ShapeId::new("smithy.api", "xmlFlattened"));
            // A flattened collection has no wrapper element for its own namespace.
            let own_namespace = namespace(&member_id, &member.traits, diagnostics);
            let namespace = if flattened {
                own_namespace
            } else {
                own_namespace.or_else(|| {
                    target.and_then(|target| namespace(&member.target, &target.traits, diagnostics))
                })
            };
            let payload_name = (protocol == Protocol::AwsRestXml
                && member
                    .traits
                    .contains_key(&ShapeId::new("smithy.api", "httpPayload"))
                && target
                    .is_some_and(|target| matches!(target.kind, Kind::Structure | Kind::Union)))
            .then(|| {
                if member
                    .traits
                    .contains_key(&ShapeId::new("smithy.api", "xmlName"))
                {
                    name.clone()
                } else {
                    // The target's own @xmlName is diagnosed where the target is lowered.
                    model[&member.target]
                        .traits
                        .get(&ShapeId::new("smithy.api", "xmlName"))
                        .and_then(|value| value.as_str())
                        .unwrap_or(member.target.name())
                        .to_owned()
                }
            });
            let attribute = member
                .traits
                .contains_key(&ShapeId::new("smithy.api", "xmlAttribute"));
            if flattened
                && !target
                    .is_some_and(|target| matches!(target.kind, Kind::List | Kind::Set | Kind::Map))
            {
                diagnostics.push(&member_id, "@xmlFlattened must target a list or map");
            }
            if attribute
                && (namespace.is_some()
                    || !target.is_some_and(|target| {
                        matches!(
                            target.kind,
                            Kind::Boolean
                                | Kind::Byte
                                | Kind::Short
                                | Kind::Integer
                                | Kind::Long
                                | Kind::Float
                                | Kind::Double
                                | Kind::BigInteger
                                | Kind::BigDecimal
                                | Kind::IntEnum
                                | Kind::String
                                | Kind::Enum
                                | Kind::Timestamp
                        )
                    }))
            {
                diagnostics.push(
                    &member_id,
                    "@xmlAttribute requires a scalar member without an XML namespace",
                );
            }
            members.insert(
                member_name.into(),
                XmlMember {
                    name,
                    query_name,
                    namespace,
                    flattened,
                    attribute,
                    payload_name,
                },
            );
        }
        result.insert(
            id.clone(),
            XmlShape {
                name: name(id, &shape.traits, id.name(), diagnostics),
                namespace: namespace(id, &shape.traits, diagnostics),
                members,
            },
        );
    }
    result
}
