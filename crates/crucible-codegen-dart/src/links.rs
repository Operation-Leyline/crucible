//! `[Name]` doc links (decision 80) as dartdoc references: `[Widget]`,
//! `[Widget.name]`, `[Color.red]`, `[ChoiceRecord]` for a union member and
//! `[LinksClient.getWidget]`, each named as this generator names the
//! symbol and qualified by the library's import prefix (`models.` in the
//! client). A target with no Dart symbol in scope (a resource, a scalar,
//! the client from the models library, which does not import it) is plain
//! text.
use super::{enum_constant, field};
use crucible_codegen_common::{doc_links, docs};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{DocNotes, Service, Shape};

/// The library a doc comment is in, which decides what is in scope.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Library {
    /// `models.dart`, which declares the models.
    Models,
    /// `client.dart`, which imports the models as `models`.
    Client,
    /// `permissions.dart`, which imports neither: every link is text.
    Permissions,
}

fn link(service: &Service, library: Library, target: DocTarget<'_>) -> Option<String> {
    let models = match library {
        Library::Models => "",
        Library::Client => "models.",
        Library::Permissions => return None,
    };
    let client = super::client_name(service);
    let reference = match target {
        DocTarget::Service if library == Library::Client => client,
        DocTarget::Operation(op) if library == Library::Client => {
            format!("{client}.{}", super::operation_method(op))
        }
        DocTarget::Service | DocTarget::Operation(_) | DocTarget::Resource(_) => return None,
        DocTarget::Shape(id, shape) => {
            if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
                return None;
            }
            format!("{models}{}", service.name_of(id))
        }
        DocTarget::Member {
            shape,
            owner,
            member,
        } => match owner {
            // A union member is a subclass of the union.
            Shape::Union(_) => format!(
                "{models}{}{}",
                service.name_of(shape),
                crate::models::variant(&member.name)
            ),
            _ => format!("{models}{}.{}", service.name_of(shape), field(&member.name)),
        },
        DocTarget::EnumValue { shape, value, .. } => {
            format!(
                "{models}{}.{}",
                service.name_of(shape),
                enum_constant(value)
            )
        }
    };
    Some(format!("[{reference}]"))
}

/// Model documentation through [`docs::escape`], its doc links as dartdoc
/// references.
pub(crate) fn escape(service: &Service, library: Library, text: &str) -> String {
    doc_links::render(service, text, docs::escape, |target| {
        link(service, library, target)
    })
}

/// [`docs::annotated`] with the model text's doc links as dartdoc references.
pub(crate) fn annotated(
    service: &Service,
    library: Library,
    text: Option<&str>,
    notes: &DocNotes,
) -> Option<String> {
    let mut parts: Vec<String> = text
        .map(|text| escape(service, library, text))
        .into_iter()
        .collect();
    parts.extend(docs::note_paragraphs(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}
