//! The module's public surface (`crucible.sdks/1`), named by the functions
//! `client.go` and `models.go` are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, exported, method_name, pages_name, ty, type_name};

/// What the generated module shows its callers: the module path, the
/// client struct, and per operation its method, paginator, and the
/// structs it takes (field literals) and returns (fields).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let package = &options.package_name;
    let config = if options.endpoint.is_some() {
        format!("{package}.Config{{}}")
    } else {
        format!("{package}.Config{{Endpoint: endpoint}}")
    };
    let mut out = surface::Surface::new(
        "go",
        Package::new("go", &options.module_path, &options.module_path),
        Client::new(&client, format!("{package}.New{client}({config})")),
    );
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method_name(operation.name()));
        entry.paginators = operation
            .paginated
            .as_ref()
            .map(|_| vec![pages_name(operation.name())]);
        entry.input = operation.input.as_ref().map(|id| structure(service, id));
        entry.output = operation.output.as_ref().map(|id| structure(service, id));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A struct as `models.go` declares it: an exported field per member,
/// typed by its value (an optional member's field wraps it in `Optional`).
fn structure(service: &Service, id: &ShapeId) -> Structure {
    let mut structure = Structure::new(type_name(service, id), "fields");
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    exported(&member.name),
                    ty(service, &member.target),
                )
            })
            .collect();
    }
    structure
}
