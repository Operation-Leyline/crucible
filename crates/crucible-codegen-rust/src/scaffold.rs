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

use std::collections::BTreeSet;
use std::path::PathBuf;

use crucible_ir::ir::{Operation, Service};

use crate::names::{self, function};
use crate::operations::{input_type, output_type};

/// Where the generated contract and the implementing type live, as paths
/// the stubs `use`.
#[derive(Debug, Clone)]
pub struct ScaffoldOptions {
    /// Override the service namespace path. Defaults to `crate::<service_name>`.
    pub contract: Option<String>,
    /// The type that implements every operation.
    pub service_type: String,
}

impl Default for ScaffoldOptions {
    fn default() -> Self {
        Self {
            contract: None,
            service_type: "crate::operations::Service".to_owned(),
        }
    }
}

/// One operation's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    /// The folder under `operations/`: `service`, or the resource's.
    pub folder: String,
    /// The Rust module identifier: the operation's name in snake_case,
    /// escaped for Rust (`r#type`, `self_`).
    pub module: String,
    /// The file stem on disk: e.g. `type` for `r#type`.
    pub file_stem: String,
    pub contents: String,
}

impl Stub {
    /// The file, relative to the `operations` folder.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.folder).join(format!("{}.rs", self.file_stem))
    }
}

/// The folder an operation's file goes in: its binding resource's name in
/// snake_case (with non-raw keywords suffixed), or `service` for an
/// operation the service binds.
pub fn folder(operation: &Operation) -> String {
    operation
        .resource
        .as_ref()
        .map_or_else(|| "service".to_owned(), |id| file_stem(id.name()))
}

/// A stub for every operation, in model order.
pub fn scaffold(service: &Service, options: &ScaffoldOptions) -> Vec<Stub> {
    service
        .operations
        .iter()
        .map(|operation| Stub {
            folder: folder(operation),
            module: module_name(operation.name()),
            file_stem: file_stem(operation.name()),
            contents: stub(service, operation, options),
        })
        .collect()
}

/// The Rust identifier for a module name: keywords escaped as raw
/// identifiers (`r#type`), and non-raw keywords suffixed (`self_`).
pub fn module_name(name: &str) -> String {
    names::module(name)
}

/// The file stem on disk: keyword escapes stripped (`type` for `r#type`),
/// suffixes preserved (`self_`).
pub fn file_stem(name: &str) -> String {
    names::file_stem(name)
}

fn stub(service: &Service, operation: &Operation, options: &ScaffoldOptions) -> String {
    let name = operation.name();
    let method = function(name);
    let output = output_type(service, operation)
        .trim_start_matches("model::")
        .to_owned();
    // The first line of the model's docs, as `//!` can carry it: rustc
    // refuses a bare CR or a bidirectional control there.
    let summary = operation
        .docs
        .as_deref()
        .map(crucible_codegen_common::docs::comment_text)
        .and_then(|docs| {
            docs.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| format!(": {}", line.trim_end_matches('.')))
        })
        .unwrap_or_default();
    let input = operation.input.is_some().then(|| {
        input_type(service, operation)
            .trim_start_matches("model::")
            .to_owned()
    });
    let (parameter, unused) = match &input {
        Some(input) => (format!(", input: {input}"), "        let _ = input;\n"),
        None => (String::new(), ""),
    };
    let mut imports = BTreeSet::from([format!("{name}Operation")]);
    for ty in input.iter().chain([&output]) {
        if ty != "()" && !ty.starts_with("::") {
            imports.insert(ty.clone());
        }
    }
    let implementation = options
        .service_type
        .rsplit("::")
        .next()
        .unwrap_or(&options.service_type);
    let service_name = if imports.contains(implementation) {
        available_name("ServiceImplementation", &imports)
    } else {
        implementation.to_owned()
    };
    let service_import = if service_name == implementation {
        options.service_type.clone()
    } else {
        format!("{} as {service_name}", options.service_type)
    };
    imports.insert(service_name.clone());
    // Runtime and implementation imports must also tolerate named model types.
    let result = if imports.contains("Result") {
        available_name("ServerResult", &imports)
    } else {
        "Result".to_owned()
    };
    let runtime_import = if result == "Result" {
        "Result".to_owned()
    } else {
        format!("Result as {result}")
    };
    imports.remove(&service_name);
    let imports = format!("{{{}}}", imports.into_iter().collect::<Vec<_>>().join(", "));
    let contract = options
        .contract
        .clone()
        .unwrap_or_else(|| format!("crate::{}", module_name(service.id.name())));
    format!(
        "//! {name}{summary}.\n\nuse {contract}::{imports};\nuse {service_import};\nuse crucible_server_runtime_rust::{runtime_import};\n\nimpl {name}Operation for {service_name} {{\n    async fn {method}(&self{parameter}) -> {result}<{output}> {{\n{unused}        todo!(\"{name}\")\n    }}\n}}\n",
        contract = contract,
    )
}

fn available_name(preferred: &str, imports: &BTreeSet<String>) -> String {
    let mut name = preferred.to_owned();
    let mut suffix = 2;
    while imports.contains(&name) {
        name = format!("{preferred}_{suffix}");
        suffix += 1;
    }
    name
}
