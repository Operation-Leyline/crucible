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
    }
}

fn ty(service: &Service, id: &ShapeId) -> String {
    let shape = service.shape(id).expect("lowered target");
    if id.is_prelude() {
        return match shape {
            Shape::Simple(s) => simple(s.kind).into(),
            Shape::Unit => "Unit".into(),
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

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let mut errors = Diagnostics::default();
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
        "Client",
        "Config",
        "HTTPClient",
        "Optional",
        "Some",
        "Null",
        "Unit",
        "ServiceError",
        "ResponseMetadata",
        "UnknownUnion",
        "NewClient",
        "NetworkError",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
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
    for op in &service.operations {
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
    let mut models = header.clone();
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
            Shape::Unit => models.push_str(&format!("type {name} = Unit\n\n")),
            Shape::Simple(s) => models.push_str(&format!("type {name} = {}\n\n", simple(s.kind))),
            Shape::List(l) => models.push_str(&format!(
                "type {name} []{}\n\n",
                ty(service, &l.member.target)
            )),
            Shape::Map(m) => models.push_str(&format!(
                "type {name} map[{}]{}\n\n",
                ty(service, &m.key.target),
                ty(service, &m.value.target)
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
                    let t = ty(service, &m.target);
                    let t = if m.optional_on_client() || matches!(shape, Shape::Union(_)) {
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
                if s.error.is_some() {
                    models.push_str(&format!("func (e *{name}) Error() string {{ return e.Metadata.Error() }}\nfunc (e *{name}) Unwrap() error {{ return e.Metadata }}\n"));
                }
                models.push('\n');
            }
        }
    }
    let mut client = header.clone();
    if !service.operations.is_empty() {
        client.push_str("import \"context\"\n\n");
    }
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, exported))
        .expect("schema serializes");
    client.push_str(&format!(
        "var wireSchema = mustSchema({})\n\n",
        literal(&schema)
    ));
    client.push_str(&format!("func NewClient(config Config) (*Client, error) {{\n if config.Endpoint == \"\" {{ config.Endpoint = {} }}\n return newClient(config, wireSchema, {})\n}}\n\n",literal(options.endpoint.as_deref().unwrap_or("")),options.content_sha256));
    client.push_str("func modeledError(target string, values map[string]any, metadata *ServiceError) error {\n switch target {\n");
    for (id, shape) in &service.shapes {
        if matches!(shape,Shape::Structure(s) if s.error.is_some()) {
            let name = exported(service.name_of(id));
            client.push_str(&format!(" case {}:\n  result := &{name}{{Metadata: metadata}}\n  if err := decodeModel(wireSchema, target, values, result); err != nil {{ return err }}\n  return result\n",literal(&id.to_string())));
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
        client.push_str(&format!("func (c *Client) {name}(ctx context.Context, input {input}) ({output}, error) {{\n var output {output}\n values, err := c.call(ctx, {}, input)\n if err != nil {{ return output, err }}\n err = decodeModel(c.schema, {}, values, &output)\n return output, err\n}}\n\n",literal(op.name()),literal(&target)));
        if let Some(p) = &op.paginated {
            client.push_str(&format!("type {name}Paginator struct {{ client *Client; input map[string]any; seen map[string]bool; done bool }}\nfunc (c *Client) {name}Pages(input {input}) (*{name}Paginator, error) {{\n values, err := encodeModel(c.schema, {}, input)\n if err != nil {{ return nil, err }}\n seen := map[string]bool{{}}\n if token, ok := values[{}].(string); ok && token != \"\" {{ seen[token] = true }}\n return &{name}Paginator{{client:c, input:values, seen:seen}}, nil\n}}\nfunc (p *{name}Paginator) HasMorePages() bool {{ return !p.done }}\nfunc (p *{name}Paginator) NextPage(ctx context.Context) ({output}, error) {{\n var result {output}\n if p.done {{ return result, errNoMorePages }}\n values, err := p.client.callValues(ctx, {}, p.input)\n if err != nil {{ return result, err }}\n if err = decodeModel(p.client.schema, {}, values, &result); err != nil {{ return result, err }}\n token, err := pageToken(values, {})\n if err != nil {{ return result, err }}\n if token == \"\" {{ p.done = true }} else if p.seen[token] {{ p.done = true; return result, errRepeatedToken }} else {{ p.seen[token] = true; p.input[{}] = token }}\n return result, nil\n}}\n\n",literal(&op.input.as_ref().expect("paginated input").to_string()),literal(&p.input_token),literal(op.name()),literal(&target),literal(&p.output_token),literal(&p.input_token)));
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
                include_str!("../runtime/runtime.go")
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
