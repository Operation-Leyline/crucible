//! The package's public surface (`crucible.sdks/1`), named by the functions
//! the client class and the model builders are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{
    Options, accessor, async_name, client_name, member_field, method, pages_async_name, pages_name,
    ty,
};

/// What the generated package shows its callers: the Maven coordinate and
/// Java package, the client class, and per operation its blocking and
/// asynchronous methods, paginators, and the models it takes (through
/// `Model.builder()`) and returns (through accessors).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    // `ClientConfig` sets its endpoint through a public field, which no
    // single expression can assign: without a generated default endpoint
    // the expression reads a configured `config`.
    let construct = if options.endpoint.is_some() {
        format!("new {client}(new ClientConfig())")
    } else {
        format!("new {client}(config)")
    };
    let package = &options.package_name;
    let mut out = surface::Surface::new(
        "java",
        Package::new(
            "maven",
            format!("{}:{}", options.group_id, options.artifact_id),
            package,
        ),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method(operation.name()));
        entry.async_method = Some(async_name(operation.name()));
        entry.paginators = operation.paginated.as_ref().map(|_| {
            vec![
                pages_name(operation.name()),
                pages_async_name(operation.name()),
            ]
        });
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, package, true));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, package, false));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// An input as its builder sets it, or an output as its accessors read it.
fn structure(service: &Service, id: &ShapeId, package: &str, input: bool) -> Structure {
    let style = if input { "builder" } else { "methods" };
    let mut structure = Structure::new(surface::unqualified(&ty(service, id, package), "."), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                let field = member_field(&member.name, package);
                let name = if input {
                    field
                } else {
                    accessor(&field, shape.error.is_some())
                };
                Member::new(
                    &member.name,
                    name,
                    surface::unqualified(&ty(service, &member.target, package), "."),
                )
            })
            .collect();
    }
    structure
}
