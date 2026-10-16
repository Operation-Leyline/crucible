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
        let link = link(service, target)?;
        let text = markdown::escape(&link.text);
        let topic = &link.topic;
        let mut out = if text == *topic {
            format!(r"\code{{\link{{{topic}}}}}")
        } else {
            format!(r"\code{{\link[={topic}]{{{text}}}}}")
        };
        out.push_str(&link.after);
        Some(out)
    })
}

/// Model Markdown for a roxygen `@md` comment, its doc links as roxygen
/// links.
pub fn roxygen(service: &Service, text: &str) -> String {
    doc_links::render(service, text, str::to_owned, |target| {
        let link = link(service, target)?;
        let (text, topic) = (&link.text, &link.topic);
        let mut out = if text == topic {
            format!("[{topic}]")
        } else {
            format!("[{text}][{topic}]")
        };
        out.push_str(&link.after);
        Some(out)
    })
}

/// A link: what it shows, as R code built from identifiers only, the help
/// topic it opens, and plain text after it.
struct Link {
    text: String,
    topic: String,
    after: String,
}

fn link(service: &Service, target: DocTarget<'_>) -> Option<Link> {
    let client = crate::client_name(service);
    let model = |id| -> Option<String> {
        matches!(
            service.shape(id),
            Some(Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_))
        )
        .then(|| service.name_of(id).to_owned())
    };
    let to = |text: String, topic: String| Link {
        text,
        topic,
        after: String::new(),
    };
    Some(match target {
        DocTarget::Service => to(format!("{client}()"), client),
        DocTarget::Operation(op) => to(
            format!("client${}()", crate::method(op)),
            "operations".to_owned(),
        ),
        DocTarget::Shape(id, _) => {
            let name = model(id)?;
            to(name.clone(), name)
        }
        DocTarget::Member { shape, member, .. } => {
            let name = model(shape)?;
            to(format!("{name}${}", field(&member.name)), name)
        }
        // A value links its type and is named after it as the model names
        // it, an identifier: its wire value can hold quotes or braces,
        // which would run an R string in `\code` past its closing brace, so
        // it never enters a link.
        DocTarget::EnumValue { shape, value, .. } => {
            let name = model(shape)?;
            Link {
                text: name.clone(),
                topic: name,
                after: format!(".{value}"),
            }
        }
        DocTarget::Resource(_) => return None,
    })
}
