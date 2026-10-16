//! The package's public surface (`crucible.sdks/1`), named by the functions
//! `client.py` and `models.py` are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::names::{field, ty};
use crate::{Options, client_name, method_name, pages_name};

/// What the generated package shows its callers: the distribution and
/// module, the client class, and per operation its method, paginator, and
/// the dataclasses it takes (keyword arguments) and returns (attributes).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}(endpoint=endpoint)")
    };
    let mut out = surface::Surface::new(
        "python",
        Package::new("pypi", &options.package_name, &options.module_name),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method_name(operation.name()));
        entry.paginators = operation
            .paginated
            .as_ref()
            .map(|_| vec![pages_name(operation.name())]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, "keywords"));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, "properties"));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A dataclass as `models.py` declares it: an attribute per member.
fn structure(service: &Service, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(ty(service, id, ""), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    ty(service, &member.target, ""),
                )
            })
            .collect();
    }
    structure
}
