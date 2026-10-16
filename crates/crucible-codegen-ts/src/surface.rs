//! The client's public surface (`crucible.sdks/1`), named by the functions
//! `client.ts` and `models.ts` are emitted with.

use crucible_codegen_common::surface::{
    self, Client, Member, Middleware, Operation, Package, Structure,
};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::Options;
use crate::client::method_names;
use crate::names::{property, ts_type};

/// What the generated package shows its callers: the npm package, the
/// client class, and per operation its method, paginator, and the
/// interfaces it takes and returns.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = crate::client_name(service, options);
    let construct = if options.endpoint.is_some() {
        format!("new {client}()")
    } else {
        format!("new {client}({{ endpoint }})")
    };
    let mut out = surface::Surface::new(
        "ts",
        Package::new("npm", &options.package_name, &options.package_name),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    out.middleware = options
        .middleware
        .iter()
        .map(|middleware| Middleware::new(middleware.name(), middleware.headers()))
        .collect();
    let names = method_names(service);
    for operation in &service.operations {
        let (method, paginator) = &names[&operation.id];
        let mut entry = Operation::new(method);
        entry.paginators = paginator.clone().map(|name| vec![name]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, "object"));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, "properties"));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// An interface as `models.ts` declares it: a property per member.
fn structure(service: &Service, id: &ShapeId, style: &str) -> Structure {
    let mut structure = Structure::new(service.name_of(id), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    property(&member.name),
                    // `globalThis.` guards against a model's own `Date`; a
                    // caller writes the global's name.
                    surface::unqualified(&ts_type(service, &member.target, ""), "."),
                )
            })
            .collect();
    }
    structure
}
