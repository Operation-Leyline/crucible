//! The package's public surface (`crucible.sdks/1`), named by the functions
//! `client.dart` and `models.dart` are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, operation_method, pages_method, ty};

/// What the generated pub package shows its callers: the package and its
/// library, the client class, and per operation its method, its `Pages`
/// stream, and the model classes it takes (named constructor arguments)
/// and returns (fields and getters).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}(ClientConfig(endpoint: endpoint))")
    };
    let package = &options.package_name;
    let mut out = surface::Surface::new(
        "dart",
        Package::new("pub", package, format!("package:{package}/{package}.dart")),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for op in &service.operations {
        let method = operation_method(op);
        let mut entry = Operation::new(&method);
        entry.paginators = op.paginated.as_ref().map(|_| vec![pages_method(&method)]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, "keywords"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class: a named constructor parameter and a field per member.
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
                    // `core.`, `typed.` and the runtime prefixes are the
                    // generated files' import aliases; the package exports
                    // the names bare.
                    surface::unqualified(&ty(service, &member.target), "."),
                )
            })
            .collect();
    }
    structure
}
