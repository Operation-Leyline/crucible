//! The public server namespace; implementation modules stay private.
use std::collections::BTreeMap;

use crucible_ir::ir::{Service, Shape};
use crucible_ir::{Diagnostics, ShapeId};

use crate::{errors, models, names};

pub(crate) fn exports(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let mut names: BTreeMap<String, ShapeId> = BTreeMap::new();
    for reserved in [
        "model",
        "errors",
        "__redaction",
        "OperationsBuilder",
        "SERVICE",
        "builder",
        "operations",
        "classify",
        "reject",
        "internal",
        "unknown",
    ] {
        names.insert(reserved.to_owned(), service.id.clone());
    }
    if !service.versions.is_empty() {
        names.insert("versions".to_owned(), service.id.clone());
    }
    if !service.permissions.is_empty() {
        names.insert("permissions".to_owned(), service.id.clone());
    }
    let mut register = |name: String, id: &ShapeId| {
        if let Some(previous) = names.insert(name.clone(), id.clone()) {
            diagnostics.push(
                id,
                format!(
                    "server API name `{name}` collides with `{previous}` in the service namespace"
                ),
            );
        }
    };
    for operation in &service.operations {
        register(operation.name().to_owned(), &operation.id);
        register(format!("{}Operation", operation.name()), &operation.id);
        register(
            format!("{}_OPERATION", crate::info::constant(operation.name())),
            &operation.id,
        );
    }
    let mut out = String::new();
    for (module, ids) in [
        (
            "model",
            models::model_shapes(service)
                .into_iter()
                .filter(|(_, shape)| !matches!(shape, Shape::Unit { .. }))
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
        ),
        (
            "errors",
            errors::error_shapes(service)
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
        ),
    ] {
        let mut exports = Vec::new();
        for id in ids {
            let name = service.name_of(id).to_owned();
            register(name.clone(), id);
            exports.push(name);
        }
        exports.sort();
        if !exports.is_empty() {
            out.push_str(&format!(
                "#[doc(inline)]\n#[allow(unused_imports, deprecated)]\npub use {module}::{{{}}};\n",
                exports.join(", ")
            ));
        }
    }
    out
}

pub(crate) fn module(service: &Service) -> String {
    names::module(service.id.name())
}
