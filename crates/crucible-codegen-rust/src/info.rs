//! The `'static` descriptions generated code hands the framework: members
//! and where they travel, operations, errors, the service.

use crucible_ir::ir::{
    Binding, Bindings, Member, PayloadKind, Service, Shape, SimpleKind, Structure, TimestampFormat,
};

use crate::names::literal;

/// `&[MemberInfo { .. }, ..]` for a structure, with each member's location
/// taken from `bindings` (labels, query, headers and the payload; the rest is
/// body).
pub(crate) fn members(service: &Service, structure: &Structure, bindings: &Bindings) -> String {
    let infos: Vec<String> = structure
        .members
        .iter()
        .map(|member| {
            let location = if bindings.labels.contains(&member.name) {
                "Label".to_owned()
            } else if let Some(b) = find(&bindings.query, &member.name) {
                format!("Query({})", literal(&b.name))
            } else if let Some(b) = find(&bindings.headers, &member.name) {
                format!("Header({})", literal(&b.name.to_ascii_lowercase()))
            } else if let Some(p) = bindings.payload.as_ref().filter(|p| p.member == member.name) {
                let kind = match p.kind {
                    PayloadKind::String => "TextPayload",
                    PayloadKind::Document => "DocumentPayload",
                    PayloadKind::Blob => "Payload",
                };
                format!("{kind}({})", literal(&p.media_type))
            } else {
                "Body".to_owned()
            };
            let scalar = if location == "Body" || location.contains("Payload") {
                "Json".to_owned()
            } else {
                scalar(
                    service,
                    member,
                    bindings.timestamp_formats.get(&member.name).copied(),
                    bindings.header_encodings.contains_key(&member.name),
                )
            };
            format!(
                "::crucible_server_runtime_rust::__generated::member_info({}, ::crucible_server_runtime_rust::__generated::operation::Location::{location}, ::crucible_server_runtime_rust::__generated::operation::Scalar::{scalar}, {}, {})",
                literal(&member.name),
                if structure.sensitive() {
                    "&::crucible_server_runtime_rust::__generated::operation::Redaction::Sensitive".to_owned()
                } else { redaction_member(service, member, "__redaction::") },
                custom_traits(&member.traits)
            )
        })
        .collect();
    if infos.is_empty() {
        "&[]".to_owned()
    } else {
        format!("&[\n        {},\n    ]", infos.join(",\n        "))
    }
}

/// Error structures bind headers and a payload.
pub(crate) fn error_bindings(error: &crucible_ir::ir::ErrorInfo) -> Bindings {
    Bindings {
        headers: error.headers.clone(),
        payload: error.payload.clone(),
        header_encodings: error.header_encodings.clone(),
        timestamp_formats: error.timestamp_formats.clone(),
        ..Bindings::default()
    }
}

fn find<'b>(bindings: &'b [Binding], member: &str) -> Option<&'b Binding> {
    bindings.iter().find(|b| b.member == member)
}

/// How a bound member's text reads: the `Scalar` variant. `timestamp` is
/// the binding's own format when it differs from the document's; `base64`
/// marks a header whose `@mediaType` strings travel base64-encoded.
fn scalar(
    service: &Service,
    member: &Member,
    timestamp: Option<TimestampFormat>,
    base64: bool,
) -> String {
    let format = |format: TimestampFormat| match format {
        TimestampFormat::DateTime => "DateTime",
        TimestampFormat::EpochSeconds => "EpochSeconds",
        TimestampFormat::HttpDate => "HttpDate",
    };
    match service.shape(&member.target) {
        Some(Shape::Map(map)) => scalar(service, &map.value, timestamp, base64),
        Some(Shape::IntEnum(_)) => "Integer".to_owned(),
        Some(Shape::Enum(_)) if base64 => "Base64".to_owned(),
        Some(Shape::List(list)) => format!(
            "List(&::crucible_server_runtime_rust::__generated::operation::Scalar::{})",
            scalar(service, &list.member, timestamp, base64)
        ),
        Some(Shape::Simple(simple)) => match simple.kind {
            SimpleKind::BigInteger => "BigInteger",
            SimpleKind::BigDecimal => "BigDecimal",
            SimpleKind::Boolean => "Boolean",
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                "Integer"
            }
            SimpleKind::Float | SimpleKind::Double => "Float",
            SimpleKind::Timestamp => {
                let text = timestamp.unwrap_or_else(|| service.timestamp_format(member));
                let json = service.document_timestamp_format(member);
                if text == json {
                    format(text)
                } else {
                    return format!(
                        "Timestamp {{ text: ::crucible_server_runtime_rust::__generated::operation::TimestampFormat::{}, json: ::crucible_server_runtime_rust::__generated::operation::TimestampFormat::{} }}",
                        format(text),
                        format(json)
                    );
                }
            }
            SimpleKind::String if base64 => "Base64",
            SimpleKind::String => "String",
            // Never bound: lowering refuses a blob in a label, query or header.
            SimpleKind::Document | SimpleKind::Blob => "Json",
        }
        .to_owned(),
        _ => "String".to_owned(),
    }
}

fn redaction_member(service: &Service, member: &Member, prefix: &str) -> String {
    if service.sensitive(member) {
        "&::crucible_server_runtime_rust::__generated::operation::Redaction::Sensitive".to_owned()
    } else {
        let index = service
            .shapes
            .keys()
            .position(|id| id == &member.target)
            .expect("reachable target");
        format!("&{prefix}R_{index}")
    }
}

/// A graph of static references also represents recursive model shapes.
pub(crate) fn redactions(service: &Service) -> String {
    let mut out = String::from("#[allow(dead_code, unused_imports)]\nmod __redaction {\n");
    if !service.shapes.is_empty() {
        out.push_str(
            "    use ::crucible_server_runtime_rust::__generated::operation::Redaction;\n",
        );
    }
    for (index, shape) in service.shapes.values().enumerate() {
        let policy = if shape.sensitive() {
            "Redaction::Sensitive".to_owned()
        } else {
            match shape {
                Shape::Structure(s) | Shape::Union(s) => {
                    let members = s
                        .members
                        .iter()
                        .map(|m| {
                            format!(
                                "({}, {})",
                                literal(&m.name),
                                redaction_member(service, m, "")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Redaction::Structure(&[{members}])")
                }
                Shape::List(l) => format!(
                    "Redaction::List({})",
                    redaction_member(service, &l.member, "")
                ),
                Shape::Map(m) if service.sensitive(&m.key) => "Redaction::Sensitive".to_owned(),
                Shape::Map(m) => format!(
                    "Redaction::Map({})",
                    redaction_member(service, &m.value, "")
                ),
                _ => "Redaction::Public".to_owned(),
            }
        };
        out.push_str(&format!(
            "    pub(super) static R_{index}: Redaction = {policy};\n"
        ));
    }
    out.push_str("}\n\n");
    out
}

/// Traits outside the standard vocabulary: the model's own.
pub(crate) fn custom_traits(traits: &crucible_ir::ir::Traits) -> String {
    let pairs: Vec<String> = traits
        .iter()
        .filter(|(id, _)| !crucible_ir::ir::is_standard_trait(id))
        .map(|(id, value)| {
            format!(
                "({}, {})",
                literal(&id.to_string()),
                literal(&value.to_string())
            )
        })
        .collect();
    format!("&[{}]", pairs.join(", "))
}

/// `SCREAMING_SNAKE` for a static's name.
pub(crate) fn constant(name: &str) -> String {
    crate::names::snake(name).to_ascii_uppercase()
}
