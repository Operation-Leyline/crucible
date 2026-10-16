//! The module's public surface (`crucible.sdks/1`), named by the functions
//! `client.go` and `models.go` are emitted with.

use std::collections::BTreeSet;

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{
    Options, absent, client_name, exported, field_type, method_name, pages_name, type_name,
};

/// What the generated module shows its callers: the module path, the
/// client struct, and per operation its method, paginator, and the
/// structs it takes (field literals) and returns (fields).
///
/// A snippet imports the module under its package name, so the module's
/// declarations read `weather.ListCitiesInput`; a standard-library type a
/// member has (`time.Time`, `*big.Int`) adds its import. A package named
/// like one of those (`time`, `big`) is imported under `{name}sdk`, so the
/// two never share a name in the snippet.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let package = &snippet_alias(&options.package_name);
    let qualifier = format!("{package}.");
    let client = format!("{qualifier}{}", client_name(service));
    let config = if options.endpoint.is_some() {
        format!("{qualifier}Config{{}}")
    } else {
        format!("{qualifier}Config{{Endpoint: endpoint}}")
    };
    // `New{Client}` returns `(*{Client}, error)`.
    let mut constructor = ClientType::new(
        &client,
        format!("{package}.New{}({config})", client_name(service)),
    );
    constructor.fallible = true;
    let mut out = surface::Surface::new(
        "go",
        Package::new("go", &options.module_path, &options.module_path),
        constructor,
    );
    out.package.qualifier.clone_from(&qualifier);
    out.endpoint.clone_from(&options.endpoint);
    let mut platform = BTreeSet::new();
    for operation in &service.operations {
        let mut entry = Operation::new(method_name(operation.name()));
        entry.paginators = operation
            .paginated
            .as_ref()
            .map(|_| vec![pages_name(operation.name())]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, &qualifier, &mut platform));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, &qualifier, &mut platform));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out.package.imports = std::iter::once(format!(
        "import {package} {}",
        crucible_codegen_common::quoted(&options.module_path)
    ))
    .chain(platform)
    .collect();
    out
}

/// A struct as `models.go` declares it: an exported field per member, of
/// the field's own type (an optional member's is `Optional[T]`, set with
/// `weather.Some(value)`).
fn structure(
    service: &Service,
    id: &ShapeId,
    qualifier: &str,
    platform: &mut BTreeSet<String>,
) -> Structure {
    let mut structure = Structure::new(format!("{qualifier}{}", type_name(service, id)), "fields");
    if let Some(shape @ Shape::Structure(fields)) = service.shape(id) {
        structure.members = fields
            .members
            .iter()
            .map(|member| {
                let native = field_type(service, shape, member, qualifier);
                for (name, import) in PLATFORM {
                    if names_package(&native, &format!("{name}.")) {
                        platform.insert((*import).to_owned());
                    }
                }
                let optional = absent(shape, member);
                Member::new(member, exported(&member.name), native, optional)
            })
            .collect();
    }
    structure
}

/// The standard-library packages a member type may name, by the name a
/// snippet's import gives them.
const PLATFORM: &[(&str, &str)] = &[("time", "import \"time\""), ("big", "import \"math/big\"")];

/// The name a snippet imports the module under: its package name, unless a
/// standard-library import the surface may add would take it.
fn snippet_alias(package: &str) -> String {
    if PLATFORM.iter().any(|(name, _)| *name == package) {
        format!("{package}sdk")
    } else {
        package.to_owned()
    }
}

/// Whether the Go type `native` names a standard-library package `uses`
/// (`time.`): at the start of an identifier, not inside a longer one.
fn names_package(native: &str, uses: &str) -> bool {
    native.match_indices(uses).any(|(at, _)| {
        !native[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
    })
}
