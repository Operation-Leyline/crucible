//! The package's public surface (`crucible.sdks/1`), named by the functions
//! `client.py` and `models.py` are emitted with.

use std::collections::BTreeSet;

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::names::{field, ty};
use crate::{Options, client_name, method_name, pages_name};

/// What the generated package shows its callers: the distribution and
/// module, the client class, and per operation its method, paginator, and
/// the dataclasses it takes (keyword arguments) and returns (attributes).
///
/// A snippet imports the module itself, so the package's declarations read
/// `weather_client.WeatherClient`, and the standard-library types a member
/// can have (`datetime`, `Decimal`, `Any`) are imported by name.
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let qualifier = format!("{}.", options.module_name);
    let client = format!("{qualifier}{}", client_name(service));
    let construct = if options.endpoint.is_some() {
        format!("{client}()")
    } else {
        format!("{client}(endpoint=endpoint)")
    };
    let mut package = Package::new("pypi", &options.package_name, &options.module_name);
    package.qualifier.clone_from(&qualifier);
    let mut platform = BTreeSet::new();
    let mut out = surface::Surface::new("python", package, ClientType::new(client, construct));
    out.endpoint.clone_from(&options.endpoint);
    for operation in &service.operations {
        let mut entry = Operation::new(method_name(operation.name()));
        entry.paginators = operation
            .paginated
            .as_ref()
            .map(|_| vec![pages_name(operation.name())]);
        entry.input = operation
            .input
            .as_ref()
            .map(|id| structure(service, id, "keywords", &qualifier, &mut platform));
        entry.output = operation
            .output
            .as_ref()
            .map(|id| structure(service, id, "properties", &qualifier, &mut platform));
        out.operations.insert(operation.id.to_string(), entry);
    }
    out.package.imports = std::iter::once(format!("import {}", options.module_name))
        .chain(platform)
        .collect();
    out
}

/// A dataclass as `models.py` declares it: an attribute per member. Every
/// standard-library type a member names adds its import to `platform`.
fn structure(
    service: &Service,
    id: &ShapeId,
    style: &str,
    qualifier: &str,
    platform: &mut BTreeSet<String>,
) -> Structure {
    let mut structure = Structure::new(ty(service, id, qualifier), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                let native = ty(service, &member.target, qualifier);
                if let Some(import) = import(&native) {
                    platform.insert(import.to_owned());
                }
                Member::new(member, field(&member.name), native)
            })
            .collect();
    }
    structure
}

/// The import a standard-library type `ty` returns needs, if any.
fn import(native: &str) -> Option<&'static str> {
    match native {
        "datetime" => Some("from datetime import datetime"),
        "Decimal" => Some("from decimal import Decimal"),
        "Any" => Some("from typing import Any"),
        _ => None,
    }
}
