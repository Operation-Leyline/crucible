//! `[Name]` doc links (decision 80) as Go doc links, which `go doc`, gopls
//! and pkg.go.dev resolve for exported identifiers of the package itself:
//! `[Widget]`, `[Widget.Name]`, `[ColorRED]` for an enum value (a
//! package constant), `[LinksClient.GetWidget]` and `[LinksClient]`, each
//! named as this generator names the symbol. A resource has no Go symbol
//! and is plain text, as is every link in the `permissions` sub-package,
//! which does not import the package it would name.
use crate::exported;
use crucible_codegen_common::{doc_links, documentation};
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::Service;

/// Where a comment is, which decides what a doc link can name.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Package {
    /// The client package, which declares every model and the client.
    Client,
    /// The `permissions` sub-package: every link is text.
    Permissions,
}

fn link(service: &Service, target: DocTarget<'_>) -> Option<String> {
    let client = format!("{}Client", exported(service.id.name()));
    let reference = match target {
        DocTarget::Service => client,
        DocTarget::Operation(op) => format!("{client}.{}", exported(op.name())),
        DocTarget::Resource(_) => return None,
        DocTarget::Shape(id, _) => exported(service.name_of(id)),
        DocTarget::Member { shape, member, .. } => format!(
            "{}.{}",
            exported(service.name_of(shape)),
            exported(&member.name)
        ),
        DocTarget::EnumValue { shape, value, .. } => {
            format!("{}{}", exported(service.name_of(shape)), exported(value))
        }
    };
    Some(format!("[{reference}]"))
}

/// [`documentation::text`] with the model text's doc links as Go doc links
/// in the client package, and as their text elsewhere.
pub(crate) fn text(service: &Service, package: Package, markdown: &str) -> String {
    doc_links::render(service, markdown, documentation::text, |target| {
        (package == Package::Client)
            .then(|| link(service, target))
            .flatten()
    })
}
