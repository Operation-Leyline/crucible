//! The `'static` descriptions generated code hands the framework: members
//! and where they travel, operations, errors, the service.

use crucible_smithy::ir::{
    Binding, Bindings, Member, Service, Shape, SimpleKind, Structure, TimestampFormat,
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
                format!("Payload({})", literal(&p.media_type))
            } else {
                "Body".to_owned()
            };
            let scalar = if location == "Body" || location.starts_with("Payload") {
                "Json".to_owned()
            } else {
                scalar(service, member)
            };
            format!(
                "::crucible::operation::MemberInfo {{ name: {}, location: ::crucible::operation::Location::{location}, scalar: ::crucible::operation::Scalar::{scalar}, redaction: {}, traits: {} }}",
                literal(&member.name),
                if structure.sensitive() {
                    "&::crucible::operation::Redaction::Sensitive".to_owned()
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

/// Error structures bind only headers.
pub(crate) fn error_bindings(headers: &[Binding]) -> Bindings {
    Bindings {
        headers: headers.to_vec(),
        ..Bindings::default()
    }
}

fn find<'b>(bindings: &'b [Binding], member: &str) -> Option<&'b Binding> {
    bindings.iter().find(|b| b.member == member)
}

/// How a bound member's text reads: the `Scalar` variant.
fn scalar(service: &Service, member: &Member) -> String {
    match service.shape(&member.target) {
        Some(Shape::List(list)) => format!(
            "List(&::crucible::operation::Scalar::{})",
            scalar(service, &list.member)
        ),
        Some(Shape::Simple(simple)) => match simple.kind {
            SimpleKind::Boolean => "Boolean",
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                "Integer"
            }
            SimpleKind::Float | SimpleKind::Double => "Float",
            SimpleKind::Timestamp => match service.timestamp_format(member) {
                TimestampFormat::DateTime => "DateTime",
                TimestampFormat::EpochSeconds => "EpochSeconds",
                TimestampFormat::HttpDate => "HttpDate",
            },
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
        "&::crucible::operation::Redaction::Sensitive".to_owned()
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
    let mut out = String::from("#[allow(dead_code)]\nmod __redaction {\n");
    if !service.shapes.is_empty() {
        out.push_str("    use ::crucible::operation::Redaction;\n");
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

/// Traits from outside Smithy's and AWS's namespaces: the model's own.
pub(crate) fn custom_traits(traits: &crucible_smithy::ir::Traits) -> String {
    let pairs: Vec<String> = traits
        .iter()
        .filter(|(id, _)| {
            !id.is_prelude()
                && !id.namespace().starts_with("aws.")
                && id.namespace() != "crucible.protocols"
        })
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
