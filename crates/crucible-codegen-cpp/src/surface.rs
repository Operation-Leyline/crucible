//! The CMake package's public surface (`crucible.sdks/1`), named by the
//! functions the client and models are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::Options;
use crate::client::{client_name, method, pages};
use crate::models::Layout;
use crate::names;

/// What the generated CMake package shows its callers: the package and
/// header, the client class, and per operation its methods and the model
/// structs it takes and returns (public fields).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let layout = Layout::new(service, options);
    let package = &options.package_name;
    let ns = &options.namespace;
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{ns}::{client}()")
    } else {
        format!("{ns}::{client}({ns}::runtime::ClientConfig{{.endpoint = endpoint}})")
    };
    let mut out = surface::Surface::new(
        "cpp",
        Package::new("cmake", package, format!("{package}/client.hpp")),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method(operation));
        entry.paginators = pages(operation).map(|pages| vec![pages]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, &layout, &operation.id, id));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, &layout, &operation.id, id));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A model struct: its public fields, typed as `Layout::ty` declares them
/// (an optional member's field wraps that type in `runtime::Field`).
fn structure(service: &Service, layout: &Layout<'_>, owner: &ShapeId, id: &ShapeId) -> Structure {
    let mut structure = Structure::new(surface::unqualified(&layout.ty(owner, id), "::"), "fields");
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    names::field(&member.name),
                    surface::unqualified(&layout.ty(id, &member.target), "::"),
                )
            })
            .collect();
    }
    structure
}
