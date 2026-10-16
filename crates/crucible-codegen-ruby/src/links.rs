//! `[Name]` doc links (decision 80) as YARD references: `{Widget}`,
//! `{Widget#name}`, `{Color::RED}`, `{Choice::RecordVariant}` for a union
//! member and `{LinksClient#get_widget}`, each named as this generator
//! names the symbol; YARD resolves them from the gem's module outwards. A
//! target with no Ruby class (a resource, a list, map or scalar) is plain
//! text.
use crate::models::{enum_constant, variant};
use crate::names::field;
use crucible_codegen_common::{doc_links, docs};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{DocNotes, Service, Shape};

fn link(service: &Service, target: DocTarget<'_>) -> Option<String> {
    let client = format!("{}Client", service.id.name());
    let reference = match target {
        DocTarget::Service => client,
        DocTarget::Operation(op) => format!("{client}#{}", field(op.name())),
        DocTarget::Resource(_) => return None,
        DocTarget::Shape(id, shape) => match shape {
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_) => {
                service.name_of(id).to_owned()
            }
            _ => return None,
        },
        DocTarget::Member {
            shape,
            owner,
            member,
        } => match owner {
            // A union member is a class nested in the union's.
            Shape::Union(_) => format!("{}::{}", service.name_of(shape), variant(&member.name)),
            _ => format!("{}#{}", service.name_of(shape), field(&member.name)),
        },
        DocTarget::EnumValue { shape, value, .. } => {
            format!("{}::{}", service.name_of(shape), enum_constant(value))
        }
    };
    Some(format!("{{{reference}}}"))
}

/// [`docs::escape_block`] with the model text's doc links as YARD references.
pub(crate) fn escape_block(service: &Service, text: &str) -> String {
    doc_links::render(service, text, docs::escape_block, |target| {
        link(service, target)
    })
}

/// [`docs::annotated_block`] with the model text's doc links as YARD
/// references.
pub(crate) fn annotated_block(
    service: &Service,
    text: Option<&str>,
    notes: &DocNotes,
) -> Option<String> {
    let mut parts: Vec<String> = text
        .map(|text| escape_block(service, text))
        .into_iter()
        .collect();
    parts.extend(docs::note_paragraphs_block(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}
