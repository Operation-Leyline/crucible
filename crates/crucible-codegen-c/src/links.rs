//! Doc links (decision 80) as Doxygen reads them: the generated symbol's
//! name, which Doxygen links to its documentation (`name()` for a function),
//! and plain text for what has no symbol.
use crucible_codegen_common::doc_links;
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{Service, Shape};

use crate::{models, names};

/// `docs` with its doc links rendered, before [`crucible_codegen_common::doxygen`]
/// escapes it: the names it inserts contain nothing that escaping changes.
pub fn render(service: &Service, docs: Option<&str>, prefix: &str) -> Option<String> {
    docs.map(|docs| text(service, docs, prefix))
}

pub fn text(service: &Service, docs: &str, prefix: &str) -> String {
    doc_links::render(service, docs, str::to_owned, |target| {
        link(service, target, prefix)
    })
}

fn link(service: &Service, target: DocTarget<'_>, prefix: &str) -> Option<String> {
    Some(match target {
        DocTarget::Service => format!("{prefix}_client_new()"),
        DocTarget::Operation(op) => format!("{}()", crate::client::function(op, prefix)),
        DocTarget::Shape(id, shape) => {
            if !models::complex(shape) && !matches!(shape, Shape::Enum(_) | Shape::IntEnum(_)) {
                return None;
            }
            models::native_name(service, id, prefix)
        }
        DocTarget::Member {
            shape,
            owner,
            member,
        } => {
            let native = models::native_name(service, shape, prefix);
            let field = names::field(&member.name);
            match owner {
                // A structure's members have accessor functions.
                Shape::Structure(_) => format!("{native}_get_{field}()"),
                _ => return None,
            }
        }
        DocTarget::EnumValue { shape, value, .. } => format!(
            "{}_known_{}()",
            models::native_name(service, shape, prefix),
            names::field(value)
        ),
        DocTarget::Resource(_) => return None,
    })
}
