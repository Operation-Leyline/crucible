//! The Kotlin package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use std::collections::BTreeSet;

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{Options, client_name, field, identifier, keyword, method_name, pages_method_name, ty};

/// What the generated package shows its callers: the Maven coordinate, the
/// client class, and per operation its `suspend` method, the page flow,
/// and the models it takes (a builder DSL block) and returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let package = &options.package_name;
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("{client}(ClientConfig())")
    } else {
        format!("{client}(ClientConfig(endpoint = endpoint))")
    };
    let mut coordinate = Package::new(
        "maven",
        format!("{}:{}", options.group_id, options.artifact_id),
        package,
    );
    // The client and its configuration sit in the package, the models in
    // `.model`: imported by name and by star, both resolve unqualified.
    coordinate.imports = vec![
        format!("import {package}.{client}"),
        format!("import {package}.ClientConfig"),
        format!("import {package}.model.*"),
    ];
    let mut out = surface::Surface::new("kotlin", coordinate, ClientType::new(&client, construct));
    out.endpoint.clone_from(&options.endpoint);
    let models = model_names(service);
    for op in &service.operations {
        let mut entry = Operation::new(escape(&method_name(op)));
        entry.paginators = pages_method_name(op).map(|name| vec![escape(&name)]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, package, &models, "dsl"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, package, &models, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model class as `model/<Name>.kt` declares it: a property per member,
/// set through its `Builder` in a DSL block.
fn structure(
    service: &Service,
    id: &ShapeId,
    package: &str,
    models: &BTreeSet<&str>,
    style: &str,
) -> Structure {
    let mut structure = Structure::new(
        caller_type(&ty(service, id, package), package, models),
        style,
    );
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                // A builder may leave out what `build()` defaults; a model
                // lacks only what is an `OptionalValue`.
                let optional = if style == "dsl" {
                    crate::models::omittable(member)
                } else {
                    crate::models::optional_value(member)
                };
                Member::new(
                    member,
                    escape(&field(&member.name)),
                    caller_type(&ty(service, &member.target, package), package, models),
                    optional,
                )
            })
            .collect();
    }
    structure
}

/// The names the package's star-imported models take, which shadow
/// Kotlin's default imports (`kotlin.String`) in a caller's file.
fn model_names(service: &Service) -> BTreeSet<&str> {
    service
        .shapes
        .keys()
        .filter(|id| !id.is_prelude())
        .map(|id| service.name_of(id))
        .collect()
}

/// `name` as Kotlin source writes it: in backticks when it is a keyword or
/// not a plain identifier, as the generated declarations always are.
fn escape(name: &str) -> String {
    if keyword(name) || !identifier(name) {
        format!("`{name}`")
    } else {
        name.to_owned()
    }
}

/// A type as a caller writes it after the package's imports:
/// ``example.weather.model.`City` `` reads `City` (`` `class` `` for a
/// keyword); a default-imported `kotlin.String` reads `String` unless a
/// model takes that name; other platform types stay qualified, as emitted.
fn caller_type(ty: &str, package: &str, models: &BTreeSet<&str>) -> String {
    if let Some(name) = ty
        .strip_prefix(&format!("{package}.model.`"))
        .and_then(|rest| rest.strip_suffix('`'))
    {
        return escape(name);
    }
    match ty.strip_prefix("kotlin.") {
        Some(name) if !name.contains('.') && !models.contains(name) => name.to_owned(),
        _ => ty.to_owned(),
    }
}
