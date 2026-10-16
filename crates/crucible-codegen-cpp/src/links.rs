//! Doc links (decision 80) as Doxygen reads them: the generated symbol's
//! qualified name, which Doxygen links to its documentation (`name()` for a
//! function), and plain text for what has no symbol.
use crucible_codegen_common::doc_links;
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{Service, Shape};

use crate::names;

/// `docs` with its doc links rendered, before [`crucible_codegen_common::doxygen`]
/// escapes it: the names it inserts contain nothing that escaping changes.
pub fn render(service: &Service, docs: Option<&str>) -> Option<String> {
    docs.map(|docs| text(service, docs))
}

pub fn text(service: &Service, docs: &str) -> String {
    doc_links::render(service, docs, str::to_owned, |target| link(service, target))
}

fn link(service: &Service, target: DocTarget<'_>) -> Option<String> {
    let client = crate::client::client_name(service);
    Some(match target {
        DocTarget::Service => client,
        DocTarget::Operation(op) => format!("{client}::{}()", crate::client::method(op)),
        // Structures, unions and enums are classes in `models`; other
        // shapes are aliases of standard types.
        DocTarget::Shape(id, shape) => match shape {
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_) => {
                format!("models::{}", service.name_of(id))
            }
            _ => return None,
        },
        DocTarget::Member {
            shape,
            owner,
            member,
        } => match owner {
            Shape::Structure(_) => format!(
                "models::{}::{}",
                service.name_of(shape),
                names::field(&member.name)
            ),
            _ => return None,
        },
        DocTarget::EnumValue { shape, value, .. } => format!(
            "models::{}::known_{}()",
            service.name_of(shape),
            names::field(value)
        ),
        DocTarget::Resource(_) => return None,
    })
}
