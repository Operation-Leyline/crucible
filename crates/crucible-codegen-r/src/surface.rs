//! The R package's public surface (`crucible.sdks/1`), named by the
//! functions the client and model constructors are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, method, pages, r_class};

/// What the generated R package shows its callers: the package, the client
/// constructor, and per operation its methods, the model constructor it
/// takes (named arguments) and the classed list it returns (`$` elements).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}(client_config(endpoint = endpoint))")
    };
    let package = &options.package_name;
    let mut out = surface::Surface::new(
        "r",
        Package::new("cran", package, package),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method(operation));
        entry.paginators = pages(operation).map(|pages| vec![pages]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, "keywords"));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, "elements"));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A model constructor's arguments, which are also its value's elements.
fn structure(service: &Service, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(service.name_of(id), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    r_class(service, &member.target),
                )
            })
            .collect();
    }
    structure
}
