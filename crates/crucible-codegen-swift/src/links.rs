//! `[Name]` doc links (decision 80) as DocC symbol links: ``` ``Widget`` ```,
//! ``` ``Widget/name`` ```, ``` ``Color/red`` ``` and
//! ``` ``LinksClient/getWidget(_:)`` ```, each named as this generator names
//! the symbol. A target with no Swift symbol (a resource, a scalar) is
//! plain text.
use super::{enum_constant, field, method, model_name};
use crucible_codegen_common::{doc_links, docs};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{DocNotes, Service, Shape};

/// The DocC link to a doc link's target, if it has a Swift symbol.
fn link(service: &Service, target: DocTarget<'_>) -> Option<String> {
    let path = match target {
        DocTarget::Service => client(service),
        DocTarget::Operation(op) => format!("{}/{}(_:)", client(service), super::method_name(op)),
        DocTarget::Resource(_) => return None,
        DocTarget::Shape(id, shape) => {
            if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
                return None;
            }
            model_name(service.name_of(id))
        }
        DocTarget::Member {
            shape,
            owner,
            member,
        } => {
            let owner_name = model_name(service.name_of(shape));
            match owner {
                // A union member is an enum case with one associated value.
                Shape::Union(_) => format!("{owner_name}/{}(_:)", method(&member.name)),
                _ => format!("{owner_name}/{}", field(&member.name)),
            }
        }
        DocTarget::EnumValue { shape, value, .. } => {
            format!(
                "{}/{}",
                model_name(service.name_of(shape)),
                enum_constant(value)
            )
        }
    };
    Some(format!("``{path}``"))
}

fn client(service: &Service) -> String {
    super::client_name(service)
}

/// Model documentation through [`docs::escape`], its doc links as DocC links.
pub(crate) fn escape(service: &Service, text: &str) -> String {
    doc_links::render(service, text, docs::escape, |target| link(service, target))
}

/// [`docs::annotated`] with the model text's doc links as DocC links.
pub(crate) fn annotated(service: &Service, text: Option<&str>, notes: &DocNotes) -> Option<String> {
    let mut parts: Vec<String> = text.map(|text| escape(service, text)).into_iter().collect();
    parts.extend(docs::note_paragraphs(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}
