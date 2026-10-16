//! The Cargo SDK's public surface (`crucible.sdks/1`), named by the
//! allocators the client methods and model builders are emitted with.

use crucible_codegen_common::surface::{self, Client, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::builders::{fallible, getter_names, setter_names};
use crate::client::{client_name, method_names};
use crate::names::function;
use crate::types::rust_type;
use crate::{ClientOptions, client_package_name};

/// What the generated Cargo package shows its callers: the crate, the
/// client struct, and per operation its methods and the models it takes
/// (through `Model::builder()`) and returns (through accessors).
pub fn surface(service: &Service, options: &ClientOptions) -> surface::Surface {
    let package = client_package_name(service, options);
    let client = client_name(service);
    let mut out = surface::Surface::new(
        "rust",
        Package::new("cargo", &package, package.replace('-', "_")),
        Client::new(&client, format!("{client}::new(Config::new(endpoint))")),
    );
    for (operation, derived) in service.operations.iter().zip(method_names(service)) {
        let mut entry = Operation::new(function(operation.name()));
        entry.options_method = Some(derived.options);
        entry.paginators = derived.pages.map(|pages| vec![pages]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, true));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, false));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// An input as its builder sets it, or an output as its accessors read it.
fn structure(service: &Service, id: &ShapeId, input: bool) -> Structure {
    let style = if input { "builder" } else { "methods" };
    let mut structure = Structure::new(service.name_of(id), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        let names = if input {
            setter_names(shape)
        } else {
            getter_names(shape)
        };
        structure.fallible = input && fallible(shape);
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    &member.name,
                    names[&member.name].clone(),
                    surface::unqualified(&rust_type(service, &member.target, ""), "::"),
                )
            })
            .collect();
    }
    structure
}
