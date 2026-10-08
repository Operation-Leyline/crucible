//! Java 17 models and synchronous/asynchronous HTTP clients packaged for Gradle and Maven.
use crucible_smithy::ir::{Service, Shape, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub group_id: String,
    pub artifact_id: String,
    pub package_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}
pub struct File {
    pub path: String,
    pub contents: String,
}
fn literal(text: &str) -> String {
    serde_json::to_string(text).expect("string serializes")
}
// Package metadata must not become Kotlin string interpolation.
fn kotlin_literal(text: &str) -> String {
    literal(text).replace('$', "\\$")
}
fn keyword(text: &str) -> bool {
    [
        "abstract",
        "assert",
        "boolean",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "class",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extends",
        "final",
        "finally",
        "float",
        "for",
        "goto",
        "if",
        "implements",
        "import",
        "instanceof",
        "int",
        "interface",
        "long",
        "native",
        "new",
        "package",
        "private",
        "protected",
        "public",
        "return",
        "short",
        "static",
        "strictfp",
        "super",
        "switch",
        "synchronized",
        "this",
        "throw",
        "throws",
        "transient",
        "try",
        "void",
        "volatile",
        "while",
        "true",
        "false",
        "null",
        "_",
        "record",
        "var",
        "yield",
        "sealed",
        "permits",
    ]
    .contains(&text)
}
fn identifier(text: &str) -> bool {
    !keyword(text)
        && text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn field(text: &str) -> String {
    if keyword(text) {
        format!("{text}_")
    } else {
        text.into()
    }
}
fn member_field(text: &str, package: &str) -> String {
    let name = field(text);
    if name == package.split('.').next().expect("validated package") {
        format!("{name}_")
    } else {
        name
    }
}
fn accessor(name: &str, error: bool) -> String {
    if error
        && [
            "errorName",
            "status",
            "requestId",
            "headers",
            "body",
            "retryable",
            "throttling",
        ]
        .contains(&name)
    {
        format!("{name}Value")
    } else {
        name.to_string()
    }
}
fn method(text: &str) -> String {
    let mut chars = text.chars();
    field(&format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    ))
}
fn ty(service: &Service, id: &ShapeId, package: &str) -> String {
    match service.shape(id).expect("lowered target") {
        Shape::Unit { .. } => format!("{package}.model.Unit"),
        Shape::Simple(s) if s.streaming => format!("{package}.ByteStream"),
        Shape::Simple(s) => match s.kind {
            SimpleKind::String => "java.lang.String",
            SimpleKind::Boolean => "java.lang.Boolean",
            SimpleKind::Byte => "java.lang.Byte",
            SimpleKind::Short => "java.lang.Short",
            SimpleKind::Integer => "java.lang.Integer",
            SimpleKind::Long => "java.lang.Long",
            SimpleKind::Float => "java.lang.Float",
            SimpleKind::Double => "java.lang.Double",
            SimpleKind::Timestamp => "java.time.Instant",
            SimpleKind::Document => "com.google.gson.JsonElement",
            SimpleKind::Blob => "byte[]",
            SimpleKind::BigInteger => "java.math.BigInteger",
            SimpleKind::BigDecimal => "java.math.BigDecimal",
        }
        .into(),
        Shape::List(list) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => {
            format!(
                "java.util.{}<{}>",
                if list.is_set { "Set" } else { "List" },
                ty(service, &list.member.target, package)
            )
        }
        Shape::Map(map) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => format!(
            "java.util.Map<{}, {}>",
            ty(service, &map.key.target, package),
            ty(service, &map.value.target, package)
        ),
        _ => format!("{package}.model.{}", service.name_of(id)),
    }
}
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Defaults are already resolved by lowering. Initialize each model instance with
// its own mutable defaults; setters may still explicitly replace them with null.
fn default_expression(
    service: &Service,
    id: &ShapeId,
    value: &serde_json::Value,
    package: &str,
) -> Option<String> {
    let native = ty(service, id, package);
    Some(match service.shape(id)? {
        Shape::Simple(simple) => match simple.kind {
            SimpleKind::String => literal(value.as_str()?),
            SimpleKind::Boolean => value.as_bool()?.to_string(),
            SimpleKind::Byte
            | SimpleKind::Short
            | SimpleKind::Integer
            | SimpleKind::Long
            | SimpleKind::Float
            | SimpleKind::Double => {
                value.as_number()?;
                let exact = match simple.kind {
                    SimpleKind::Byte => Some("byteValueExact"),
                    SimpleKind::Short => Some("shortValueExact"),
                    SimpleKind::Integer => Some("intValueExact"),
                    SimpleKind::Long => Some("longValueExact"),
                    _ => None,
                };
                if value.as_i64().is_none()
                    && let Some(exact) = exact
                {
                    format!(
                        "new java.math.BigDecimal({}).{exact}()",
                        literal(&value.to_string())
                    )
                } else {
                    format!("{native}.valueOf({})", literal(&value.to_string()))
                }
            }
            SimpleKind::BigInteger | SimpleKind::BigDecimal => {
                format!("new {native}({})", literal(&value.to_string()))
            }
            SimpleKind::Document => format!(
                "com.google.gson.JsonParser.parseString({})",
                literal(&value.to_string())
            ),
            SimpleKind::Blob if simple.streaming => format!(
                "{package}.ByteStream.fromBytes(java.util.Base64.getDecoder().decode({}))",
                literal(value.as_str()?)
            ),
            SimpleKind::Blob => format!(
                "java.util.Base64.getDecoder().decode({})",
                literal(value.as_str()?)
            ),
            SimpleKind::Timestamp => {
                if let Some(text) = value.as_str() {
                    format!("java.time.Instant.parse({})", literal(text))
                } else {
                    value.as_number()?;
                    let decimal =
                        format!("new java.math.BigDecimal({})", literal(&value.to_string()));
                    format!(
                        "java.time.Instant.ofEpochSecond({decimal}.longValue(), {decimal}.remainder(java.math.BigDecimal.ONE).movePointRight(9).longValueExact())"
                    )
                }
            }
        },
        Shape::Enum(_) => format!("new {native}({})", literal(value.as_str()?)),
        Shape::IntEnum(_) => format!("new {native}({})", value.as_i64()?),
        Shape::List(list) if value.as_array()?.is_empty() => {
            if recursive_collection(service, id, id, &mut BTreeSet::new()) {
                format!("new {native}()")
            } else {
                if list.is_set {
                    "new java.util.LinkedHashSet<>()".into()
                } else {
                    "new java.util.ArrayList<>()".into()
                }
            }
        }
        Shape::Map(_) if value.as_object()?.is_empty() => {
            if recursive_collection(service, id, id, &mut BTreeSet::new()) {
                format!("new {native}()")
            } else {
                "new java.util.LinkedHashMap<>()".into()
            }
        }
        _ => return None,
    })
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
        Feature::Streaming,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::HttpChecksum,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::AwsRestXml,
        Feature::AwsQuery,
        Feature::Ec2Query,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let exception = crucible_codegen_common::service_exception_name(service);
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let client_name = format!("{}Client", service.id.name());
    let mut diagnostics = Diagnostics::default();
    if !options.package_name.split('.').all(identifier) {
        diagnostics.push(
            &service.id,
            "Java package must contain valid identifiers separated by dots",
        );
    }
    for (name, value) in [
        ("groupId", &options.group_id),
        ("artifactId", &options.artifact_id),
        ("version", &options.package_version),
    ] {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            diagnostics.push(&service.id, format!("invalid Maven {name}"));
        }
    }
    let mut types = BTreeSet::from(["Unit".to_string(), "Builder".to_string()]);
    types.extend([
        "java".to_string(),
        "com".to_string(),
        options
            .package_name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_string(),
    ]);
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        if !identifier(name) || !types.insert(name.to_string()) {
            diagnostics.push(id, "invalid or colliding Java model name");
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::new();
            let mut accessors = BTreeSet::new();
            for m in &s.members {
                if !m.optional_on_client()
                    && let Some(value) = m.client_default()
                    && default_expression(service, &m.target, value, &options.package_name)
                        .is_none()
                {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "unsupported Java native default value",
                    );
                }
                let native = member_field(&m.name, &options.package_name);
                if !accessors.insert(accessor(&native, s.error.is_some())) {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "Java model accessor name collision",
                    );
                }
                if [
                    "builder",
                    "wait",
                    "notify",
                    "notifyAll",
                    "getClass",
                    "hashCode",
                    "toString",
                    "clone",
                    "finalize",
                    "getMessage",
                    "getCause",
                    "getStackTrace",
                    "getSuppressed",
                    "getLocalizedMessage",
                    "fillInStackTrace",
                    "printStackTrace",
                ]
                .contains(&native.as_str())
                {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "Java model accessor name collision",
                    );
                }
                if !members.insert(native.clone())
                    || matches!(shape, Shape::Union(_)) && native == "unknownVariant"
                    || s.error.is_some() && native == "serialVersionUID"
                {
                    diagnostics.push(&id.with_member(&m.name), "Java member name collision");
                }
            }
        }
        let enum_names: Vec<_> = match shape {
            Shape::Enum(e) => e.values.iter().map(|v| &v.name).collect(),
            Shape::IntEnum(e) => e.values.iter().map(|v| &v.name).collect(),
            _ => Vec::new(),
        };
        {
            let mut values = BTreeSet::new();
            for name in enum_names {
                if !values.insert(field(name).to_uppercase()) {
                    diagnostics.push(id, "Java enum constant name collision");
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
            for m in &s.members {
                let streaming =
                    matches!(service.shape(&m.target), Some(Shape::Simple(t)) if t.streaming);
                if streaming
                    && !bindings
                        .payload
                        .as_ref()
                        .is_some_and(|p| p.streaming && p.member == m.name)
                {
                    diagnostics.push(
                        &op.id,
                        format!(
                            "Java streams only an HTTP payload; {} is not the streaming payload",
                            m.name
                        ),
                    );
                }
            }
        }
    }
    let mut methods = BTreeSet::from_iter(
        [
            "wait",
            "notify",
            "notifyAll",
            "getClass",
            "hashCode",
            "equals",
            "toString",
        ]
        .map(str::to_string),
    );
    for op in &service.operations {
        let name = method(op.name());
        for suffix in ["", "Async"]
            .into_iter()
            .chain(op.paginated.is_some().then_some("Pages"))
            .chain(op.paginated.is_some().then_some("PagesAsync"))
        {
            if !methods.insert(format!("{name}{suffix}")) {
                diagnostics.push(&op.id, "Java client method name collision");
            }
        }
    }
    for op in &service.operations {
        for waiter in op.waiters.keys() {
            for suffix in ["", "Async"] {
                let name = format!("waitUntil{waiter}{suffix}");
                if !identifier(&name) || !methods.insert(name) {
                    diagnostics.push(
                        &op.id,
                        "Java waiter method name collision or invalid identifier",
                    );
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = &options.package_name;
    let header = format!("// Generated by Crucible. DO NOT EDIT.\npackage {package};\n\n");
    let source = format!("src/main/java/{}", package.replace('.', "/"));
    let resource = format!("src/main/resources/{}", package.replace('.', "/"));
    let file = |path: String, contents: String| File { path, contents };
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let mut model_files = vec![file(
        format!("{source}/model/Unit.java"),
        format!("package {package}.model;\npublic record Unit() {{}}\n"),
    )];
    let mut registry = String::from(
        "    static java.util.Map<java.lang.String, java.lang.Class<?>> classes() {\n        java.util.Map<java.lang.String, java.lang.Class<?>> result = new java.util.HashMap<>();\n",
    );
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        let native = ty(service, id, package);
        let class = match shape {
            Shape::List(list) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => {
                if list.is_set {
                    "java.util.LinkedHashSet".to_string()
                } else {
                    "java.util.ArrayList".to_string()
                }
            }
            Shape::Map(_) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => {
                "java.util.LinkedHashMap".to_string()
            }
            _ => native.clone(),
        };
        registry.push_str(&format!(
            "        result.put({}, {class}.class);\n",
            literal(&id.to_string())
        ));
        if matches!(shape, Shape::List(_) | Shape::Map(_))
            && !recursive_collection(service, id, id, &mut BTreeSet::new())
        {
            continue;
        }
        let mut models =
            format!("// Generated by Crucible. DO NOT EDIT.\npackage {package}.model;\n\n");
        match shape {
            Shape::List(l)=>models.push_str(&format!("    public final class {name} extends java.util.{}<{}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",if l.is_set { "LinkedHashSet" } else { "ArrayList" },ty(service,&l.member.target,package))),
            Shape::Map(m)=>models.push_str(&format!("    public final class {name} extends java.util.LinkedHashMap<{}, {}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",ty(service,&m.key.target,package),ty(service,&m.value.target,package))),
            Shape::Enum(e)=>{
                models.push_str(&format!("    public record {name}(java.lang.String value) {{\n        public {name} {{ java.util.Objects.requireNonNull(value); }}\n        @Override public java.lang.String toString() {{ return value; }}\n"));
                for v in &e.values {models.push_str(&format!("        public static final {name} {} = new {name}({});\n",field(&v.name).to_uppercase(),literal(&v.value)));}
                models.push_str("    }\n");
            }
            Shape::IntEnum(e)=>{
                models.push_str(&format!("    public record {name}(int value) {{\n        @Override public java.lang.String toString() {{ return java.lang.Integer.toString(value); }}\n"));
                for v in &e.values {models.push_str(&format!("        public static final {name} {} = new {name}({});\n",field(&v.name).to_uppercase(),v.value));}
                models.push_str("    }\n");
            }
            Shape::Structure(s) | Shape::Union(s)=>{
                models.push_str(&format!("    public final class {name}{} {{\n",if s.error.is_some(){format!(" extends {package}.{exception}")}else{String::new()}));
                if s.error.is_some(){models.push_str("        private static final long serialVersionUID = 1L;\n");}
                for m in &s.members {
                    let t=ty(service,&m.target,package); let f=member_field(&m.name,package);
                    if m.optional_on_client() || matches!(shape,Shape::Union(_)) {models.push_str(&format!("        public {package}.OptionalValue<{t}> {f} = {package}.OptionalValue.unset();\n"));}
                    else {
                        let initial = m.client_default().map(|value| format!(" = {}", default_expression(service, &m.target, value, package).expect("validated default"))).unwrap_or_default();
                        models.push_str(&format!("        public {t} {f}{initial};\n"));
                    }
                }
                if matches!(shape,Shape::Union(_)){models.push_str(&format!("        public {package}.UnknownUnion unknownVariant;\n"));}
                models.push_str("        public static Builder builder() { return new Builder(); }\n");
                let mut builder = format!("        public static final class Builder {{\n            private final {name} value = new {name}();\n");
                for (index, m) in s.members.iter().enumerate() {
                    let t=ty(service,&m.target,package); let f=member_field(&m.name,package);
                    let optional = m.optional_on_client() || matches!(shape,Shape::Union(_));
                    // '$' cannot occur in a lowered Smithy member name, keeping
                    // these private flags independent of modeled identifiers.
                    let set_default = if !optional && m.client_default().is_some() {
                        builder.push_str(&format!("            private boolean $defaultSet{index};\n"));
                        format!("$defaultSet{index} = true; ")
                    } else { String::new() };
                    let read = if optional { format!("{f}.value()") } else {f.clone()};
                    let getter = accessor(&f, s.error.is_some());
                    models.push_str(&format!("        public {t} {getter}() {{ return {read}; }}\n"));
                    let assigned = if optional {format!("{package}.OptionalValue.of(item)")}else{"item".into()};
                    let clear = if matches!(shape, Shape::Union(_)) {
                        s.members.iter().map(|other| format!("value.{} = {package}.OptionalValue.unset(); ", member_field(&other.name,package))).collect::<String>() + "value.unknownVariant = null; "
                    } else { String::new() };
                    builder.push_str(&format!("            public Builder {f}({t} item) {{ {clear}{set_default}value.{f} = {assigned}; return this; }}\n"));
                }
                builder.push_str(&format!("            public {name} build() {{\n                {name} result = new {name}();\n"));
                for (index, m) in s.members.iter().enumerate() {
                    let f=member_field(&m.name,package);
                    let assigned = if !m.optional_on_client() && m.client_default().is_some() && !matches!(shape, Shape::Union(_)) {
                        format!("if ($defaultSet{index}) ")
                    } else { String::new() };
                    builder.push_str(&format!("                {assigned}result.{f} = value.{f};\n"));
                }
                if matches!(shape, Shape::Union(_)) { builder.push_str("                result.unknownVariant = value.unknownVariant;\n"); }
                builder.push_str("                return result;\n            }\n");
                if matches!(shape, Shape::Union(_)) {
                    let clear = s.members.iter().map(|other| format!("value.{} = {package}.OptionalValue.unset(); ", member_field(&other.name,package))).collect::<String>();
                    builder.push_str(&format!("            public Builder unknownVariant({package}.UnknownUnion item) {{ {clear}value.unknownVariant = item; return this; }}\n"));
                }
                builder.push_str("        }\n");
                models.push_str(&builder);
                models.push_str("    }\n");
            }
            _=>unreachable!(),
        }
        model_files.push(file(format!("{source}/model/{name}.java"), models));
    }
    registry.push_str("        return java.util.Map.copyOf(result);\n    }\n");
    let registry = format!("{header}final class ModelRegistry {{\n{registry}}}\n");
    let mut client = format!(
        "{header}public final class {client_name} {{\n    private final ClientRuntime runtime;\n    public {client_name}(ClientConfig config) {{\n        runtime = new ClientRuntime(config, {}, {}, ModelRegistry.classes());\n    }}\n",
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256
    );
    for op in &service.operations {
        let name = method(op.name());
        let input = op.input.as_ref().map(|id| ty(service, id, package));
        let output = op
            .output
            .as_ref()
            .map_or(format!("{package}.model.Unit"), |id| {
                ty(service, id, package)
            });
        let parameter = input
            .as_ref()
            .map_or(String::new(), |t| format!("{t} input"));
        let argument = if input.is_some() { "input" } else { "null" };
        client.push_str(&format!("    public java.util.concurrent.CompletableFuture<{output}> {name}Async({parameter}) {{\n        return runtime.call({}, {argument}, {output}.class);\n    }}\n    public {output} {name}({parameter}) {{\n        return ClientRuntime.await({name}Async({}));\n    }}\n",literal(op.name()),if input.is_some(){"input"}else{""}));
        if let Some(input) = &input {
            if op.input.as_ref().is_some_and(|id| matches!(service.shape(id), Some(Shape::Structure(s)) if s.members.iter().all(|m| m.optional_on_client()))) {
                client.push_str(&format!("    public {output} {name}() {{ return {name}({input}.builder().build()); }}\n    public java.util.concurrent.CompletableFuture<{output}> {name}Async() {{ return {name}Async({input}.builder().build()); }}\n"));
            }
            client.push_str(&format!("    public {output} {name}(java.util.function.Consumer<{input}.Builder> configure) {{\n        var builder = {input}.builder(); configure.accept(builder); return {name}(builder.build());\n    }}\n    public java.util.concurrent.CompletableFuture<{output}> {name}Async(java.util.function.Consumer<{input}.Builder> configure) {{\n        var builder = {input}.builder(); configure.accept(builder); return {name}Async(builder.build());\n    }}\n"));
        }
        for waiter in op.waiters.keys() {
            let method_name = format!("waitUntil{waiter}");
            let params = if input.is_some() {
                format!("{parameter}, WaiterOptions options")
            } else {
                "WaiterOptions options".into()
            };
            let args = if input.is_some() {
                "input, options"
            } else {
                "options"
            };
            let default_args = if input.is_some() {
                "input, new WaiterOptions()"
            } else {
                "new WaiterOptions()"
            };
            client.push_str(&format!("    public java.util.concurrent.CompletableFuture<{output}> {method_name}Async({params}) {{ return runtime.waitUntil({}, {}, {argument}, {output}.class, options); }}\n    public {output} {method_name}({params}) {{ return ClientRuntime.await({method_name}Async({args})); }}\n    public java.util.concurrent.CompletableFuture<{output}> {method_name}Async({parameter}) {{ return {method_name}Async({default_args}); }}\n    public {output} {method_name}({parameter}) {{ return {method_name}({default_args}); }}\n", literal(op.name()), literal(waiter)));
            if let Some(input) = &input {
                client.push_str(&format!("    public java.util.concurrent.CompletableFuture<{output}> {method_name}Async(WaiterOptions options, java.util.function.Consumer<{input}.Builder> configure) {{ var builder={input}.builder(); configure.accept(builder); return {method_name}Async(builder.build(), options); }}\n    public {output} {method_name}(WaiterOptions options, java.util.function.Consumer<{input}.Builder> configure) {{ return ClientRuntime.await({method_name}Async(options, configure)); }}\n    public java.util.concurrent.CompletableFuture<{output}> {method_name}Async(java.util.function.Consumer<{input}.Builder> configure) {{ return {method_name}Async(new WaiterOptions(), configure); }}\n    public {output} {method_name}(java.util.function.Consumer<{input}.Builder> configure) {{ return {method_name}(new WaiterOptions(), configure); }}\n"));
            }
        }
        if op.paginated.is_some() {
            if let Some(input) = &input {
                client.push_str(&format!("    public java.util.Iterator<{output}> {name}Pages(java.util.function.Consumer<{input}.Builder> configure) {{\n        var builder = {input}.builder(); configure.accept(builder); return {name}Pages(builder.build());\n    }}\n"));
            }
            client.push_str(&format!("    public java.util.Iterator<{output}> {name}Pages({parameter}) {{\n        return runtime.pages({}, {argument}, {output}.class);\n    }}\n",literal(op.name())));
            client.push_str(&format!("    public java.util.concurrent.Flow.Publisher<{output}> {name}PagesAsync({parameter}) {{\n        return runtime.pagesAsync({}, {argument}, {output}.class);\n    }}\n",literal(op.name())));
            if let Some(input) = &input {
                client.push_str(&format!("    public java.util.concurrent.Flow.Publisher<{output}> {name}PagesAsync(java.util.function.Consumer<{input}.Builder> configure) {{\n        var builder = {input}.builder(); configure.accept(builder); return {name}PagesAsync(builder.build());\n    }}\n"));
                if op.input.as_ref().is_some_and(|id| matches!(service.shape(id), Some(Shape::Structure(s)) if s.members.iter().all(|m| m.optional_on_client()))) {
                    client.push_str(&format!("    public java.util.concurrent.Flow.Publisher<{output}> {name}PagesAsync() {{ return {name}PagesAsync({input}.builder().build()); }}\n"));
                }
            }
        }
    }
    client.push_str("}\n");
    let pom = format!(
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>{}</groupId><artifactId>{}</artifactId><version>{}</version>\n  <properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties>\n  <dependencies><dependency><groupId>com.google.code.gson</groupId><artifactId>gson</artifactId><version>2.13.2</version></dependency></dependencies>\n  <build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.14.1</version></plugin></plugins></build>\n</project>\n",
        xml(&options.group_id),
        xml(&options.artifact_id),
        xml(&options.package_version)
    );
    let gradle = format!(
        "plugins {{\n    `java-library`\n    `maven-publish`\n}}\n\ngroup = {}\nversion = {}\n\nrepositories {{\n    mavenCentral()\n}}\n\ndependencies {{\n    api(\"com.google.code.gson:gson:2.13.2\")\n}}\n\njava {{\n    sourceCompatibility = JavaVersion.VERSION_17\n    targetCompatibility = JavaVersion.VERSION_17\n    withSourcesJar()\n}}\n\ntasks.withType<JavaCompile>().configureEach {{\n    options.release.set(17)\n    options.encoding = \"UTF-8\"\n}}\n\npublishing {{\n    publications {{\n        create<MavenPublication>(\"sdk\") {{\n            from(components[\"java\"])\n        }}\n    }}\n}}\n",
        kotlin_literal(&options.group_id),
        kotlin_literal(&options.package_version),
    );
    let settings = format!(
        "rootProject.name = {}\n",
        kotlin_literal(&options.artifact_id),
    );
    let provenance=serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    let mut files = vec![
        file("pom.xml".into(), pom),
        file("build.gradle.kts".into(), gradle),
        file("settings.gradle.kts".into(), settings),
        file(format!("{source}/ModelRegistry.java"), registry),
        file(format!("{source}/{client_name}.java"), client),
        file(format!("{resource}/schema.json"), schema),
        file(format!("{resource}/crucible.json"), provenance),
    ];
    files.extend(model_files);
    for (name, body) in [
        (
            "ClientRuntime",
            include_str!("../runtime/ClientRuntime.java"),
        ),
        ("ClientConfig", include_str!("../runtime/ClientConfig.java")),
        ("Auth", include_str!("../runtime/Auth.java")),
        ("Checksums", include_str!("../runtime/Checksums.java")),
        (
            "ChecksumPolicy",
            include_str!("../runtime/ChecksumPolicy.java"),
        ),
        (
            "ChecksumMismatch",
            include_str!("../runtime/ChecksumMismatch.java"),
        ),
        ("Query", include_str!("../runtime/Query.java")),
        ("XmlProtocol", include_str!("../runtime/XmlProtocol.java")),
        ("Endpoints", include_str!("../runtime/Endpoints.java")),
        (
            "WaiterOptions",
            include_str!("../runtime/WaiterOptions.java"),
        ),
        ("WaiterError", include_str!("../runtime/WaiterError.java")),
        (
            "WaiterTimeout",
            include_str!("../runtime/WaiterTimeout.java"),
        ),
        (
            "BasicCredentials",
            include_str!("../runtime/BasicCredentials.java"),
        ),
        (
            "AwsCredentials",
            include_str!("../runtime/AwsCredentials.java"),
        ),
        (
            "CachedCredentials",
            include_str!("../runtime/CachedCredentials.java"),
        ),
        (
            "CredentialProviders",
            include_str!("../runtime/CredentialProviders.java"),
        ),
        (
            "OptionalValue",
            include_str!("../runtime/OptionalValue.java"),
        ),
        ("ServiceError", include_str!("../runtime/ServiceError.java")),
        (
            exception.as_str(),
            include_str!("../runtime/ServiceException.java"),
        ),
        ("NetworkError", include_str!("../runtime/NetworkError.java")),
        ("UnknownUnion", include_str!("../runtime/UnknownUnion.java")),
        ("ByteStream", include_str!("../runtime/ByteStream.java")),
        ("StreamBody", include_str!("../runtime/StreamBody.java")),
        ("StreamInput", include_str!("../runtime/StreamInput.java")),
        (
            "InputStreamPublisher",
            include_str!("../runtime/InputStreamPublisher.java"),
        ),
    ] {
        files.push(file(
            format!("{source}/{name}.java"),
            format!(
                "{header}{}",
                crucible_codegen_common::service_exception_runtime(service, body)
                    .replace("\r\n", "\n")
                    .replace("Models.Unit", &format!("{package}.model.Unit"))
            ),
        ));
    }
    Ok(files)
}

// Recursive collection aliases require a nominal JVM type to break the cycle.
fn recursive_collection(
    service: &Service,
    start: &ShapeId,
    current: &ShapeId,
    visited: &mut BTreeSet<ShapeId>,
) -> bool {
    if !visited.insert(current.clone()) {
        return false;
    }
    let targets = match service.shape(current) {
        Some(Shape::List(list)) => vec![&list.member.target],
        Some(Shape::Map(map)) => vec![&map.key.target, &map.value.target],
        _ => return false,
    };
    targets
        .into_iter()
        .any(|target| target == start || recursive_collection(service, start, target, visited))
}
