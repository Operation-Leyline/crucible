//! Stubs for a service's operation files: one file per operation, in one
//! folder per resource, all under one `operations` folder.
//!
//! ```text
//! operations/
//!   mod.rs                 the service type, and what operations share
//!   service/               operations bound to the service itself
//!     get_me.rs
//!   graph/                 one folder per resource: snake_case of its name
//!     mod.rs
//!     create_graph.rs      impl CreateGraphOperation for Service
//! ```
//!
//! This only plans the files; writing them, and never overwriting one that
//! exists, is the caller's job (`crucible scaffold`).

use std::path::PathBuf;

use crucible_smithy::ir::{Operation, Service};

use crate::names::{function, snake};
use crate::operations::{input_type, output_type};

/// Where the generated contract and the implementing type live, as paths
/// the stubs `use`.
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    /// The module `crucible::include_service!()` is included in.
    pub contract: String,
    /// The type that implements every operation.
    pub service_type: String,
}

impl Default for ScaffoldOptions {
    fn default() -> Self {
        Self {
            contract: "crate::contract".to_owned(),
            service_type: "crate::operations::Service".to_owned(),
        }
    }
}

/// One operation's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    /// The folder under `operations/`: `service`, or the resource's.
    pub folder: String,
    /// The module (and file stem): the operation's name in snake_case.
    pub module: String,
    pub contents: String,
}

impl Stub {
    /// The file, relative to the `operations` folder.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.folder).join(format!("{}.rs", self.module))
    }
}

/// The folder an operation's file goes in: its binding resource's name in
/// snake_case, or `service` for an operation the service binds.
pub fn folder(operation: &Operation) -> String {
    operation
        .resource
        .as_ref()
        .map_or_else(|| "service".to_owned(), |id| snake(id.name()))
}

/// A stub for every operation, in model order.
pub fn scaffold(service: &Service, options: &ScaffoldOptions) -> Vec<Stub> {
    service
        .operations
        .iter()
        .map(|operation| Stub {
            folder: folder(operation),
            module: snake(operation.name()),
            contents: stub(service, operation, options),
        })
        .collect()
}

fn stub(service: &Service, operation: &Operation, options: &ScaffoldOptions) -> String {
    let name = operation.name();
    let method = function(name);
    let output = output_type(service, operation);
    let summary = operation
        .docs
        .as_deref()
        .and_then(|docs| docs.lines().map(str::trim).find(|line| !line.is_empty()))
        .map(|line| format!(": {}", line.trim_end_matches('.')))
        .unwrap_or_default();
    let input = operation
        .input
        .is_some()
        .then(|| input_type(service, operation));
    let (parameter, unused) = match &input {
        Some(input) => (format!(", input: {input}"), "        let _ = input;\n"),
        None => (String::new(), ""),
    };
    // `model` only when a signature names a model type.
    let uses_model = input
        .iter()
        .chain([&output])
        .any(|ty| ty.starts_with("model::"));
    let imports = if uses_model {
        format!("{{model, {name}Operation}}")
    } else {
        format!("{name}Operation")
    };
    format!(
        "//! {name}{summary}.\n\nuse {contract}::{imports};\nuse {service_type};\n\nimpl {name}Operation for {service_name} {{\n    async fn {method}(&self{parameter}) -> crucible::Result<{output}> {{\n{unused}        todo!(\"{name}\")\n    }}\n}}\n",
        contract = options.contract,
        service_type = options.service_type,
        service_name = options
            .service_type
            .rsplit("::")
            .next()
            .unwrap_or(&options.service_type),
    )
}
