//! Flat public SDK types; serde and wire implementation modules stay private.
use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::{client, errors, models, names};

pub(crate) fn declared_names(service: &Service) -> BTreeSet<String> {
    service
        .operations
        .iter()
        .flat_map(|op| [op.name().to_owned(), client::error_type(op)])
        .chain(shape_names(service).map(|(_, id)| service.name_of(id).to_owned()))
        .chain([
            format!("{}Client", names::variant(service.id.name())),
            names::variant(&crucible_codegen_common::service_exception_name(service)),
        ])
        .collect()
}

pub(crate) fn exports(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let mut declared = BTreeMap::new();
    for reserved in [
        "model",
        "errors",
        "__redaction",
        "runtime",
        "__CRUCIBLE_CLIENT_METADATA",
    ] {
        declared.insert(reserved.to_owned(), service.id.clone());
    }
    let mut register = |name: String, id: &ShapeId| {
        if let Some(previous) = declared.insert(name.clone(), id.clone()) {
            diagnostics.push(
                id,
                format!("client API name `{name}` collides with `{previous}` in the SDK namespace"),
            );
        }
    };
    register(
        format!("{}Client", names::variant(service.id.name())),
        &service.id,
    );
    register(
        names::variant(&crucible_codegen_common::service_exception_name(service)),
        &service.id,
    );
    for op in &service.operations {
        register(op.name().to_owned(), &op.id);
        register(client::error_type(op), &op.id);
        register(
            format!("{}_OPERATION", crate::info::constant(op.name())),
            &op.id,
        );
        register(format!("__crucible_metadata_{}", op.name()), &op.id);
    }
    let mut exports: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (module, id) in shape_names(service) {
        let name = service.name_of(id).to_owned();
        register(name.clone(), id);
        exports.entry(module).or_default().push(name);
    }
    let mut out = String::new();
    for (module, mut names) in exports {
        names.sort();
        out.push_str(&format!(
            "#[doc(inline)]\n#[allow(unused_imports)]\npub use {module}::{{{}}};\n",
            names.join(", ")
        ));
    }
    out
}

fn shape_names(service: &Service) -> impl Iterator<Item = (&'static str, &ShapeId)> {
    models::model_shapes(service)
        .into_iter()
        .filter(|(_, shape)| !matches!(shape, Shape::Unit { .. }))
        .map(|(id, _)| ("model", id))
        .chain(
            errors::error_shapes(service)
                .into_iter()
                .map(|(id, _)| ("errors", id)),
        )
}
