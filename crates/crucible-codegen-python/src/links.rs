//! `[Name]` doc links (decision 80) in docstrings. The generated docstrings
//! are plain text (Google style, read by `help()` and IDEs), which has no
//! link syntax, so a link is the generated Python name without brackets:
//! `Widget`, `Widget.name`, `Color.RED`, `LinksClient.get_widget`. A
//! resource has no Python symbol and keeps its label.
use crate::names::{enum_member, field};
use crucible_codegen_common::{doc_links, documentation};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{Service, Shape};

fn name(service: &Service, target: DocTarget<'_>) -> Option<String> {
    let client = format!("{}Client", service.id.name());
    Some(match target {
        DocTarget::Service => client,
        DocTarget::Operation(op) => format!("{client}.{}", field(op.name())),
        DocTarget::Resource(_) | DocTarget::Shape(_, Shape::Unit { .. }) => return None,
        DocTarget::Shape(id, _) => service.name_of(id).to_owned(),
        DocTarget::Member { shape, member, .. } => {
            format!("{}.{}", service.name_of(shape), field(&member.name))
        }
        DocTarget::EnumValue { shape, value, .. } => {
            format!("{}.{}", service.name_of(shape), enum_member(value))
        }
    })
}

/// [`documentation::text`] with the model text's doc links as Python names.
pub(crate) fn text(service: &Service, markdown: &str) -> String {
    doc_links::render(service, markdown, documentation::text, |target| {
        name(service, target)
    })
}
