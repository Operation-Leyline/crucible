//! Native Go models, context-aware clients, and a standard-library runtime.
mod links;
mod permissions;
mod surface;
use crucible_codegen_common::documentation::{model as shape_docs, operation as operation_docs};
use crucible_ir::ir::{DocNotes, Service, Shape, SimpleKind};
use crucible_ir::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub use surface::surface;

/// The client struct: the exported service name plus `Client`.
pub(crate) fn client_name(service: &Service) -> String {
    format!("{}Client", exported(service.id.name()))
}

/// An operation's method on the client.
pub(crate) fn method_name(operation: &str) -> String {
    exported(operation)
}

/// The method that creates a `@paginated` operation's paginator.
pub(crate) fn pages_name(operation: &str) -> String {
    format!("{}Pages", method_name(operation))
}

/// The paginator type [`pages_name`] returns.
pub(crate) fn paginator_name(operation: &str) -> String {
    format!("{}Paginator", method_name(operation))
}

/// A shape's declared Go type name, before [`ty`] makes a structure a pointer.
pub(crate) fn type_name(service: &Service, id: &ShapeId) -> String {
    exported(service.name_of(id))
}

pub struct Options {
    pub module_path: String,
    pub package_name: String,
    pub endpoint: Option<String>,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

fn literal(value: &str) -> String {
    crucible_codegen_common::quoted(value)
}

pub(crate) fn exported(value: &str) -> String {
    let mut result = String::new();
    let mut upper = true;
    for c in value.chars() {
        if c == '_' {
            upper = true;
        } else {
            result.push(if upper { c.to_ascii_uppercase() } else { c });
            upper = false;
        }
    }
    if !result.starts_with(|c: char| c.is_ascii_uppercase()) {
        result.insert_str(0, "Value");
    }
    result
}

/// The Go type of a simple kind. `Decimal` is the package's own, so it
/// takes `qualifier` (empty inside the package); the rest are the
/// language's or the standard library's.
fn simple(kind: SimpleKind, qualifier: &str) -> String {
    let native = match kind {
        SimpleKind::BigInteger => "*big.Int",
        SimpleKind::BigDecimal => return format!("{qualifier}Decimal"),
        SimpleKind::String => "string",
        SimpleKind::Boolean => "bool",
        SimpleKind::Byte => "int8",
        SimpleKind::Short => "int16",
        SimpleKind::Integer => "int32",
        SimpleKind::Long => "int64",
        SimpleKind::Float => "float32",
        SimpleKind::Double => "float64",
        SimpleKind::Timestamp => "time.Time",
        SimpleKind::Document => "any",
        SimpleKind::Blob => "[]byte",
    };
    native.to_owned()
}

fn nullable_ty(service: &Service, id: &ShapeId, sparse: bool) -> String {
    let value = ty(service, id);
    if sparse && !value.starts_with('*') {
        format!("*{value}")
    } else {
        value
    }
}

pub(crate) fn ty(service: &Service, id: &ShapeId) -> String {
    qualified_ty(service, id, "")
}

/// [`ty`] as code outside the package writes it: the package's own types
/// prefixed with `qualifier` (`weather.`).
pub(crate) fn qualified_ty(service: &Service, id: &ShapeId, qualifier: &str) -> String {
    let shape = service.shape(id).expect("lowered target");
    if id.is_prelude() {
        return match shape {
            Shape::Simple(s) => simple(s.kind, qualifier),
            Shape::IntEnum(_) => unreachable!("integer enums are capability-gated"),
            Shape::Unit { .. } => format!("{qualifier}Unit"),
            _ => unreachable!("prelude shape"),
        };
    }
    let name = type_name(service, id);
    if matches!(shape, Shape::Structure(_) | Shape::Union(_)) {
        format!("*{qualifier}{name}")
    } else {
        format!("{qualifier}{name}")
    }
}

/// A structure or union member's field type: its value's type, wrapped in
/// `Optional` when the member may be absent and in every union.
/// Whether a member's field is `Optional[T]`, so it can be left absent:
/// Go fills no modeled defaults, and a plain field always sends its value
/// (its zero value when unset).
pub(crate) fn absent(shape: &Shape, member: &crucible_ir::ir::Member) -> bool {
    member.optional_on_client() || matches!(shape, Shape::Union(_))
}

pub(crate) fn field_type(
    service: &Service,
    shape: &Shape,
    member: &crucible_ir::ir::Member,
    qualifier: &str,
) -> String {
    let t = qualified_ty(service, &member.target, qualifier);
    if absent(shape, member) {
        format!("{qualifier}Optional[{t}]")
    } else {
        t
    }
}

pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::GreedyLabels,
        Feature::Sparse,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    emit_package(service, options, true)
}

/// [`emit`] without the `permissions` sub-package or its name checks, for
/// a package that embeds the client and never names a permission (the
/// Terraform provider): a permission Go could not spell must not stop it.
pub fn emit_without_permissions(
    service: &Service,
    options: &Options,
) -> Result<Vec<File>, Diagnostics> {
    emit_package(service, options, false)
}

fn emit_package(
    service: &Service,
    options: &Options,
    with_permissions: bool,
) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut errors = Diagnostics::default();
    let client_name = client_name(service);
    let exception = exported(&crucible_codegen_common::service_exception_name(service));
    if options.package_name.is_empty()
        || !options
            .package_name
            .starts_with(|c: char| c.is_ascii_lowercase())
        || !options
            .package_name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        || [
            "break",
            "default",
            "func",
            "interface",
            "select",
            "case",
            "defer",
            "go",
            "map",
            "struct",
            "chan",
            "else",
            "goto",
            "package",
            "switch",
            "const",
            "fallthrough",
            "if",
            "range",
            "type",
            "continue",
            "for",
            "import",
            "return",
            "var",
            "init",
            "main",
        ]
        .contains(&options.package_name.as_str())
    {
        errors.push(
            &service.id,
            "Go package name must be a non-reserved lowercase identifier",
        );
    }
    if options.module_path.is_empty()
        || options
            .module_path
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || "/.-_~".contains(c)))
    {
        errors.push(&service.id, "Go module path contains invalid characters");
    }
    let mut names: BTreeSet<String> = [
        "Config",
        "HTTPClient",
        "Optional",
        "Some",
        "Null",
        "Unit",
        "ServiceError",
        "ResponseMetadata",
        "UnknownUnion",
        "NetworkError",
        "Decimal",
        "ParseDecimal",
        "RetryMode",
        "RetryModeLegacy",
        "RetryModeStandard",
        "RetryModeAdaptive",
        "Clock",
        "TelemetryHook",
        "NopTelemetryHook",
        "OperationStartedEvent",
        "AttemptStartedEvent",
        "AttemptFinishedEvent",
        "OperationFinishedEvent",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    names.insert(client_name.clone());
    names.insert(exception.clone());
    names.insert(format!("New{client_name}"));
    let mut methods = BTreeSet::new();
    for (id, shape) in service.shapes.iter().filter(|(id, _)| !id.is_prelude()) {
        let name = type_name(service, id);
        if !names.insert(name.clone()) {
            errors.push(
                id,
                format!("Go name `{name}` collides with another declaration"),
            );
        }
        if let Shape::Enum(e) = shape {
            for v in &e.values {
                let symbol = format!("{name}{}", exported(&v.name));
                if !names.insert(symbol.clone()) {
                    errors.push(id, format!("Go enum name `{symbol}` collides"));
                }
            }
        }
        if let Shape::IntEnum(e) = shape {
            for v in &e.values {
                let symbol = format!("{name}{}", exported(&v.name));
                if !names.insert(symbol.clone()) {
                    errors.push(id, format!("Go integer enum name `{symbol}` collides"));
                }
            }
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::new();
            for m in &s.members {
                let native = exported(&m.name);
                if !members.insert(native.clone())
                    || ["String", "GoString"].contains(&native.as_str())
                    || matches!(shape, Shape::Union(_)) && native == "UnknownVariant"
                    || s.error.is_some()
                        && ["Error", "Unwrap", "Metadata"].contains(&native.as_str())
                {
                    errors.push(
                        &id.with_member(&m.name),
                        "member collides with a Go model field or method",
                    );
                }
            }
        }
    }
    for op in &service.operations {
        if !methods.insert(method_name(op.name())) {
            errors.push(&op.id, "Go operation method name collision");
        }
        if op.paginated.is_some()
            && (!methods.insert(pages_name(op.name())) || !names.insert(paginator_name(op.name())))
        {
            errors.push(&op.id, "Go paginator name collision");
        }
    }
    if with_permissions {
        permissions::check(service, &mut errors);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let header = format!(
        "// Code generated by Crucible. DO NOT EDIT.\npackage {}\n\n",
        options.package_name
    );
    let mut models = header.clone();
    if service
        .shapes
        .values()
        .any(|s| matches!(s, Shape::Simple(s) if s.kind == SimpleKind::BigInteger))
    {
        models.push_str("import \"math/big\"\n\n");
    }
    if service
        .shapes
        .values()
        .any(|s| matches!(s,Shape::Simple(s) if s.kind == SimpleKind::Timestamp))
    {
        models.push_str("import \"time\"\n\n");
    }
    for (id, shape) in service.shapes.iter().filter(|(id, _)| !id.is_prelude()) {
        let name = type_name(service, id);
        go_doc_for(
            &mut models,
            (service, links::Package::Client),
            "",
            &name,
            shape_docs(shape).as_deref(),
            shape.notes().unwrap_or(&DocNotes::default()),
        );
        match shape {
            Shape::IntEnum(e) => {
                models.push_str(&format!(
                    "// {name} preserves unrecognized integer values.\ntype {name} int32\nconst (\n"
                ));
                for v in &e.values {
                    go_doc_for(
                        &mut models,
                        (service, links::Package::Client),
                        "    ",
                        &format!("{name}{}", exported(&v.name)),
                        v.docs.as_deref(),
                        &v.notes,
                    );
                    models.push_str(&format!(
                        "    {name}{} {name} = {}\n",
                        exported(&v.name),
                        v.value
                    ));
                }
                models.push_str(")\n\n");
            }
            Shape::Unit { .. } => models.push_str(&format!("type {name} = Unit\n\n")),
            Shape::Simple(s) => {
                models.push_str(&format!("type {name} = {}\n\n", simple(s.kind, "")))
            }
            Shape::List(l) => models.push_str(&format!(
                "type {name} []{}\n\n",
                nullable_ty(service, &l.member.target, l.sparse)
            )),
            Shape::Map(m) => models.push_str(&format!(
                "type {name} map[{}]{}\n\n",
                ty(service, &m.key.target),
                nullable_ty(service, &m.value.target, m.sparse)
            )),
            Shape::Enum(e) => {
                models.push_str(&format!(
                    "// {name} preserves unrecognized string values.\ntype {name} string\nconst (\n"
                ));
                for v in &e.values {
                    go_doc_for(
                        &mut models,
                        (service, links::Package::Client),
                        "    ",
                        &format!("{name}{}", exported(&v.name)),
                        v.docs.as_deref(),
                        &v.notes,
                    );
                    models.push_str(&format!(
                        "    {name}{} {name} = {}\n",
                        exported(&v.name),
                        literal(&v.value)
                    ));
                }
                models.push_str(")\n\n");
            }
            Shape::Structure(s) | Shape::Union(s) => {
                models.push_str(&format!("type {name} struct {{\n"));
                for m in &s.members {
                    go_doc_for(
                        &mut models,
                        (service, links::Package::Client),
                        "    ",
                        &exported(&m.name),
                        m.docs.as_deref(),
                        &m.notes,
                    );
                    let t = field_type(service, shape, m, "");
                    models.push_str(&format!("    {} {t}\n", exported(&m.name)));
                }
                if matches!(shape, Shape::Union(_)) {
                    models.push_str("    UnknownVariant *UnknownUnion\n");
                }
                if s.error.is_some() {
                    models.push_str("    Metadata *ServiceError\n");
                }
                models.push_str("}\n");
                models.push_str(&format!("// String describes present members and redacts sensitive values.\nfunc (m {name}) String() string {{ return formatModel(wireSchema, {}, m) }}\n// GoString applies the same redaction for Go syntax formatting.\nfunc (m {name}) GoString() string {{ return m.String() }}\n", literal(&id.to_string())));
                // Keep tri-state storage available, while ordinary callers use
                // native arguments and return values as in conventional Go APIs.
                let mut used: BTreeSet<String> =
                    s.members.iter().map(|m| exported(&m.name)).collect();
                used.extend(
                    [
                        "Error",
                        "Unwrap",
                        "Metadata",
                        "UnknownVariant",
                        "String",
                        "GoString",
                    ]
                    .map(str::to_owned),
                );
                for m in &s.members {
                    let native = exported(&m.name);
                    let t = ty(service, &m.target);
                    let optional = m.optional_on_client() || matches!(shape, Shape::Union(_));
                    for action in ["Set", "Get", "Has", "IsNull"] {
                        if !optional && matches!(action, "Has" | "IsNull") {
                            continue;
                        }
                        let base = format!("{action}{native}");
                        let mut method = base.clone();
                        let mut suffix = 2;
                        while !used.insert(method.clone()) {
                            method = format!("{base}{suffix}");
                            suffix += 1;
                        }
                        go_doc(
                            &mut models,
                            (service, links::Package::Client),
                            "",
                            &method,
                            Some(match action {
                                "Set" => {
                                    if matches!(shape, Shape::Union(_)) {
                                        "Set this union arm and clear the other arms."
                                    } else {
                                        "Set the member value."
                                    }
                                }
                                "Get" => {
                                    "Return the member value; absent and null optional members return the zero value."
                                }
                                "Has" => {
                                    "Report whether the member is present, including explicit null."
                                }
                                _ => "Report whether the member is explicitly null.",
                            }),
                        );
                        match action {
                            "Set" => {
                                let value = if optional { "Some(value)" } else { "value" };
                                let clear = if matches!(shape, Shape::Union(_)) { "*m = ".to_owned() + &name + "{}; " } else { String::new() };
                                models.push_str(&format!("func (m *{name}) {method}(value {t}) {{ {clear}m.{native} = {value} }}\n"));
                            }
                            "Get" => {
                                let value = if optional { format!("m.{native}.Value") } else { format!("m.{native}") };
                                if optional {
                                    models.push_str(&format!("func (m *{name}) {method}() {t} {{ if m != nil && m.{native}.Set && !m.{native}.Null {{ return {value} }}; var zero {t}; return zero }}\n"));
                                } else {
                                    models.push_str(&format!("func (m *{name}) {method}() {t} {{ return {value} }}\n"));
                                }
                            }
                            "Has" => models.push_str(&format!("func (m *{name}) {method}() bool {{ return m != nil && m.{native}.Set }}\n")),
                            _ => models.push_str(&format!("func (m *{name}) {method}() bool {{ return m != nil && m.{native}.Set && m.{native}.Null }}\n")),
                        }
                    }
                }
                if s.error.is_some() {
                    models.push_str(&format!("func (*{name}) serviceException() {{}}\n// Error returns the redacted modeled representation.\nfunc (e *{name}) Error() string {{ return e.String() }}\n// Unwrap exposes the underlying service response metadata.\nfunc (e *{name}) Unwrap() error {{ return e.Metadata }}\n"));
                }
                models.push('\n');
            }
        }
        if matches!(
            shape,
            Shape::List(_) | Shape::Map(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) {
            models.push_str(&format!("// String renders nested values and redacts sensitive data.\nfunc (m {name}) String() string {{ return formatModel(wireSchema, {}, m) }}\n// GoString applies the same redaction for Go syntax formatting.\nfunc (m {name}) GoString() string {{ return m.String() }}\n\n", literal(&id.to_string())));
        }
    }
    let mut client = header.clone();
    if !service.operations.is_empty() {
        client.push_str("import \"context\"\n\n");
    }
    let mut schema = crucible_codegen_common::schema(service, exported);
    for (id, shape) in &service.shapes {
        if shape.sensitive() {
            schema["shapes"][id.to_string()]["sensitive"] = serde_json::Value::Bool(true);
        }
    }
    let schema = serde_json::to_string(&schema).expect("schema serializes");
    client.push_str(&format!(
        "var wireSchema = mustSchema({})\n\n",
        literal(&schema)
    ));
    go_doc(
        &mut client,
        (service, links::Package::Client),
        "",
        &client_name,
        Some(&format!(
            "{}\n\nHTTP client. Construct with New{client_name}(Config). It can be shared across goroutines if supplied transports, callbacks and middleware are concurrency-safe. There is no close method; callers own custom transport cleanup. Do not mutate shared configuration maps after construction.",
            service.docs.as_deref().unwrap_or("")
        )),
    );
    client.push_str(&format!("type {client_name} struct {{ *clientRuntime }}\n\n// New{client_name} validates configuration and applies defaults without sending a request.\nfunc New{client_name}(config Config) (*{client_name}, error) {{\n if config.Endpoint == \"\" {{ config.Endpoint = {} }}\n runtime, err := newClient(config, wireSchema)\n if err != nil {{ return nil, err }}\n return &{client_name}{{clientRuntime: runtime}}, nil\n}}\n\n",literal(options.endpoint.as_deref().unwrap_or(""))));
    client.push_str("func modeledError(target string, values map[string]any, metadata *ServiceError) error {\n switch target {\n");
    for (id, shape) in &service.shapes {
        if matches!(shape,Shape::Structure(s) if s.error.is_some()) {
            let name = type_name(service, id);
            client.push_str(&format!(" case {}:\n  result := &{name}{{Metadata: metadata}}\n  if err := decodeModel(wireSchema, target, values, result); err != nil {{ return metadata }}\n  return result\n",literal(&id.to_string())));
        }
    }
    client.push_str(" }\n return metadata\n}\n\n");
    for op in &service.operations {
        let name = method_name(op.name());
        let pages = pages_name(op.name());
        let paginator = paginator_name(op.name());
        let input = op
            .input
            .as_ref()
            .map_or("Unit".into(), |id| ty(service, id));
        let output = op
            .output
            .as_ref()
            .map_or("Unit".into(), |id| ty(service, id));
        let target = op
            .output
            .as_ref()
            .map_or(String::new(), ToString::to_string);
        go_doc_for(
            &mut client,
            (service, links::Package::Client),
            "",
            &name,
            Some(&operation_docs(service, op)),
            &op.notes,
        );
        client.push_str(&format!("func (c *{client_name}) {name}(ctx context.Context, input {input}) ({output}, error) {{\n var output {output}\n values, err := c.call(ctx, {}, input)\n if err != nil {{ return output, err }}\n err = decodeModel(c.schema, {}, values, &output)\n return output, err\n}}\n\n",literal(op.name()),literal(&target)));
        if let Some(p) = &op.paginated {
            let input_token = wire_path(
                service,
                op.input.as_ref().expect("paginated input"),
                &p.input_token,
            );
            let output_token = wire_path(
                service,
                op.output.as_ref().expect("paginated output"),
                &p.output_token,
            );
            client.push_str(&format!("// {paginator} fetches pages sequentially and rejects repeated tokens. Do not share it across goroutines.\ntype {paginator} struct {{ client *{client_name}; input map[string]any; seen map[string]bool; done bool }}\n// {pages} snapshots input and creates a paginator without sending a request.\nfunc (c *{client_name}) {pages}(input {input}) (*{paginator}, error) {{\n values, err := encodeModel(c.schema, {}, input)\n if err != nil {{ return nil, err }}\n seen := map[string]bool{{}}\n if token, ok := values[{}].(string); ok && token != \"\" {{ seen[token] = true }}\n return &{paginator}{{client:c, input:values, seen:seen}}, nil\n}}\n// HasMorePages reports whether pagination has reached the end.\nfunc (p *{paginator}) HasMorePages() bool {{ return !p.done }}\n// NextPage fetches one page using ctx and advances its token; returns an error for exhausted or repeated tokens.\nfunc (p *{paginator}) NextPage(ctx context.Context) ({output}, error) {{\n var result {output}\n if p.done {{ return result, errNoMorePages }}\n values, err := p.client.callValues(ctx, {}, p.input)\n if err != nil {{ return result, err }}\n if err = decodeModel(p.client.schema, {}, values, &result); err != nil {{ return result, err }}\n token, err := pageToken(values, {})\n if err != nil {{ return result, err }}\n if token == \"\" {{ p.done = true }} else if p.seen[token] {{ p.done = true; return result, errRepeatedToken }} else {{ p.seen[token] = true; p.input[{}] = token }}\n return result, nil\n}}\n\n",literal(&op.input.as_ref().expect("paginated input").to_string()),literal(&input_token),literal(op.name()),literal(&target),literal(&output_token),literal(&input_token)));
        }
    }
    let provenance = serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    let mut files = vec![
        File {
            path: "go.mod".into(),
            contents: format!("module {}\n\ngo 1.24.0\n", options.module_path),
        },
        File {
            path: "models.go".into(),
            contents: models,
        },
        File {
            path: "client.go".into(),
            contents: client,
        },
        File {
            path: "runtime.go".into(),
            contents: format!(
                "{header}{}",
                crucible_codegen_common::service_exception_runtime_named(
                    &exception,
                    include_str!("../runtime/runtime.go")
                )
                .replace("\r\n", "\n")
                .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "auth.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/auth.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "policy.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/policy.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "telemetry.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/telemetry.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "crucible.json".into(),
            contents: provenance,
        },
    ];
    if let Some(contents) = permissions::emit(service).filter(|_| with_permissions) {
        files.push(File {
            path: permissions::PATH.into(),
            contents,
        });
    }
    Ok(files)
}

fn wire_path(service: &Service, root: &crucible_ir::ShapeId, path: &str) -> String {
    let mut target = root;
    let mut result = Vec::new();
    for component in path.split('.') {
        let Shape::Structure(shape) = &service.shapes[target] else {
            unreachable!("lowered pagination path")
        };
        let member = shape.member(component).expect("lowered pagination member");
        result.push(member.name.as_str());
        target = &member.target;
    }
    result.join(".")
}

fn go_doc(
    out: &mut String,
    package: (&Service, links::Package),
    indent: &str,
    name: &str,
    docs: Option<&str>,
) {
    go_doc_for(out, package, indent, name, docs, &DocNotes::default());
}

/// The same, plus "Unstable."/"Since X." lines and, last, a `Deprecated:`
/// paragraph: the form `go doc`, gopls and staticcheck recognise.
fn go_doc_for(
    out: &mut String,
    (service, package): (&Service, links::Package),
    indent: &str,
    name: &str,
    docs: Option<&str>,
    notes: &DocNotes,
) {
    let mut docs = docs.map(|docs| links::text(service, package, docs));
    for line in notes.lines() {
        let text = docs.get_or_insert_with(String::new);
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&line);
    }
    if let Some(deprecated) = &notes.deprecated {
        let text = docs.get_or_insert_with(String::new);
        // The header line is a paragraph of its own, so `Deprecated:` begins one.
        text.push_str(if text.is_empty() { "\n" } else { "\n\n" });
        // A model message may span lines; each stays inside the paragraph.
        text.push_str(&format!(
            "Deprecated: {}",
            crucible_codegen_common::documentation::text(&deprecated.note())
        ));
    }
    if let Some(docs) = docs {
        out.push_str(&format!("{indent}// {name}:\n"));
        for line in docs.lines() {
            if line.is_empty() {
                out.push_str(&format!("{indent}//\n"));
            } else {
                out.push_str(&format!("{indent}// {line}\n"));
            }
        }
    }
}
