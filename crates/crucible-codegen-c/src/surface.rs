//! The CMake package's public surface (`crucible.sdks/1`), named by the
//! functions the client and model symbols are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::client::{client_type, function, pages};
use crate::models::{helper, native_name, slot_type};
use crate::{Options, names};

/// What the generated C package shows its callers: the package and
/// header, the runtime client type and its constructor, and per operation
/// its functions and the model structs it takes (through `set_` functions)
/// and returns (through `get_` functions). Every symbol carries the
/// prefix, so `#include "pkg/client.h"` brings them all in unqualified.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let p = &options.prefix;
    let package = &options.package_name;
    let client = client_type(p);
    let endpoint = if options.endpoint.is_some() {
        String::new()
    } else {
        format!(" config.endpoint = {p}_string_view(endpoint);")
    };
    let construct = format!(
        "{client}_config config = {client}_defaults();{endpoint} {client} *client = NULL; {p}_error error = {{0}}; {client}_new(&config, &client, &error);"
    );
    let header = format!("{package}/client.h");
    let mut package = Package::new("cmake", package, &header);
    package.imports = vec![format!("#include \"{header}\"")];
    // Constructing is statements, and reports failure through `error`.
    let mut client = ClientType::new(client, construct);
    client.statements = true;
    client.fallible = true;
    let mut out = surface::Surface::new("c", package, client);
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(function(operation, p));
        entry.paginators = pages(operation, p).map(Vec::from);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, p, "setters", "set"));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, p, "functions", "get"));
        let call = crucible_codegen_common::snippet::call(service, operation);
        (entry.snippet, entry.paginated_snippet) =
            crate::snippet::snippets(service, options, &call);
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// A model struct and, per member, its `set_` or `get_` function, typed as
/// the function takes or returns the value.
fn structure(service: &Service, id: &ShapeId, p: &str, style: &str, accessor: &str) -> Structure {
    let native = native_name(service, id, p);
    let mut structure = Structure::new(&native, style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    member,
                    helper(&native, accessor, &names::field(&member.name)),
                    slot_type(service, &member.target, p),
                    member.optional_on_client(),
                )
            })
            .collect();
    }
    structure
}
