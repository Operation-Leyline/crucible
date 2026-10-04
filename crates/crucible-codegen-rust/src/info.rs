//! The `'static` descriptions generated code hands the framework: members
//! and where they travel, operations, errors, the service.

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{
    Binding, Bindings, Member, Service, Shape, SimpleKind, Structure, TimestampFormat,
};

use crate::names::literal;

/// `&[MemberInfo { .. }, ..]` for a structure, with each member's location
/// taken from `bindings` (labels, query and headers; the rest is body).
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
            } else {
                "Body".to_owned()
            };
            let scalar = if location == "Body" {
                "Json".to_owned()
            } else {
                scalar(service, member)
            };
            format!(
                "::crucible::operation::MemberInfo {{ name: {}, location: ::crucible::operation::Location::{location}, scalar: ::crucible::operation::Scalar::{scalar}, sensitive: {} }}",
                literal(&member.name),
                sensitive(service, member)
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
            SimpleKind::Document => "Json",
        }
        .to_owned(),
        _ => "String".to_owned(),
    }
}

/// `@sensitive` on the member or its target.
fn sensitive(service: &Service, member: &Member) -> bool {
    let id = ShapeId::new("smithy.api", "sensitive");
    let target = match service.shape(&member.target) {
        Some(Shape::Simple(s)) => s.traits.contains_key(&id),
        Some(Shape::Structure(s) | Shape::Union(s)) => s.traits.contains_key(&id),
        Some(Shape::List(l)) => l.traits.contains_key(&id),
        Some(Shape::Map(m)) => m.traits.contains_key(&id),
        Some(Shape::Enum(e)) => e.traits.contains_key(&id),
        _ => false,
    };
    target || member.traits.contains_key(&id)
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
