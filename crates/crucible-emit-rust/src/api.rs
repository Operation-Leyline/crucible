//! `api.rs`: the traits a service implements, one per resource and one for
//! the operations the service binds directly. A large service splits into
//! one `impl` per resource instead of one impl of every operation.

use std::collections::BTreeMap;

use crucible_core::ir::{Operation, Service};
use crucible_core::{Diagnostics, ShapeId};

use crate::names::{doc, function};

/// Traits by name, each with its operations, in name order.
pub(crate) fn traits(service: &Service) -> BTreeMap<String, Vec<&Operation>> {
    let mut traits: BTreeMap<String, Vec<&Operation>> = BTreeMap::new();
    for operation in &service.operations {
        traits
            .entry(trait_name(service, operation.resource.as_ref()))
            .or_default()
            .push(operation);
    }
    traits
}

fn trait_name(service: &Service, resource: Option<&ShapeId>) -> String {
    match resource {
        Some(resource) => format!("{}Api", service.name_of(resource)),
        None => format!("{}Api", service.id.name()),
    }
}

pub(crate) fn emit(
    service: &Service,
    validation: &ShapeId,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::from(crate::HEADER);
    out.push_str("use crate::runtime::{Context, Rejection};\n");
    let error = format!("crate::errors::{}", service.name_of(validation));
    let reject = match crate::errors::default_rejection(service, validation) {
        Some(body) => format!(
            "    /// The default fills `message` and `fieldList`; override it to choose more.\n    fn reject(&self, rejection: Rejection) -> {error} {{\n        {body}\n    }}\n"
        ),
        None => format!(
            "    /// crucible cannot fill every required member of this error, so the\n    /// service builds it. `Violation::kind` tells a malformed request from\n    /// one that broke a constraint.\n    fn reject(&self, rejection: Rejection) -> {error};\n"
        ),
    };
    out.push_str(&format!(
        "\n/// How a request that broke the model is answered: as the service's\n/// validation error, built from every violation found.\npub trait Reject: Send + Sync {{\n{reject}}}\n"
    ));
    for (name, operations) in traits(service) {
        if service.shapes.keys().any(|id| service.name_of(id) == name) {
            diagnostics.push(
                &service.id,
                format!("the trait `{name}` collides with a shape of the same name"),
            );
        }
        let what = match operations[0].resource.as_ref() {
            Some(resource) => format!("Operations on the {} resource.", service.name_of(resource)),
            None => "Operations the service binds directly.".to_owned(),
        };
        out.push_str(&format!("\n/// {what}\npub trait {name}: Send + Sync {{\n"));
        for (index, operation) in operations.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            doc(&mut out, "    ", operation.docs.as_deref());
            out.push_str(&format!("    {};\n", signature(service, operation)));
        }
        out.push_str("}\n");
    }
    out
}

/// `fn create_city(&self, ctx, input) -> impl Future<Output = Result<..>> + Send`.
/// An implementation writes it as an `async fn`.
fn signature(service: &Service, operation: &Operation) -> String {
    let input = operation
        .input
        .as_ref()
        .map(|id| format!(",\n        input: crate::models::{}", service.name_of(id)))
        .unwrap_or_default();
    let output = operation.output.as_ref().map_or("()".to_owned(), |id| {
        format!("crate::models::{}", service.name_of(id))
    });
    format!(
        "fn {}(\n        &self,\n        ctx: &Context{input},\n    ) -> impl ::std::future::Future<Output = ::std::result::Result<{output}, crate::errors::{}Error>> + Send",
        function(operation.name()),
        operation.name()
    )
}
