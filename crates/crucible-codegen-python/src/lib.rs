//! A Python package with typed models and sync/async HTTPX clients.
mod links;
mod names;
mod permissions;
mod surface;

use crucible_ir::ir::{Service, Shape};
use crucible_ir::{Capabilities, Diagnostics, Feature, ShapeId};
use names::{field, literal, ty};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub use surface::surface;

/// The synchronous client class: the service name plus `Client`.
pub(crate) fn client_name(service: &Service) -> String {
    format!("{}Client", service.id.name())
}

/// The asynchronous client class beside [`client_name`].
pub(crate) fn async_client_name(service: &Service) -> String {
    format!("Async{}", client_name(service))
}

/// An operation's method on both client classes.
pub(crate) fn method_name(operation: &str) -> String {
    field(operation)
}

/// The method that pages through a `@paginated` operation's results.
pub(crate) fn pages_name(operation: &str) -> String {
    format!("{}_pages", method_name(operation))
}

pub struct Options {
    pub package_name: String,
    pub module_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

pub fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
    ])
}

/// Whether a structure's dataclass field defaults to `UNSET`, so a caller
/// may leave it out: in an input every member not `@required` (the server
/// applies a default), elsewhere a client-optional one; an idempotency
/// token always, since the client fills it.
pub(crate) fn omittable(
    structure: &crucible_ir::ir::Structure,
    member: &crucible_ir::ir::Member,
) -> bool {
    (if structure.input {
        !member.required
    } else {
        member.optional_on_client()
    }) || member.idempotency_token
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let exception = crucible_codegen_common::service_exception_name(service);
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut diagnostics = Diagnostics::default();
    let client_name = client_name(service);
    let async_client_name = async_client_name(service);
    if !names::identifier(&options.module_name)
        || field(&options.module_name) != options.module_name
    {
        diagnostics.push(
            &service.id,
            "Python module name must be a lowercase identifier that is not reserved",
        );
    }
    let reserved = [
        "Any",
        "model_repr",
        "ServiceError",
        "NetworkError",
        "ResponseMetadata",
        "Unset",
        "UNSET",
        "OpenEnum",
        "OpenIntEnum",
        "Decimal",
        "TelemetryHook",
        "OpenTelemetryHook",
        "OperationStarted",
        "AttemptStarted",
        "AttemptFinished",
        "OperationFinished",
        "datetime",
        "dataclass",
        "field",
        "json",
        "TypeAlias",
        "_CLASSES",
        "_SCHEMA",
        "str",
        "int",
        "float",
        "bool",
        "list",
        "dict",
        "tuple",
    ];
    let mut shapes = BTreeSet::new();
    for (id, shape) in service.shapes.iter().filter(|(id, _)| !id.is_prelude()) {
        let name = service.name_of(id);
        if !names::identifier(name)
            || names::keyword(name)
            || reserved.contains(&name)
            || name == client_name
            || name == async_client_name
            || !shapes.insert(name)
        {
            diagnostics.push(
                id,
                format!(
                    "Python type name `{name}` is invalid or collides with another exported name"
                ),
            );
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::new();
            for m in &s.members {
                if !members.insert(field(&m.name)) {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "members collide after Python naming",
                    );
                }
                if s.error.is_some()
                    && [
                        "name",
                        "metadata",
                        "retryable",
                        "throttling",
                        "fields",
                        "args",
                    ]
                    .contains(&field(&m.name).as_str())
                {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "member collides with a Python error attribute",
                    );
                }
            }
        }
        if let Shape::Enum(e) = shape {
            let mut values = BTreeSet::new();
            for value in &e.values {
                if !values.insert(names::enum_member(&value.name)) {
                    diagnostics.push(id, "enum members collide after Python naming");
                }
            }
        }
        if let Shape::IntEnum(e) = shape {
            let mut values = BTreeSet::new();
            for value in &e.values {
                if !values.insert(names::enum_member(&value.name)) {
                    diagnostics.push(id, "integer enum members collide after Python naming");
                }
            }
        }
    }
    let mut methods = BTreeSet::from(["close".to_string(), "aclose".to_string()]);
    for op in &service.operations {
        if method_name(op.name()).starts_with('_') || !methods.insert(method_name(op.name())) {
            diagnostics.push(&op.id, "operation collides with a Python client method");
        }
        if op.paginated.is_some() && !methods.insert(pages_name(op.name())) {
            diagnostics.push(&op.id, "paginator collides with a Python client method");
        }
    }
    permissions::check_names(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let module = &options.module_name;
    let file = |path: String, contents: String| File { path, contents };
    let mut schema = crucible_codegen_common::schema(service, field);
    for (id, shape) in &service.shapes {
        if shape.sensitive() {
            schema["shapes"][id.to_string()]["sensitive"] = serde_json::Value::Bool(true);
        }
        if let Shape::Structure(structure) | Shape::Union(structure) = shape {
            for member in &structure.members {
                schema["shapes"][id.to_string()]["members"][&member.name]["optional"] =
                    serde_json::Value::Bool(member.optional_on_client());
            }
        }
    }
    let schema = serde_json::to_string(&schema).expect("schema serializes");
    let mut models = format!(
        "# Generated by Crucible. Edit the Smithy model, then regenerate.\nfrom __future__ import annotations\n\nimport json\nfrom dataclasses import dataclass, field\nfrom datetime import datetime\nfrom typing import Any, TypeAlias\nfrom .runtime import UNSET, Unset, OpenEnum, model_repr, {exception}\n\n",
    );
    let decimal = service.shapes.values().any(|shape| {
        matches!(shape,
        Shape::Simple(s) if s.kind == crucible_ir::ir::SimpleKind::BigDecimal)
    });
    if decimal {
        models.push_str("from decimal import Decimal\n\n");
    }
    if service
        .shapes
        .values()
        .any(|shape| matches!(shape, Shape::IntEnum(_)))
    {
        models.push_str("from .runtime import OpenIntEnum\n\n");
    }
    // Aliases use forward references so model-file ordering cannot affect imports.
    for (id, shape) in service.shapes.iter().filter(|(id, _)| !id.is_prelude()) {
        let name = service.name_of(id);
        match shape {
            Shape::IntEnum(enumeration) => {
                models.push_str(&format!("class {name}(OpenIntEnum):\n"));
                python_doc(
                    service,
                    &mut models,
                    "    ",
                    &crucible_codegen_common::documentation::with_notes(
                        Some(enumeration.docs.as_deref().unwrap_or(
                            "An open integer enumeration; unknown values are preserved.",
                        )),
                        &enumeration.notes,
                    )
                    .unwrap_or_default(),
                );
                for value in &enumeration.values {
                    models.push_str(&format!(
                        "    {} = {}\n",
                        names::enum_member(&value.name),
                        value.value
                    ));
                    if let Some(docs) = crucible_codegen_common::documentation::with_notes(
                        value.docs.as_deref(),
                        &value.notes,
                    ) {
                        python_doc(service, &mut models, "    ", &docs);
                    }
                }
                models.push('\n');
            }
            Shape::Unit { .. } => models.push_str(&format!("{name}: TypeAlias = None\n\n")),
            Shape::Simple(s) => {
                let prelude = ShapeId::new(
                    "smithy.api",
                    match s.kind {
                        crucible_ir::ir::SimpleKind::String => "String",
                        crucible_ir::ir::SimpleKind::Boolean => "Boolean",
                        crucible_ir::ir::SimpleKind::Timestamp => "Timestamp",
                        crucible_ir::ir::SimpleKind::Document => "Document",
                        crucible_ir::ir::SimpleKind::Blob => "Blob",
                        crucible_ir::ir::SimpleKind::BigDecimal => "BigDecimal",
                        crucible_ir::ir::SimpleKind::Float
                        | crucible_ir::ir::SimpleKind::Double => "Double",
                        _ => "Long",
                    },
                );
                let t = match prelude.name() {
                    "String" => "str",
                    "Boolean" => "bool",
                    "Timestamp" => "datetime",
                    "Document" => "Any",
                    "Blob" => "bytes",
                    "Double" => "float",
                    "BigDecimal" => "Decimal",
                    _ => "int",
                };
                models.push_str(&format!("{name}: TypeAlias = {t}\n\n"));
            }
            Shape::List(l) => models.push_str(&format!(
                "{name}: TypeAlias = list[{}]\n\n",
                literal(&format!(
                    "{}{}",
                    ty(service, &l.member.target, ""),
                    if l.sparse { " | None" } else { "" }
                ))
            )),
            Shape::Map(m) => models.push_str(&format!(
                "{name}: TypeAlias = dict[{}, {}]\n\n",
                literal(&ty(service, &m.key.target, "")),
                literal(&format!(
                    "{}{}",
                    ty(service, &m.value.target, ""),
                    if m.sparse { " | None" } else { "" }
                ))
            )),
            Shape::Enum(e) => {
                models.push_str(&format!("class {name}(OpenEnum):\n"));
                if let Some(docs) =
                    crucible_codegen_common::documentation::with_notes(e.docs.as_deref(), &e.notes)
                {
                    models.push_str(&format!("    {}\n", literal(&links::text(service, &docs))));
                }
                for v in &e.values {
                    models.push_str(&format!(
                        "    {} = {}\n",
                        names::enum_member(&v.name),
                        literal(&v.value)
                    ));
                    if let Some(docs) = crucible_codegen_common::documentation::with_notes(
                        v.docs.as_deref(),
                        &v.notes,
                    ) {
                        python_doc(service, &mut models, "    ", &docs);
                    }
                }
                if e.values.is_empty() {
                    models.push_str("    pass\n");
                }
                models.push('\n');
            }
            Shape::Structure(s) | Shape::Union(s) => {
                if s.error.is_some() {
                    models.push_str(&format!("class {name}({exception}):\n"));
                } else {
                    models.push_str(&format!(
                        "@dataclass(kw_only=True, repr=False)\nclass {name}:\n"
                    ));
                }
                let mut docs = s
                    .docs
                    .clone()
                    .unwrap_or_else(|| format!("Modeled {name} value."));
                docs = crucible_codegen_common::documentation::with_notes(Some(&docs), &s.notes)
                    .unwrap_or(docs);
                if !s.members.is_empty() {
                    docs.push_str("\n\nAttributes:\n");
                }
                for member in &s.members {
                    docs.push_str(&format!(
                        "    {}: {}\n",
                        field(&member.name),
                        member.docs.as_deref().unwrap_or("Modeled member.")
                    ));
                }
                python_doc(service, &mut models, "    ", &docs);
                for m in &s.members {
                    let native = field(&m.name);
                    let t = ty(service, &m.target, "");
                    let optional = matches!(shape, Shape::Union(_)) || omittable(s, m);
                    if s.error.is_some() {
                        if native == "message" {
                            models.push_str("    message: str\n");
                        } else {
                            models.push_str(&format!("    {native}: {t} | Unset\n"));
                        }
                    } else if optional {
                        models.push_str(&format!(
                            "    {native}: {t} | None | Unset = field(default=UNSET, repr={})\n",
                            if service.sensitive(m) {
                                "False"
                            } else {
                                "True"
                            }
                        ));
                    } else {
                        models.push_str(&format!(
                            "    {native}: {t} = field(repr={})\n",
                            if service.sensitive(m) {
                                "False"
                            } else {
                                "True"
                            }
                        ));
                    }
                    if let Some(docs) = crucible_codegen_common::documentation::with_notes(
                        m.docs.as_deref(),
                        &m.notes,
                    ) {
                        python_doc(service, &mut models, "    ", &docs);
                    }
                }
                if matches!(shape, Shape::Union(_)) {
                    models.push_str("    _unknown: tuple[str, Any] | Unset = field(default=UNSET, repr=False)\n");
                } else if s.members.is_empty() {
                    models.push_str("    pass\n");
                }
                models.push_str(&format!("\n    def __repr__(self) -> str:\n        \"\"\"Describe present members, redacting modeled sensitive values.\"\"\"\n        return model_repr(_SCHEMA, {}, self)\n", literal(&id.to_string())));
                models.push('\n');
            }
        }
        if matches!(shape, Shape::Simple(_) | Shape::List(_) | Shape::Map(_)) {
            if let Some(docs) = crucible_codegen_common::documentation::model_with_notes(shape) {
                python_doc(service, &mut models, "", &docs);
            }
        }
    }
    models.push_str(&format!(
        "_SCHEMA = json.loads({}{})\n_CLASSES: dict[str, Any] = {{\n",
        literal(&schema),
        if decimal { ", parse_float=Decimal" } else { "" }
    ));
    for (id, shape) in &service.shapes {
        if matches!(
            shape,
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) && !id.is_prelude()
        {
            models.push_str(&format!(
                "    {}: {},\n",
                literal(&id.to_string()),
                service.name_of(id)
            ));
        }
    }
    models.push_str("}\n");
    let mut client = String::from(
        "# Generated by Crucible.\nfrom __future__ import annotations\nfrom typing import Any, Iterator, AsyncIterator\nfrom . import models as m\nfrom .runtime import SyncClient, AsyncClientBase\n\n",
    );
    for asynchronous in [false, true] {
        let (name, base, def, wait) = if asynchronous {
            (
                async_client_name.as_str(),
                "AsyncClientBase",
                "async def",
                "await ",
            )
        } else {
            (client_name.as_str(), "SyncClient", "def", "")
        };
        let client_docs = format!(
            "{}\n\nHTTP client. Construct with endpoint and keyword configuration described by the inherited initializer. {} The client only closes its own HTTPX transport; caller-supplied http transports remain caller-owned.",
            service.docs.as_deref().unwrap_or(""),
            if asynchronous {
                "Use async with or await aclose(). Share across tasks on one event loop if callbacks are safe for concurrent calls."
            } else {
                "Use with or close(). Share across threads only if all supplied callbacks are thread-safe."
            }
        );
        client.push_str(&format!("class {name}({base}):\n    {}\n    def __init__(self, endpoint: str | None = None, **kwargs: Any) -> None:\n        super().__init__(endpoint=endpoint or {}, schema=m._SCHEMA, classes=m._CLASSES, **kwargs)\n    __init__.__doc__ = {base}.__init__.__doc__\n\n",literal(&links::text(service, &client_docs)),options.endpoint.as_ref().map_or("None".into(),|s|literal(s))));
        for op in &service.operations {
            let input = op
                .input
                .as_ref()
                .map_or("None".into(), |id| ty(service, id, "m."));
            let output = op
                .output
                .as_ref()
                .map_or("None".into(), |id| ty(service, id, "m."));
            let method = method_name(op.name());
            let pages = pages_name(op.name());
            let required = op
                .input
                .as_ref()
                .and_then(|id| service.shape(id))
                .is_some_and(|shape| match shape {
                    Shape::Structure(s) => s
                        .members
                        .iter()
                        .any(|m| m.required && op.idempotency_token.as_ref() != Some(&m.name)),
                    _ => false,
                });
            let parameter = if op.input.is_none() {
                String::new()
            } else if required {
                format!("input: {input}, ")
            } else {
                format!("input: {input} | None = None, ")
            };
            let argument = if op.input.is_none() { "None" } else { "input" };
            let docs = format!(
                "        {}\n",
                literal(&links::text(
                    service,
                    &crucible_codegen_common::documentation::operation_with_notes(service, op)
                ))
            );
            client.push_str(&format!("    {def} {method}(self, {parameter}**options: Any) -> {output}:\n{docs}        return {wait}self._call({}, {argument}, **options)\n\n",literal(op.name())));
            if op.paginated.is_some() {
                let iter = if asynchronous {
                    "AsyncIterator"
                } else {
                    "Iterator"
                };
                let each = if asynchronous { "async for" } else { "for" };
                client.push_str(&format!("    {def} {pages}(self, {parameter}**options: Any) -> {iter}[{output}]:\n        \"\"\"Yield pages lazily from a snapshot of input; stop on absent/empty tokens and reject repeated tokens.\"\"\"\n        {each} page in self._pages({}, {argument}, **options):\n            yield page\n\n",literal(op.name())));
            }
        }
    }
    let pyproject = format!(
        "[build-system]\nrequires = [\"setuptools>=68\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[project]\nname = {}\nversion = {}\nrequires-python = \">=3.11\"\ndependencies = [\"httpx>=0.28,<1\"]\n\n[tool.setuptools.packages.find]\nwhere = [\"src\"]\n\n[tool.setuptools.package-data]\n{} = [\"py.typed\", \"crucible.json\"]\n",
        literal(&options.package_name),
        literal(&options.package_version),
        literal(module)
    );
    let exports = service
        .shapes
        .keys()
        .filter(|id| !id.is_prelude())
        .map(|id| format!("    {},", service.name_of(id)))
        .collect::<Vec<_>>()
        .join("\n");
    let model_import = if exports.is_empty() {
        String::new()
    } else {
        format!("from .models import (\n{exports}\n)\n")
    };
    let provenance = serde_json::to_string_pretty(&serde_json::json!({
        "generatorVersion": env!("CARGO_PKG_VERSION"), "service": service.id.to_string(),
        "serviceVersion": service.version, "schemaSha256": format!("{:x}", Sha256::digest(schema.as_bytes())),
    })).expect("provenance serializes");
    let mut files = vec![
        file("pyproject.toml".into(), pyproject),
        file(format!("src/{module}/models.py"), models),
        file(format!("src/{module}/client.py"), client),
    ];
    // Its own module, so a model named `GraphPermission` or `PERMISSIONS`
    // cannot collide with a permission's name.
    if permissions::present(service) {
        files.push(file(
            format!("src/{module}/permissions.py"),
            permissions::emit(service),
        ));
    }
    files.extend([
        file(
            format!("src/{module}/runtime.py"),
            crucible_codegen_common::service_exception_runtime(
                service,
                include_str!("../runtime/runtime.py"),
            )
            .replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/auth.py"),
            include_str!("../runtime/auth.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/policy.py"),
            include_str!("../runtime/policy.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/telemetry.py"),
            include_str!("../runtime/telemetry.py").replace("\r\n", "\n"),
        ),
        file(format!("src/{module}/py.typed"), String::new()),
        file(format!("src/{module}/crucible.json"), provenance),
        file(
            format!("src/{module}/__init__.py"),
            format!(
                "from .client import {client_name}, {async_client_name}\n{model_import}from .runtime import UNSET, ServiceError, {exception}, NetworkError, ResponseMetadata\nfrom .telemetry import TelemetryHook, OpenTelemetryHook, OperationStarted, AttemptStarted, AttemptFinished, OperationFinished\n"
            ),
        ),
    ]);
    Ok(files)
}

pub(crate) fn python_doc(service: &Service, out: &mut String, indent: &str, docs: &str) {
    // A standalone string literal is an attribute docstring after an assignment.
    // JSON escaping also prevents quotes and backslashes from changing Python syntax.
    out.push_str(&format!(
        "{indent}{}\n",
        literal(&links::text(service, docs))
    ));
}
