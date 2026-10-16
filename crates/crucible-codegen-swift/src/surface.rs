//! The Swift package's public surface (`crucible.sdks/1`), named by the
//! functions the client class and models are emitted with.

use std::collections::BTreeSet;

use crucible_codegen_common::surface::{self, ClientType, Member, Operation, Package, Structure};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape};

use crate::{
    Options, client_name, field, method_name, model_name, pages_method_name, reserved_module, ty,
};

/// What the generated package shows its callers: the SwiftPM package and
/// module, the client class, and per operation its `async` method, the
/// page sequence, and the models it takes (memberwise initializer labels)
/// and returns (properties).
pub fn surface(service: &Service, options: &Options) -> surface::Surface {
    let client = client_name(service);
    let construct = if options.endpoint.is_some() {
        format!("try {client}()")
    } else {
        format!("try {client}(ClientConfig(endpoint: endpoint))")
    };
    let module = &options.module_name;
    let mut package = Package::new("swiftpm", &options.package_name, module);
    // Blobs are `Foundation.Data`; everything else the module declares.
    package.imports = vec!["import Foundation".to_owned(), format!("import {module}")];
    let mut constructor = ClientType::new(client, construct);
    // `init` throws on an invalid configuration.
    constructor.fallible = true;
    let mut out = surface::Surface::new("swift", package, constructor);
    out.endpoint.clone_from(&options.endpoint);
    let models = model_names(service);
    for op in &service.operations {
        let mut entry = Operation::new(escape(&method_name(op)));
        entry.paginators = pages_method_name(op).map(|name| vec![escape(&name)]);
        entry.input = op
            .input
            .as_ref()
            .map(|id| structure(service, id, module, &models, "keywords"));
        entry.output = op
            .output
            .as_ref()
            .map(|id| structure(service, id, module, &models, "properties"));
        out.operations.insert(op.id.to_string(), entry);
    }
    out
}

/// A model struct as `Models.swift` declares it: a property per member,
/// and an initializer label per member.
fn structure(
    service: &Service,
    id: &ShapeId,
    module: &str,
    models: &BTreeSet<String>,
    style: &str,
) -> Structure {
    let mut structure = Structure::new(caller_type(&ty(service, id, module), models), style);
    if let Some(Shape::Structure(shape)) = service.shape(id) {
        structure.members = shape
            .members
            .iter()
            .map(|member| {
                let mut entry = Member::new(
                    member,
                    escape(&field(&member.name)),
                    caller_type(&ty(service, &member.target, module), models),
                );
                entry.optional = crate::models::presence(member);
                entry
            })
            .collect();
    }
    structure
}

/// The type names the module declares for models, which would make a
/// caller's unqualified `String` ambiguous with `Swift.String`.
fn model_names(service: &Service) -> BTreeSet<String> {
    service
        .shapes
        .keys()
        .filter(|id| !id.is_prelude())
        .map(|id| model_name(service.name_of(id)))
        .collect()
}

/// `name` as Swift source writes it: in backticks when it is a name the
/// generator reserves (Swift's keywords among them), as the generated
/// declarations always are.
fn escape(name: &str) -> String {
    if reserved_module(name) {
        format!("`{name}`")
    } else {
        name.to_owned()
    }
}

/// A type as a caller writes it after the package's imports: `` `City` ``
/// (or a reserved name's `` `TypeModel` ``) reads `City`, `Swift.Int64`
/// reads `Int64` unless a model takes that name, and `Foundation.Data`
/// stays qualified.
fn caller_type(ty: &str, models: &BTreeSet<String>) -> String {
    if let Some(name) = ty.strip_prefix('`').and_then(|rest| rest.strip_suffix('`')) {
        return name.to_owned();
    }
    match ty.strip_prefix("Swift.") {
        Some(name) if !models.contains(name) => name.to_owned(),
        _ => ty.to_owned(),
    }
}
