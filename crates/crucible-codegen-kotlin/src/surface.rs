//! The Kotlin package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, method_name, pages_method_name, ty};

/// What the generated package shows its callers: the Maven coordinate, the
/// client class, and per operation its `suspend` method, the page flow,
/// and the models it takes (a builder DSL block) and returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}(ClientConfig())")
    } else {
        format!("{client}(ClientConfig(endpoint = endpoint))")
    };
    let mut out = surface::Surface::new(
        "kotlin",
        Package::new(
            "maven",
            format!("{}:{}", options.group_id, options.artifact_id),
            &options.package_name,
        ),
        Client::new(client, construct),
    );
    out.endpoint.clone_from(&options.endpoint);
    let package = &options.package_name;
    for op in &service.operations {
        let mut entry = Operation::new(method_name(op));
        entry.paginators = pages_method_name(op).map(|name| vec![name]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, package, "dsl"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, package, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class as `model/<Name>.kt` declares it: a property per member,
/// set through its `Builder` in a DSL block.
fn structure(service: &Service, id: &ShapeId, package: &str, style: &str) -> Structure {
    let mut structure = Structure::new(caller_type(&ty(service, id, package)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    field(&member.name),
                    caller_type(&ty(service, &member.target, package)),
                )
            })
            .collect();
    }
    structure
}

/// A type as a caller writes it with the package imported:
/// ``com.example.model.`City` `` reads `City`, `kotlin.time.Instant` reads
/// `Instant`.
fn caller_type(ty: &str) -> String {
    surface::unqualified(ty, ".").replace('`', "")
}
