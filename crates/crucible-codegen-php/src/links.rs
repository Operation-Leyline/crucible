//! `[Name]` doc links (decision 80) as PHPDoc inline `{@see}` tags, fully
//! qualified so they resolve from any namespace the SDK writes:
//! `{@see \Ns\Widget}`, `{@see \Ns\Widget::$name}`, `{@see \Ns\Color::RED}`,
//! `{@see \Ns\ChoiceRecord}` for a union member and
//! `{@see \Ns\LinksClient::getWidget()}`, each named as this generator names
//! the symbol. A target with no PHP class (a resource, a list, map or
//! scalar, which are PHP arrays and scalars) is plain text.
use crate::names::{constant, field, variant};
use crucible_codegen_common::{doc_links, docs};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{DocNotes, Service, Shape};

fn link(service: &Service, namespace: &str, target: DocTarget<'_>) -> Option<String> {
    let class = |name: &str| format!("\\{namespace}\\{name}");
    let client = class(&crate::client_name(service));
    let reference = match target {
        DocTarget::Service => client,
        DocTarget::Operation(op) => format!("{client}::{}()", crate::operation_method(op)),
        DocTarget::Resource(_) => return None,
        DocTarget::Shape(id, shape) => match shape {
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_) => {
                class(service.name_of(id))
            }
            _ => return None,
        },
        DocTarget::Member {
            shape,
            owner,
            member,
        } => match owner {
            // A union member is a subclass of the union.
            Shape::Union(_) => class(&format!(
                "{}{}",
                service.name_of(shape),
                variant(&member.name)
            )),
            _ => format!(
                "{}::${}",
                class(service.name_of(shape)),
                field(&member.name)
            ),
        },
        DocTarget::EnumValue { shape, value, .. } => {
            format!("{}::{}", class(service.name_of(shape)), constant(value))
        }
    };
    Some(format!("{{@see {reference}}}"))
}

/// [`docs::escape_block`] with the model text's doc links as `{@see}` tags.
pub(crate) fn escape_block(service: &Service, namespace: &str, text: &str) -> String {
    doc_links::render(service, text, docs::escape_block, |target| {
        link(service, namespace, target)
    })
}

/// [`docs::annotated_block`] with the model text's doc links as `{@see}`
/// tags.
pub(crate) fn annotated_block(
    service: &Service,
    namespace: &str,
    text: Option<&str>,
    notes: &DocNotes,
) -> Option<String> {
    let mut parts: Vec<String> = text
        .map(|text| escape_block(service, namespace, text))
        .into_iter()
        .collect();
    parts.extend(docs::note_paragraphs_block(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}
