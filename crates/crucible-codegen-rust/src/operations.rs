//! The top level of the generated file: per operation a marker type, its
//! `OperationInfo`, and the trait the service implements; for the service its
//! `ServiceInfo` and the registration functions.

use crucible_smithy::ir::{Operation, Segment, Service, Shape};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::errors::{error_shapes, info_static};
use crate::info::{constant, custom_traits, members};
use crate::names::{doc, function, literal};

pub(crate) struct Functions {
    pub classify: String,
    pub reject: Option<String>,
    pub internal: Option<String>,
    pub unknown: Option<String>,
}

pub(crate) fn emit(
    service: &Service,
    functions: Functions,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut out = String::new();
    for operation in &service.operations {
        out.push_str(&operation_items(service, operation));
        if function(operation.name()) == "build" {
            diagnostics.push(
                &operation.id,
                "an operation named Build collides with OperationsBuilder::build",
            );
        }
    }
    out.push('\n');
    out.push_str(&functions.classify);
    if let Some(reject) = &functions.reject {
        out.push('\n');
        out.push_str(reject);
    }
    if let Some(internal) = &functions.internal {
        out.push('\n');
        out.push_str(internal);
    }
    if let Some(unknown) = &functions.unknown {
        out.push('\n');
        out.push_str(unknown);
    }
    out.push_str(&service_info(service, &functions));
    out.push_str(&registration(service));
    out
}

fn operation_static(operation: &Operation) -> String {
    format!("{}_OPERATION", constant(operation.name()))
}

pub(crate) fn input_type(service: &Service, operation: &Operation) -> String {
    operation
        .input
        .as_ref()
        .map_or("::crucible::operation::Unit".to_owned(), |id| {
            format!("model::{}", service.name_of(id))
        })
}

pub(crate) fn output_type(service: &Service, operation: &Operation) -> String {
    operation.output.as_ref().map_or("()".to_owned(), |id| {
        format!("model::{}", service.name_of(id))
    })
}

pub(crate) fn structure<'s>(
    service: &'s Service,
    id: Option<&ShapeId>,
) -> Option<&'s crucible_smithy::ir::Structure> {
    match service.shape(id?)? {
        Shape::Structure(structure) => Some(structure),
        _ => None,
    }
}

fn operation_items(service: &Service, op: &Operation) -> String {
    let name = op.name();
    let mut out = marker(service, op, &output_type(service, op));
    doc(&mut out, "", op.docs.as_deref());
    let parameter = if op.input.is_some() {
        format!(", input: {}", input_type(service, op))
    } else {
        String::new()
    };
    out.push_str(&format!(
        "pub trait {name}Operation: ::crucible::WithContext + Send + Sync + 'static {{\n    fn {}(&self{parameter}) -> impl ::std::future::Future<Output = ::crucible::Result<{}>> + Send;\n}}\n",
        function(name),
        output_type(service, op),
    ));
    out
}

/// The marker type, its `OperationInfo` and its `Operation` impl, with
/// `output_ty` as the output type: a client reads a missing output as `Unit`.
pub(crate) fn marker(service: &Service, op: &Operation, output_ty: &str) -> String {
    let name = op.name();
    let mut out = String::from("\n");
    doc(&mut out, "", op.docs.as_deref());
    out.push_str(&format!("pub struct {name};\n\n"));

    let path: Vec<String> = op
        .uri
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Literal(text) => {
                format!("::crucible::operation::Segment::Literal({})", literal(text))
            }
            Segment::Label(label) => {
                format!("::crucible::operation::Segment::Label({})", literal(label))
            }
        })
        .collect();
    let query = op
        .uri
        .query
        .iter()
        .map(|(name, value)| {
            let value = value.as_ref().map_or("None".to_owned(), |value| {
                format!("Some({})", literal(value))
            });
            format!("({}, {value})", literal(name))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let input = structure(service, op.input.as_ref())
        .map_or("&[]".to_owned(), |s| members(service, s, &op.request));
    let output = structure(service, op.output.as_ref())
        .map_or("&[]".to_owned(), |s| members(service, s, &op.response));
    let errors: Vec<String> = op
        .errors
        .iter()
        .map(|id| literal(service.name_of(id)))
        .collect();
    let resource = op.resource.as_ref().map_or("None".to_owned(), |id| {
        format!("Some({})", literal(service.name_of(id)))
    });
    let token = op
        .idempotency_token
        .as_ref()
        .map_or("None".to_owned(), |token| {
            format!("Some({})", literal(token))
        });
    out.push_str(&format!(
        "static {}: ::crucible::OperationInfo = ::crucible::OperationInfo {{\n    id: {},\n    name: {},\n    resource: {resource},\n    method: {},\n    path: &[{}],\n    literal_query: &[{query}],\n    status: {},\n    input: {input},\n    output: {output},\n    errors: &[{}],\n    readonly: {},\n    idempotent: {},\n    idempotency_token: {token},\n    traits: {},\n}};\n\n",
        operation_static(op),
        literal(&op.id.to_string()),
        literal(name),
        literal(&op.method),
        path.join(", "),
        op.status,
        errors.join(", "),
        op.readonly,
        op.idempotent,
        custom_traits(&op.traits),
    ));
    out.push_str(&format!(
        "impl ::crucible::Operation for {name} {{\n    const INFO: &'static ::crucible::OperationInfo = &{};\n    type Input = {};\n    type Output = {};\n}}\n\n",
        operation_static(op),
        input_type(service, op),
        output_ty,
    ));
    out
}

fn service_info(service: &Service, functions: &Functions) -> String {
    let operations: Vec<String> = service
        .operations
        .iter()
        .map(|op| format!("&{}", operation_static(op)))
        .collect();
    let errors: Vec<String> = error_shapes(service)
        .into_iter()
        .map(|(id, _)| format!("&errors::{}", info_static(service.name_of(id))))
        .collect();
    let option = |present: bool, name: &str| {
        if present {
            format!("Some({name})")
        } else {
            "None".to_owned()
        }
    };
    format!(
        "\n/// The service, as the framework sees it.\npub static SERVICE: ::crucible::ServiceInfo = ::crucible::ServiceInfo {{\n    id: {},\n    version: {},\n    operations: &[\n        {},\n    ],\n    errors: &[{}],\n    classify,\n    reject: {},\n    internal: {},\n    unknown: {},\n}};\n",
        literal(&service.id.to_string()),
        literal(&service.version),
        operations.join(",\n        "),
        errors.join(", "),
        option(functions.reject.is_some(), "reject"),
        option(functions.internal.is_some(), "internal"),
        option(functions.unknown.is_some(), "unknown"),
    )
}

fn registration(service: &Service) -> String {
    let mut out = String::from(
        "\n/// Registers implementations one operation at a time, for a service split\n/// across several types. `build` names any operation left out.\npub struct OperationsBuilder(::crucible::operations::OperationsBuilder);\n\nimpl OperationsBuilder {\n",
    );
    for op in &service.operations {
        let method = function(op.name());
        let call = if op.input.is_some() {
            format!(
                "|implementation, input| ::std::boxed::Box::pin(async move {{ implementation.{method}(input).await }})"
            )
        } else {
            format!(
                "|implementation, _| ::std::boxed::Box::pin(async move {{ implementation.{method}().await }})"
            )
        };
        out.push_str(&format!(
            "    /// Serves {name} with `implementation`.\n    pub fn {method}<S: {name}Operation>(self, implementation: ::std::sync::Arc<S>) -> Self {{\n        Self(self.0.add::<{name}, S>(implementation, {call}))\n    }}\n\n",
            name = op.name(),
        ));
    }
    out.push_str(
        "    pub fn build(self) -> ::std::result::Result<::crucible::Operations, ::crucible::MissingOperations> {\n        self.0.build()\n    }\n}\n\n",
    );
    out.push_str("pub fn builder() -> OperationsBuilder {\n    OperationsBuilder(::crucible::Operations::builder(&SERVICE))\n}\n\n");
    let bounds: Vec<String> = service
        .operations
        .iter()
        .map(|op| format!("{}Operation", op.name()))
        .collect();
    let calls: String = service
        .operations
        .iter()
        .map(|op| format!("\n        .{}(implementation.clone())", function(op.name())))
        .collect();
    out.push_str(&format!(
        "/// Every operation, served by one type: checked at compile time.\npub fn operations<T>(implementation: ::std::sync::Arc<T>) -> ::crucible::Operations\nwhere\n    T: {},\n{{\n    builder(){calls}\n        .build()\n        .expect(\"every operation is registered\")\n}}\n",
        bounds.join(" + ")
    ));
    out
}
