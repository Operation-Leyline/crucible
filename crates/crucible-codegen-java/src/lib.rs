//! Java 17 models and synchronous/asynchronous HTTP clients packaged for Maven.
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
        Shape::Unit => format!("{package}.model.Unit"),
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
        }
        .into(),
        Shape::List(list) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => {
            format!(
                "java.util.List<{}>",
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
            SimpleKind::Document => format!(
                "com.google.gson.JsonParser.parseString({})",
                literal(&value.to_string())
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
        Shape::List(_) if value.as_array()?.is_empty() => {
            if recursive_collection(service, id, id, &mut BTreeSet::new()) {
                format!("new {native}()")
            } else {
                "new java.util.ArrayList<>()".into()
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

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
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
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
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
                let native = field(&m.name);
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
        if let Shape::Enum(e) = shape {
            let mut values = BTreeSet::new();
            for v in &e.values {
                if !values.insert(field(&v.name).to_uppercase()) {
                    diagnostics.push(id, "Java enum constant name collision");
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
        {
            if !methods.insert(format!("{name}{suffix}")) {
                diagnostics.push(&op.id, "Java client method name collision");
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
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
            continue;
        }
        let name = service.name_of(id);
        let native = ty(service, id, package);
        let class = match shape {
            Shape::List(_) if !recursive_collection(service, id, id, &mut BTreeSet::new()) => {
                "java.util.ArrayList".to_string()
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
            Shape::List(l)=>models.push_str(&format!("    public final class {name} extends java.util.ArrayList<{}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",ty(service,&l.member.target,package))),
            Shape::Map(m)=>models.push_str(&format!("    public final class {name} extends java.util.LinkedHashMap<{}, {}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",ty(service,&m.key.target,package),ty(service,&m.value.target,package))),
            Shape::Enum(e)=>{
                models.push_str(&format!("    public record {name}(java.lang.String value) {{\n        public {name} {{ java.util.Objects.requireNonNull(value); }}\n        @Override public java.lang.String toString() {{ return value; }}\n"));
                for v in &e.values {models.push_str(&format!("        public static final {name} {} = new {name}({});\n",field(&v.name).to_uppercase(),literal(&v.value)));}
                models.push_str("    }\n");
            }
            Shape::Structure(s) | Shape::Union(s)=>{
                models.push_str(&format!("    public final class {name}{} {{\n",if s.error.is_some(){format!(" extends {package}.ServiceError")}else{String::new()}));
                if s.error.is_some(){models.push_str("        private static final long serialVersionUID = 1L;\n");}
                for m in &s.members {
                    let t=ty(service,&m.target,package); let f=field(&m.name);
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
                    let t=ty(service,&m.target,package); let f=field(&m.name);
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
                        s.members.iter().map(|other| format!("value.{} = {package}.OptionalValue.unset(); ", field(&other.name))).collect::<String>() + "value.unknownVariant = null; "
                    } else { String::new() };
                    builder.push_str(&format!("            public Builder {f}({t} item) {{ {clear}{set_default}value.{f} = {assigned}; return this; }}\n"));
                }
                builder.push_str(&format!("            public {name} build() {{\n                {name} result = new {name}();\n"));
                for (index, m) in s.members.iter().enumerate() {
                    let f=field(&m.name);
                    let assigned = if !m.optional_on_client() && m.client_default().is_some() && !matches!(shape, Shape::Union(_)) {
                        format!("if ($defaultSet{index}) ")
                    } else { String::new() };
                    builder.push_str(&format!("                {assigned}result.{f} = value.{f};\n"));
                }
                if matches!(shape, Shape::Union(_)) { builder.push_str("                result.unknownVariant = value.unknownVariant;\n"); }
                builder.push_str("                return result;\n            }\n");
                if matches!(shape, Shape::Union(_)) {
                    let clear = s.members.iter().map(|other| format!("value.{} = {package}.OptionalValue.unset(); ", field(&other.name))).collect::<String>();
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
        if op.paginated.is_some() {
            if let Some(input) = &input {
                client.push_str(&format!("    public java.util.Iterator<{output}> {name}Pages(java.util.function.Consumer<{input}.Builder> configure) {{\n        var builder = {input}.builder(); configure.accept(builder); return {name}Pages(builder.build());\n    }}\n"));
            }
            client.push_str(&format!("    public java.util.Iterator<{output}> {name}Pages({parameter}) {{\n        return runtime.pages({}, {argument}, {output}.class);\n    }}\n",literal(op.name())));
        }
    }
    client.push_str("}\n");
    let pom = format!(
        "<project xmlns=\"http://maven.apache.org/POM/4.0.0\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd\">\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>{}</groupId><artifactId>{}</artifactId><version>{}</version>\n  <properties><maven.compiler.release>17</maven.compiler.release><project.build.sourceEncoding>UTF-8</project.build.sourceEncoding></properties>\n  <dependencies><dependency><groupId>com.google.code.gson</groupId><artifactId>gson</artifactId><version>2.13.2</version></dependency></dependencies>\n  <build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-compiler-plugin</artifactId><version>3.14.1</version></plugin></plugins></build>\n</project>\n",
        xml(&options.group_id),
        xml(&options.artifact_id),
        xml(&options.package_version)
    );
    let provenance=serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    let mut files = vec![
        file("pom.xml".into(), pom),
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
        (
            "OptionalValue",
            include_str!("../runtime/OptionalValue.java"),
        ),
        ("ServiceError", include_str!("../runtime/ServiceError.java")),
        ("NetworkError", include_str!("../runtime/NetworkError.java")),
        ("UnknownUnion", include_str!("../runtime/UnknownUnion.java")),
    ] {
        files.push(file(
            format!("{source}/{name}.java"),
            format!(
                "{header}{}",
                body.replace("\r\n", "\n")
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
