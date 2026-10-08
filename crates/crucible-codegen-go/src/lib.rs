//! Native Go models, context-aware clients, and a standard-library runtime.
use crucible_smithy::ir::{Service, Shape, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub module_path: String,
    pub package_name: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}

pub struct File {
    pub path: String,
    pub contents: String,
}

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}

fn exported(value: &str) -> String {
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

fn simple(kind: SimpleKind) -> &'static str {
    match kind {
        SimpleKind::BigInteger => "*big.Int",
        SimpleKind::BigDecimal => "Decimal",
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
    }
}

fn ty(service: &Service, id: &ShapeId) -> String {
    let shape = service.shape(id).expect("lowered target");
    if id.is_prelude() {
        return match shape {
            Shape::Simple(s) => simple(s.kind).into(),
            Shape::IntEnum(_) => unreachable!("integer enums are capability-gated"),
            Shape::Unit { .. } => "Unit".into(),
            _ => unreachable!("prelude shape"),
        };
    }
    let name = exported(service.name_of(id));
    if matches!(shape, Shape::Structure(_) | Shape::Union(_)) {
        format!("*{name}")
    } else {
        name
    }
}

fn is_stream(service: &Service, id: &ShapeId) -> bool {
    matches!(service.shape(id), Some(Shape::Simple(s)) if s.streaming)
}

/// A streaming blob is an `io.Reader` the caller supplies, or an
/// `io.ReadCloser` the caller reads and closes when the structure is an output.
fn member_ty(
    service: &Service,
    id: &ShapeId,
    outputs: &BTreeSet<&ShapeId>,
    container: &ShapeId,
) -> String {
    if !is_stream(service, id) {
        ty(service, id)
    } else if outputs.contains(container) {
        "io.ReadCloser".into()
    } else {
        "io.Reader".into()
    }
}

fn nullable_ty(service: &Service, id: &ShapeId, sparse: bool) -> String {
    let value = ty(service, id);
    if sparse && !value.starts_with('*') {
        format!("*{value}")
    } else {
        value
    }
}

pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
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
        Feature::Streaming,
        Feature::EndpointDiscovery,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut errors = Diagnostics::default();
    let client_name = format!("{}Client", exported(service.id.name()));
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
        "BasicCredentials",
        "AWSCredentials",
        "CredentialsProvider",
        "CredentialChain",
        "EnvironmentCredentials",
        "ProfileCredentials",
        "CachedCredentials",
        "NewCachedCredentials",
        "SigningOptions",
        "SignV4",
        "WaiterOptions",
        "WaiterError",
        "WaiterTimeoutError",
        "ChecksumPolicy",
        "ChecksumWhenSupported",
        "ChecksumWhenRequired",
        "ChecksumError",
        "ByteStream",
        "NewByteStream",
        "NewReaderStream",
        "ErrStreamLength",
        "ErrStreamConsumed",
        "ErrStreamDigest",
        "RetryMode",
        "RetryModeLegacy",
        "RetryModeStandard",
        "RetryModeAdaptive",
        "Clock",
        "EndpointDiscoveryMode",
        "EndpointDiscoveryAuto",
        "EndpointDiscoveryEnabled",
        "EndpointDiscoveryDisabled",
        "ErrEndpointDiscovery",
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
        let name = exported(service.name_of(id));
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
    // The runtime carries a stream only as the whole HTTP body.
    for op in &service.operations {
        for (shape, bindings) in [(&op.input, &op.request), (&op.output, &op.response)] {
            let Some(Shape::Structure(s)) = shape.as_ref().and_then(|id| service.shape(id)) else {
                continue;
            };
            for m in s.members.iter().filter(|m| is_stream(service, &m.target)) {
                if !bindings
                    .payload
                    .as_ref()
                    .is_some_and(|p| p.streaming && p.member == m.name)
                {
                    errors.push(
                        &op.id,
                        format!(
                            "Go streams only an HTTP payload; {} is not the streaming payload",
                            m.name
                        ),
                    );
                }
            }
        }
    }
    for op in &service.operations {
        for waiter in op.waiters.keys() {
            if !methods.insert(format!("WaitUntil{}", exported(waiter))) {
                errors.push(&op.id, "Go waiter method name collision");
            }
        }
        let name = exported(op.name());
        if !methods.insert(name.clone()) {
            errors.push(&op.id, "Go operation method name collision");
        }
        if op.paginated.is_some()
            && (!methods.insert(format!("{name}Pages"))
                || !names.insert(format!("{name}Paginator")))
        {
            errors.push(&op.id, "Go paginator name collision");
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let header = format!(
        "// Code generated by Crucible. DO NOT EDIT.\npackage {}\n\n",
        options.package_name
    );
    let outputs: BTreeSet<&ShapeId> = service
        .operations
        .iter()
        .filter_map(|op| op.output.as_ref())
        .collect();
    let mut models = header.clone();
    if service.shapes.iter().any(|(_, shape)| {
        matches!(shape, Shape::Structure(s) if s.members.iter().any(|m| is_stream(service, &m.target)))
    }) {
        models.push_str("import \"io\"\n\n");
    }
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
        let name = exported(service.name_of(id));
        match shape {
            Shape::IntEnum(e) => {
                models.push_str(&format!(
                    "// {name} preserves unrecognized integer values.\ntype {name} int32\nconst (\n"
                ));
                for v in &e.values {
                    models.push_str(&format!(
                        "    {name}{} {name} = {}\n",
                        exported(&v.name),
                        v.value
                    ));
                }
                models.push_str(")\n\n");
            }
            Shape::Unit { .. } => models.push_str(&format!("type {name} = Unit\n\n")),
            // Members use io.Reader or io.ReadCloser directly; see member_ty.
            Shape::Simple(s) if s.streaming => {}
            Shape::Simple(s) => models.push_str(&format!("type {name} = {}\n\n", simple(s.kind))),
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
                    let t = member_ty(service, &m.target, &outputs, id);
                    // A stream is never null: a nil input reader sends an empty body.
                    let t = if is_stream(service, &m.target) {
                        t
                    } else if m.optional_on_client() || matches!(shape, Shape::Union(_)) {
                        format!("Optional[{t}]")
                    } else {
                        t
                    };
                    models.push_str(&format!("    {} {t}\n", exported(&m.name)));
                }
                if matches!(shape, Shape::Union(_)) {
                    models.push_str("    UnknownVariant *UnknownUnion\n");
                }
                if s.error.is_some() {
                    models.push_str("    Metadata *ServiceError\n");
                }
                models.push_str("}\n");
                // Keep tri-state storage available, while ordinary callers use
                // native arguments and return values as in conventional Go APIs.
                let mut used: BTreeSet<String> =
                    s.members.iter().map(|m| exported(&m.name)).collect();
                used.extend(["Error", "Unwrap", "Metadata", "UnknownVariant"].map(str::to_owned));
                for m in &s.members {
                    let native = exported(&m.name);
                    let t = member_ty(service, &m.target, &outputs, id);
                    let optional = !is_stream(service, &m.target)
                        && (m.optional_on_client() || matches!(shape, Shape::Union(_)));
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
                        match action {
                            "Set" => {
                                let value = if optional { "Some(value)" } else { "value" };
                                let clear = if matches!(shape, Shape::Union(_)) { "*m = ".to_owned() + &name + "{}; " } else { String::new() };
                                models.push_str(&format!("func (m *{name}) {method}(value {t}) {{ {clear}m.{native} = {value} }}\n"));
                            }
                            "Get" => {
                                let value = if optional { format!("m.{native}.Value") } else { format!("m.{native}") };
                                if optional {
                                    models.push_str(&format!("// {method} returns the value, or its zero value when absent or null.\nfunc (m *{name}) {method}() {t} {{ if m != nil && m.{native}.Set && !m.{native}.Null {{ return {value} }}; var zero {t}; return zero }}\n"));
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
                    models.push_str(&format!("func (*{name}) serviceException() {{}}\nfunc (e *{name}) Error() string {{ return e.Metadata.Error() }}\nfunc (e *{name}) Unwrap() error {{ return e.Metadata }}\n"));
                }
                models.push('\n');
            }
        }
    }
    let mut client = header.clone();
    if !service.operations.is_empty() {
        client.push_str("import \"context\"\n\n");
    }
    if service.operations.iter().any(|op| !op.waiters.is_empty()) {
        client.push_str("import (\"time\"; \"fmt\")\n\n");
    }
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, exported))
        .expect("schema serializes");
    client.push_str(&format!(
        "var wireSchema = mustSchema({})\n\n",
        literal(&schema)
    ));
    client.push_str(&format!("type {client_name} struct {{ *clientRuntime }}\n\nfunc New{client_name}(config Config) (*{client_name}, error) {{\n if config.Endpoint == \"\" {{ config.Endpoint = {} }}\n runtime, err := newClient(config, wireSchema, {})\n if err != nil {{ return nil, err }}\n return &{client_name}{{clientRuntime: runtime}}, nil\n}}\n\n",literal(options.endpoint.as_deref().unwrap_or("")),options.content_sha256));
    client.push_str("func modeledError(target string, values map[string]any, metadata *ServiceError) error {\n switch target {\n");
    for (id, shape) in &service.shapes {
        if matches!(shape,Shape::Structure(s) if s.error.is_some()) {
            let name = exported(service.name_of(id));
            client.push_str(&format!(" case {}:\n  result := &{name}{{Metadata: metadata}}\n  if err := decodeModel(wireSchema, target, values, result); err != nil {{ return metadata }}\n  return result\n",literal(&id.to_string())));
        }
    }
    client.push_str(" }\n return metadata\n}\n\n");
    for op in &service.operations {
        let name = exported(op.name());
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
        // A streamed response is returned at its headers; the caller reads and
        // closes the body, and the runtime closes it if decoding fails.
        let streamed = op.response.payload.as_ref().is_some_and(|p| p.streaming);
        let release = if streamed {
            "\n if err != nil { closeStreams(values) }"
        } else {
            ""
        };
        client.push_str(&format!("func (c *{client_name}) {name}(ctx context.Context, input {input}) ({output}, error) {{\n var output {output}\n values, err := c.call(ctx, {}, input)\n if err != nil {{ return output, err }}\n err = decodeModel(c.schema, {}, values, &output){release}\n return output, err\n}}\n\n",literal(op.name()),literal(&target)));
        for waiter in op.waiters.keys() {
            client.push_str(&format!("func (c *{client_name}) WaitUntil{}(ctx context.Context, input {input}, timeout time.Duration, options ...WaiterOptions) ({output}, error) {{\n var result {output}; var config WaiterOptions\n if len(options) > 1 {{ return result, fmt.Errorf(\"only one waiter options value is accepted\") }}\n if len(options) == 1 {{ config = options[0] }}\n value, err := c.wait(ctx, {}, {}, input, timeout, config, func(values map[string]any) (any, error) {{ var output {output}; err := decodeModel(c.schema, {}, values, &output); return output, err }})\n if err != nil || value == nil {{ return result, err }}\n return value.({output}), nil\n}}\n\n",exported(waiter),literal(op.name()),literal(waiter),literal(&target)));
        }
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
            client.push_str(&format!("type {name}Paginator struct {{ client *{client_name}; input map[string]any; seen map[string]bool; done bool }}\nfunc (c *{client_name}) {name}Pages(input {input}) (*{name}Paginator, error) {{\n values, err := encodeModel(c.schema, {}, input)\n if err != nil {{ return nil, err }}\n seen := map[string]bool{{}}\n if token, ok := values[{}].(string); ok && token != \"\" {{ seen[token] = true }}\n return &{name}Paginator{{client:c, input:values, seen:seen}}, nil\n}}\nfunc (p *{name}Paginator) HasMorePages() bool {{ return !p.done }}\nfunc (p *{name}Paginator) NextPage(ctx context.Context) ({output}, error) {{\n var result {output}\n if p.done {{ return result, errNoMorePages }}\n values, err := p.client.callValues(ctx, {}, p.input)\n if err != nil {{ return result, err }}\n if err = decodeModel(p.client.schema, {}, values, &result); err != nil {{ return result, err }}\n token, err := pageToken(values, {})\n if err != nil {{ return result, err }}\n if token == \"\" {{ p.done = true }} else if p.seen[token] {{ p.done = true; return result, errRepeatedToken }} else {{ p.seen[token] = true; p.input[{}] = token }}\n return result, nil\n}}\n\n",literal(&op.input.as_ref().expect("paginated input").to_string()),literal(&input_token),literal(op.name()),literal(&target),literal(&output_token),literal(&input_token)));
        }
    }
    let provenance = serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    Ok(vec![
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
            path: "checksums.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/checksums.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "query.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/query.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "endpoints.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/endpoints.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "stream.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/stream.go")
                    .replace("\r\n", "\n")
                    .replacen("package sdk\n", "", 1)
            ),
        },
        File {
            path: "waiters.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/waiters.go")
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
            path: "discovery.go".into(),
            contents: format!(
                "{header}{}",
                include_str!("../runtime/discovery.go")
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
    ])
}

fn wire_path(service: &Service, root: &crucible_smithy::ShapeId, path: &str) -> String {
    let mut target = root;
    let mut result = Vec::new();
    for component in path.split('.') {
        let Shape::Structure(shape) = &service.shapes[target] else {
            unreachable!("lowered pagination path")
        };
        let member = shape.member(component).expect("lowered pagination member");
        result.push(member.wire_name.as_str());
        target = &member.target;
    }
    result.join(".")
}
