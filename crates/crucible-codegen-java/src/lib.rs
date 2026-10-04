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
fn method(text: &str) -> String {
    let mut chars = text.chars();
    field(&format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    ))
}
fn ty(service: &Service, id: &ShapeId) -> String {
    match service.shape(id).expect("lowered target") {
        Shape::Unit => "Models.Unit".into(),
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
        }
        .into(),
        _ => format!("Models.{}", service.name_of(id)),
    }
}
fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
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
    let mut types = BTreeSet::from(["Unit".to_string(), "Models".to_string()]);
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
            for m in &s.members {
                let native = field(&m.name);
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
    let mut models = format!(
        "{header}public final class Models {{\n    private Models() {{}}\n    public record Unit() {{}}\n"
    );
    let mut registry = String::from(
        "    static java.util.Map<java.lang.String, java.lang.Class<?>> classes() {\n        java.util.Map<java.lang.String, java.lang.Class<?>> result = new java.util.HashMap<>();\n",
    );
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
            continue;
        }
        let name = service.name_of(id);
        registry.push_str(&format!(
            "        result.put({}, {name}.class);\n",
            literal(&id.to_string())
        ));
        match shape {
            Shape::List(l)=>models.push_str(&format!("    public static final class {name} extends java.util.ArrayList<{}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",ty(service,&l.member.target))),
            Shape::Map(m)=>models.push_str(&format!("    public static final class {name} extends java.util.LinkedHashMap<{}, {}> {{\n        private static final long serialVersionUID = 1L;\n    }}\n",ty(service,&m.key.target),ty(service,&m.value.target))),
            Shape::Enum(e)=>{
                models.push_str(&format!("    public record {name}(java.lang.String value) {{\n        public {name} {{ java.util.Objects.requireNonNull(value); }}\n        @Override public java.lang.String toString() {{ return value; }}\n"));
                for v in &e.values {models.push_str(&format!("        public static final {name} {} = new {name}({});\n",field(&v.name).to_uppercase(),literal(&v.value)));}
                models.push_str("    }\n");
            }
            Shape::Structure(s) | Shape::Union(s)=>{
                models.push_str(&format!("    public static final class {name}{} {{\n",if s.error.is_some(){format!(" extends {package}.ServiceError")}else{String::new()}));
                if s.error.is_some(){models.push_str("        private static final long serialVersionUID = 1L;\n");}
                for m in &s.members {
                    let t=ty(service,&m.target); let f=field(&m.name);
                    if m.optional_on_client() || matches!(shape,Shape::Union(_)) {models.push_str(&format!("        public {package}.OptionalValue<{t}> {f} = {package}.OptionalValue.unset();\n"));}
                    else {models.push_str(&format!("        public {t} {f};\n"));}
                }
                if matches!(shape,Shape::Union(_)){models.push_str(&format!("        public {package}.UnknownUnion unknownVariant;\n"));}
                models.push_str("    }\n");
            }
            _=>unreachable!(),
        }
    }
    registry.push_str("        return java.util.Map.copyOf(result);\n    }\n");
    models.push_str(&registry);
    models.push_str("}\n");
    let mut client = format!(
        "{header}public final class Client {{\n    private final ClientRuntime runtime;\n    public Client(ClientConfig config) {{\n        runtime = new ClientRuntime(config, {}, {}, Models.classes());\n    }}\n",
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256
    );
    for op in &service.operations {
        let name = method(op.name());
        let input = op.input.as_ref().map(|id| ty(service, id));
        let output = op
            .output
            .as_ref()
            .map_or("Models.Unit".into(), |id| ty(service, id));
        let parameter = input
            .as_ref()
            .map_or(String::new(), |t| format!("{t} input"));
        let argument = if input.is_some() { "input" } else { "null" };
        client.push_str(&format!("    public java.util.concurrent.CompletableFuture<{output}> {name}Async({parameter}) {{\n        return runtime.call({}, {argument}, {output}.class);\n    }}\n    public {output} {name}({parameter}) {{\n        return ClientRuntime.await({name}Async({}));\n    }}\n",literal(op.name()),if input.is_some(){"input"}else{""}));
        if op.paginated.is_some() {
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
        file(format!("{source}/Models.java"), models),
        file(format!("{source}/Client.java"), client),
        file(format!("{resource}/schema.json"), schema),
        file(format!("{resource}/crucible.json"), provenance),
    ];
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
            format!("{header}{}", body.replace("\r\n", "\n")),
        ));
    }
    Ok(files)
}
