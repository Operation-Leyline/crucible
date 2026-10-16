//! Doc links (decision 80) in the R package's two documentation forms: the
//! Rd pages it ships (`\code{\link{topic}}`) and the roxygen comments beside
//! the code (`[text][topic]`, roxygen's Markdown link). A model type's
//! topic is its constructor's name, an operation's the `operations` page,
//! the client's its constructor; what has no topic is plain text.
use crucible_codegen_common::doc_links;
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::{Service, Shape};

use crate::{field, markdown};

/// Model Markdown as Rd, its doc links as Rd links.
pub fn rd(service: &Service, text: &str) -> String {
    doc_links::render(service, text, markdown::render, |target| {
        let (text, topic) = link(service, target)?;
        let text = markdown::escape(&text);
        Some(if text == topic {
            format!(r"\code{{\link{{{topic}}}}}")
        } else {
            format!(r"\code{{\link[={topic}]{{{text}}}}}")
        })
    })
}

/// Model Markdown for a roxygen `@md` comment, its doc links as roxygen
/// links.
pub fn roxygen(service: &Service, text: &str) -> String {
    doc_links::render(service, text, str::to_owned, |target| {
        let (text, topic) = link(service, target)?;
        Some(if text == topic {
            format!("[{topic}]")
        } else {
            format!("[{text}][{topic}]")
        })
    })
}

/// What a link shows, as R code, and the help topic it opens.
fn link(service: &Service, target: DocTarget<'_>) -> Option<(String, String)> {
    let client = format!("{}Client", service.id.name());
    let model = |id| -> Option<String> {
        matches!(
            service.shape(id),
            Some(Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_))
        )
        .then(|| service.name_of(id).to_owned())
    };
    Some(match target {
        DocTarget::Service => (format!("{client}()"), client),
        DocTarget::Operation(op) => (
            format!("client${}()", field(op.name())),
            "operations".to_owned(),
        ),
        DocTarget::Shape(id, _) => {
            let name = model(id)?;
            (name.clone(), name)
        }
        DocTarget::Member { shape, member, .. } => {
            let name = model(shape)?;
            (format!("{name}${}", field(&member.name)), name)
        }
        // A value is constructed from its wire value.
        DocTarget::EnumValue {
            shape,
            owner,
            value,
        } => {
            let name = model(shape)?;
            let wire = match owner {
                Shape::Enum(e) => crucible_codegen_common::quoted(
                    &e.values.iter().find(|v| v.name == value)?.value,
                ),
                Shape::IntEnum(e) => {
                    format!("{}L", e.values.iter().find(|v| v.name == value)?.value)
                }
                _ => return None,
            };
            (format!("{name}({wire})"), name)
        }
        DocTarget::Resource(_) => return None,
    })
}
