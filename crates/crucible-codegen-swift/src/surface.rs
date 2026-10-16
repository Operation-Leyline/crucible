//! The Swift package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, method_name, pages_method_name, ty};

/// What the generated package shows its callers: the SwiftPM package and
/// module, the client class, and per operation its `async` method, the
/// page sequence, and the models it takes (memberwise initializer labels)
/// and returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("try {client}()")
    } else {
        format!("try {client}(ClientConfig(endpoint: endpoint))")
    };
    let mut out = surface::Surface::new(
        "swift",
        Package::new("swiftpm", &options.package_name, &options.module_name),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    let module = &options.module_name;
    for op in &service.operations {
        let mut entry = Operation::new(method_name(op));
        entry.paginators = pages_method_name(op).map(|name| vec![name]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, module, "keywords"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, module, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model struct as `Models.swift` declares it: a property per member,
/// and an initializer label per member.
fn structure(service: &Service, id: &ShapeId, module: &str, style: &str) -> Structure {
    let mut structure = Structure::new(caller_type(&ty(service, id, module)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    caller_type(&ty(service, &member.target, module)),
                )
            })
            .collect();
    }
    structure
}

/// A type as a caller writes it with the module imported: `` `City` ``
/// (or a reserved name's `` `TypeModel` ``) reads `City`, `Swift.Int64`
/// reads `Int64`.
fn caller_type(ty: &str) -> String {
    surface::unqualified(ty, ".").replace('`', "")
}
