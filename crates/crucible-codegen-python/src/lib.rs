//! A Python package with typed models and sync/async HTTPX clients.
mod names;

use crucible_smithy::ir::{Service, Shape};
use crucible_smithy::{Capabilities, Diagnostics, Feature, ShapeId};
use names::{field, literal, ty};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub package_name: String,
    pub module_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

pub fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::JsonName,
        Feature::GreedyLabels,
        Feature::QueryParams,
        Feature::PrefixHeaders,
        Feature::ResponseCode,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::Sets,
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::HttpChecksum,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    let mut diagnostics = Diagnostics::default();
    let client_name = format!("{}Client", service.id.name());
    let async_client_name = format!("Async{client_name}");
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
        "ServiceError",
        "NetworkError",
        "ResponseMetadata",
        "Unset",
        "UNSET",
        "OpenEnum",
        "OpenIntEnum",
        "Decimal",
        "BasicCredentials",
        "AwsCredentials",
        "CachedCredentials",
        "AsyncCachedCredentials",
        "credential_chain",
        "async_credential_chain",
        "environment_credentials",
        "profile_credentials",
        "WaiterError",
        "WaiterTimeout",
        "ChecksumError",
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
        if field(op.name()).starts_with('_') || !methods.insert(field(op.name())) {
            diagnostics.push(&op.id, "operation collides with a Python client method");
        }
        if op.paginated.is_some() && !methods.insert(format!("{}_pages", field(op.name()))) {
            diagnostics.push(&op.id, "paginator collides with a Python client method");
        }
        for name in op.waiters.keys() {
            if !methods.insert(format!("wait_until_{}", field(name))) {
                diagnostics.push(&op.id, "waiter collides with a Python client method");
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let module = &options.module_name;
    let file = |path: String, contents: String| File { path, contents };
    let mut schema = crucible_codegen_common::schema(service, field);
    for (id, shape) in &service.shapes {
        if let Shape::Structure(structure) | Shape::Union(structure) = shape {
            for member in &structure.members {
                schema["shapes"][id.to_string()]["members"][&member.name]["optional"] =
                    serde_json::Value::Bool(member.optional_on_client());
            }
        }
    }
    let schema = serde_json::to_string(&schema).expect("schema serializes");
    let mut models = String::from(
        "# Generated by Crucible. Edit the Smithy model, then regenerate.\nfrom __future__ import annotations\n\nimport json\nfrom dataclasses import dataclass, field\nfrom datetime import datetime\nfrom typing import Any, TypeAlias\nfrom .runtime import UNSET, Unset, OpenEnum, ServiceError\n\n",
    );
    let decimal = service.shapes.values().any(|shape| {
        matches!(shape,
        Shape::Simple(s) if s.kind == crucible_smithy::ir::SimpleKind::BigDecimal)
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
                for value in &enumeration.values {
                    models.push_str(&format!(
                        "    {} = {}\n",
                        names::enum_member(&value.name),
                        value.value
                    ));
                }
                models.push('\n');
            }
            Shape::Unit => models.push_str(&format!("{name}: TypeAlias = None\n\n")),
            Shape::Simple(s) => {
                let prelude = ShapeId::new(
                    "smithy.api",
                    match s.kind {
                        crucible_smithy::ir::SimpleKind::String => "String",
                        crucible_smithy::ir::SimpleKind::Boolean => "Boolean",
                        crucible_smithy::ir::SimpleKind::Timestamp => "Timestamp",
                        crucible_smithy::ir::SimpleKind::Document => "Document",
                        crucible_smithy::ir::SimpleKind::Blob => "Blob",
                        crucible_smithy::ir::SimpleKind::BigDecimal => "BigDecimal",
                        crucible_smithy::ir::SimpleKind::Float
                        | crucible_smithy::ir::SimpleKind::Double => "Double",
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
                if let Some(docs) = &e.docs {
                    models.push_str(&format!("    {}\n", literal(docs)));
                }
                for v in &e.values {
                    models.push_str(&format!(
                        "    {} = {}\n",
                        names::enum_member(&v.name),
                        literal(&v.value)
                    ));
                }
                if e.values.is_empty() {
                    models.push_str("    pass\n");
                }
                models.push('\n');
            }
            Shape::Structure(s) | Shape::Union(s) => {
                if s.error.is_some() {
                    models.push_str(&format!("class {name}(ServiceError):\n"));
                } else {
                    models.push_str(&format!("@dataclass(kw_only=True)\nclass {name}:\n"));
                }
                if let Some(docs) = &s.docs {
                    models.push_str(&format!("    {}\n", literal(docs)));
                }
                for m in &s.members {
                    let native = field(&m.name);
                    let t = ty(service, &m.target, "");
                    let input = s.traits.contains_key(&ShapeId::new("smithy.api", "input"));
                    let optional = matches!(shape, Shape::Union(_))
                        || if input {
                            !m.required
                        } else {
                            m.optional_on_client()
                        }
                        || m.traits
                            .contains_key(&ShapeId::new("smithy.api", "idempotencyToken"));
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
                }
                if matches!(shape, Shape::Union(_)) {
                    models.push_str("    _unknown: tuple[str, Any] | Unset = field(default=UNSET, repr=False)\n");
                } else if s.members.is_empty() {
                    models.push_str("    pass\n");
                }
                models.push('\n');
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
        client.push_str(&format!("class {name}({base}):\n    def __init__(self, endpoint: str | None = None, **kwargs: Any) -> None:\n        super().__init__(endpoint=endpoint or {}, schema=m._SCHEMA, classes=m._CLASSES, content_sha256={}, **kwargs)\n\n",options.endpoint.as_ref().map_or("None".into(),|s|literal(s)),if options.content_sha256 {"True"} else {"False"}));
        for op in &service.operations {
            let input = op
                .input
                .as_ref()
                .map_or("None".into(), |id| ty(service, id, "m."));
            let output = op
                .output
                .as_ref()
                .map_or("None".into(), |id| ty(service, id, "m."));
            let method = field(op.name());
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
            let docs = op
                .docs
                .as_ref()
                .map_or(String::new(), |s| format!("        {}\n", literal(s)));
            client.push_str(&format!("    {def} {method}(self, {parameter}**options: Any) -> {output}:\n{docs}        return {wait}self._call({}, {argument}, **options)\n\n",literal(op.name())));
            for name in op.waiters.keys() {
                let result = if op.output.is_some() {
                    format!("{output} | None")
                } else {
                    output.clone()
                };
                client.push_str(&format!("    {def} wait_until_{}(self, {parameter}*, timeout: float, min_delay: float | None = None, max_delay: float | None = None) -> {result}:\n        return {wait}self._wait({}, {}, {argument}, timeout=timeout, min_delay=min_delay, max_delay=max_delay)\n\n", field(name), literal(op.name()), literal(name)));
            }
            if op.paginated.is_some() {
                let iter = if asynchronous {
                    "AsyncIterator"
                } else {
                    "Iterator"
                };
                let each = if asynchronous { "async for" } else { "for" };
                client.push_str(&format!("    {def} {method}_pages(self, {parameter}**options: Any) -> {iter}[{output}]:\n        {each} page in self._pages({}, {argument}, **options):\n            yield page\n\n",literal(op.name())));
            }
        }
    }
    let pyproject = format!(
        "[build-system]\nrequires = [\"setuptools>=68\"]\nbuild-backend = \"setuptools.build_meta\"\n\n[project]\nname = {}\nversion = {}\nrequires-python = \">=3.11\"\ndependencies = [\"httpx>=0.28,<1\", \"jmespath==1.0.1\"]\n\n[tool.setuptools.packages.find]\nwhere = [\"src\"]\n\n[tool.setuptools.package-data]\n{} = [\"py.typed\", \"crucible.json\"]\n",
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
    Ok(vec![
        file("pyproject.toml".into(), pyproject),
        file(format!("src/{module}/models.py"), models),
        file(format!("src/{module}/client.py"), client),
        file(
            format!("src/{module}/runtime.py"),
            include_str!("../runtime/runtime.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/auth.py"),
            include_str!("../runtime/auth.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/checksums.py"),
            include_str!("../runtime/checksums.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/query.py"),
            include_str!("../runtime/query.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/waiters.py"),
            include_str!("../runtime/waiters.py").replace("\r\n", "\n"),
        ),
        file(
            format!("src/{module}/endpoints.py"),
            include_str!("../runtime/endpoints.py").replace("\r\n", "\n"),
        ),
        file(format!("src/{module}/py.typed"), String::new()),
        file(format!("src/{module}/crucible.json"), provenance),
        file(
            format!("src/{module}/__init__.py"),
            format!(
                "from .client import {client_name}, {async_client_name}\n{model_import}from .runtime import UNSET, ServiceError, NetworkError, ResponseMetadata\nfrom .auth import BasicCredentials, AwsCredentials, CachedCredentials, AsyncCachedCredentials, credential_chain, async_credential_chain, environment_credentials, profile_credentials\nfrom .waiters import WaiterError, WaiterTimeout\nfrom .checksums import ChecksumError\n"
            ),
        ),
    ])
}
