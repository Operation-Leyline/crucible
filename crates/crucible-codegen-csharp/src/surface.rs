//! The NuGet package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, async_method_name, client_name, field, method_name, pages_method_name, ty};

/// What the generated package shows its callers: the NuGet package, the
/// client class, and per operation its blocking and asynchronous methods,
/// the page walker, and the models it takes (an object initializer) and
/// returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("new {client}(new ClientConfig())")
    } else {
        format!("new {client}(new ClientConfig {{ Endpoint = endpoint }})")
    };
    let mut out = surface::Surface::new(
        "csharp",
        Package::new("nuget", &options.package_id, &options.namespace),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    for op in &service.operations {
        let mut entry = Operation::new(method_name(op));
        entry.async_method = Some(async_method_name(op));
        entry.paginators = pages_method_name(op).map(|name| vec![name]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, &options.namespace, "fields"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, &options.namespace, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class as `Models/<Name>.cs` declares it: a property per member.
fn structure(service: &Service, id: &ShapeId, ns: &str, style: &str) -> Structure {
    let mut structure = Structure::new(caller_type(&ty(service, id, ns)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    caller_type(&ty(service, &member.target, ns)),
                )
            })
            .collect();
    }
    structure
}

/// A type as a caller writes it with the package's namespaces imported:
/// `global::Ns.Models.@City` reads `City`, `global::System.DateTimeOffset`
/// reads `DateTimeOffset`.
fn caller_type(ty: &str) -> String {
    surface::unqualified(&surface::unqualified(ty, "::"), ".").replace('@', "")
}
