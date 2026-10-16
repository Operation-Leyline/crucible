//! The package's public surface (`crucible.sdks/1`), named by the functions
//! `client.dart` and `models.dart` are emitted with.

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, operation_method, pages_method, ty};

/// What the generated pub package shows its callers: the package and its
/// library, the client class, and per operation its method, its `Pages`
/// stream, and the model classes it takes (named constructor arguments)
/// and returns (fields and getters).
///
/// The package library exports the client, the models, `ClientConfig` and
/// the runtime value types (`Timestamp`, `JsonValue`, `BigDecimal`) bare,
/// so its one import brings every name in unqualified; only a blob's
/// `Uint8List` needs `dart:typed_data` as well.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}(ClientConfig(endpoint: endpoint))")
    };
    let package_name = &options.package_name;
    let import = format!("package:{package_name}/{package_name}.dart");
    let mut package = Package::new("pub", package_name, &import);
    package.imports = vec![format!("import '{import}';")];
    let mut out = surface::Surface::new("dart", package, ClientType::new(client, construct));
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
    let blobs = out
        .operations
        .values()
        .flat_map(|op| op.input.iter().chain(&op.output))
        .flat_map(|structure| &structure.members)
        .any(|member| member.type_name.contains("Uint8List"));
    if blobs {
        out.package
            .imports
            .push("import 'dart:typed_data';".to_owned());
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
                    member,
                    field(&member.name),
                    // `core.`, `typed.`, `json.` and `stamp.` are the
                    // generated files' import aliases: `dart:core` is
                    // implicit, `dart:typed_data` is imported above when a
                    // blob needs it, and the package exports the rest bare.
                    surface::unqualified(&ty(service, &member.target), "."),
                )
            })
            .collect();
    }
    structure
}
