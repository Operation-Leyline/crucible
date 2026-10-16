//! The Cargo SDK's public surface (`crucible.sdks/1`), named by the
//! allocators the client methods and model builders are emitted with.

use std::collections::BTreeSet;

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::builders::{fallible, getter_names, required_to_build, setter_names};
use crate::client::{client_name, method_names};
use crate::names::function;
use crate::types::{boxed_members, public_builtin};
use crate::{ClientOptions, client_package_name};

/// What the generated Cargo package shows its callers: the crate, the
/// client struct, and per operation its methods and the models it takes
/// (through `Model::builder()`) and returns (through accessors).
pub fn surface(service: &Service, options: &ClientOptions) -> surface::Surface {
    let package = client_package_name(service, options);
    let client = client_name(service);
    let crate_name = package.replace('-', "_");
    let mut package = Package::new("cargo", &package, &crate_name);
    // The crate root holds the client, the models and `runtime`.
    package.imports = vec![format!("use {crate_name}::*;")];
    let mut out = surface::Surface::new(
        "rust",
        package,
        ClientType::new(
            &client,
            format!("{client}::new(runtime::Config::new(endpoint))"),
        ),
    );
    if options.content_sha256 {
        out.middleware = vec![surface::Middleware::content_sha256()];
    }
    let boxed = boxed_members(service);
    for (operation, derived) in service.operations.iter().zip(method_names(service)) {
        let mut entry = Operation::new(function(operation.name()));
        entry.options_method = Some(derived.options);
        entry.paginators = derived.pages.map(|pages| vec![pages]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, &boxed, true));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, &boxed, false));
        let call = crucible_codegen_common::snippet::call(service, operation);
        (entry.snippet, entry.paginated_snippet) = crate::snippet::snippets(
            service,
            options,
            &out.middleware,
            &call,
            &entry.method,
            entry
                .paginators
                .as_ref()
                .and_then(|pages| pages.first())
                .map(String::as_str),
        );
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// `target`'s type as a caller writes it after `use sdk::*;`.
fn caller_type(service: &Service, target: &ShapeId) -> String {
    if target.is_prelude()
        && let Some(Shape::Simple(simple)) = service.shape(target)
    {
        return public_builtin(simple.kind).to_owned();
    }
    service.name_of(target).to_owned()
}

/// An input as its builder sets it, or an output as its accessors read it.
/// A recursive member is boxed, as the model's field is.
fn structure(service: &Service, id: &ShapeId, boxed: &BTreeSet<ShapeId>, input: bool) -> Structure {
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
                let mut ty = caller_type(service, &member.target);
                if boxed.contains(&id.with_member(&member.name)) {
                    ty = format!("Box<{ty}>");
                }
                // A builder may leave unset what `build` does not need; an
                // accessor returns an `Option` for what may be absent.
                let optional = if input {
                    !required_to_build(member)
                } else {
                    member.optional_on_client()
                };
                Member::new(member, names[&member.name].clone(), ty, optional)
            })
            .collect();
    }
    structure
}
