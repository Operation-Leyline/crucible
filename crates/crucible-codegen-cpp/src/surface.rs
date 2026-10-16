//! The CMake package's public surface (`crucible.sdks/1`), named by the
//! functions the client and models are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::Options;
use crate::client::{client_name, method, pages};
use crate::models::Layout;
use crate::names;

/// What the generated CMake package shows its callers: the package and
/// header, the client class, and per operation its methods and the model
/// structs it takes (constructor arguments) and returns (public fields).
///
/// After `#include "pkg/client.hpp"`, the package's declarations are
/// spelled from its namespace down (`example::weather::WeatherClient`,
/// `example::weather::models::CreateCityInput`,
/// `example::weather::runtime::Timestamp`) and the standard library's from
/// `std::`, as callers write them.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let layout = Layout::new(service, options);
    let ns = &options.namespace;
    let client = format!("{ns}::{}", client_name(service));
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}({ns}::runtime::ClientConfig{{.endpoint = endpoint}})")
    };
    let header = format!("{}/client.hpp", options.package_name);
    let mut package = Package::new("cmake", &options.package_name, &header);
    package.imports = vec![format!("#include \"{header}\"")];
    package.qualifier = format!("{ns}::");
    let mut out = surface::Surface::new("cpp", package, ClientType::new(client, construct));
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method(operation));
        entry.paginators = pages(operation).map(|pages| vec![pages]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, &layout, options, &operation.id, id, "positional"));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, &layout, options, &operation.id, id, "fields"));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out
}

/// `Layout::ty`'s spelling, which names the package's declarations from
/// the global namespace (`::example::weather::models::City`), as a caller
/// writes it: without the leading `::`. `std::` stays.
fn caller_type(options: &Options, ty: &str) -> String {
    let ns = &options.namespace;
    ty.replace(&format!("::{ns}::"), &format!("{ns}::"))
}

/// A model struct. An input is built with its explicit constructor, whose
/// arguments are the required members and then the optional ones, each in
/// model order (`Member::optional` tells them apart); an output is read
/// through its public fields. A member's type is its value type, since an
/// optional field's `runtime::Field<T>` converts from a `T`.
fn structure(
    service: &Service,
    layout: &Layout<'_>,
    options: &Options,
    owner: &ShapeId,
    id: &ShapeId,
    style: &str,
) -> Structure {
    let mut structure = Structure::new(caller_type(options, &layout.ty(owner, id)), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                Member::new(
                    member,
                    names::field(&member.name),
                    caller_type(options, &layout.ty(id, &member.target)),
                )
            })
            .collect();
    }
    structure
}
