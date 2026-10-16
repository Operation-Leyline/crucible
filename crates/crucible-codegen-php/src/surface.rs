//! The package's public surface (`crucible.sdks/1`), named by the functions
//! the client and model classes are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::names::{field, ty};
use crate::{Options, client_name, operation_method, pages_method};

/// What the generated Composer package shows its callers: the package and
/// its namespace, the client class, and per operation its method, its
/// `Pages` generator, and the model classes it takes (named constructor
/// arguments) and returns (readonly properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let namespace = &options.namespace;
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("new \\{namespace}\\{client}()")
    } else {
        format!(
            "new \\{namespace}\\{client}(new \\{namespace}\\Runtime\\ClientConfig(endpoint: $endpoint))"
        )
    };
    let mut out = surface::Surface::new(
        "php",
        Package::new("composer", &options.package_name, namespace),
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
            .map(|id| structure(service, namespace, id, "keywords"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, namespace, id, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class: a promoted constructor parameter per member.
fn structure(service: &Service, namespace: &str, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(service.name_of(id), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    // The docblock spelling: `list<City>` says more than
                    // the declared `array`.
                    surface::unqualified(&ty(service, &member.target, namespace, true), "\\"),
                )
            })
            .collect();
    }
    structure
}
