//! Native .NET 8 models, asynchronous clients, and NuGet packaging.
use crucible_smithy::ir::{Service, Shape, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub namespace: String,
    pub package_id: String,
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
fn identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn pascal(text: &str) -> String {
    let mut chars = text.chars();
    format!(
        "{}{}",
        chars.next().expect("model name").to_ascii_uppercase(),
        chars.as_str()
    )
}
fn field(text: &str) -> String {
    let name = pascal(text);
    // Exception and union metadata remain accessible without hiding modeled members.
    if [
        "Message",
        "Data",
        "Source",
        "StackTrace",
        "TargetSite",
        "InnerException",
        "HelpLink",
        "HResult",
        "ErrorName",
        "StatusCode",
        "RequestId",
        "Headers",
        "Retryable",
        "Throttling",
        "UnknownVariant",
        "ToString",
        "Equals",
        "GetHashCode",
        "GetType",
        "GetBaseException",
        "GetObjectData",
    ]
    .contains(&name.as_str())
    {
        format!("{name}Value")
    } else {
        name
    }
}
fn ty(service: &Service, id: &ShapeId, ns: &str) -> String {
    if collection_cycle(service, id, &mut BTreeSet::new()) {
        return format!("global::{ns}.Models.@{}", service.name_of(id));
    }
    match service.shape(id).expect("lowered target") {
        Shape::IntEnum(_) => format!("global::{ns}.Models.@{}", service.name_of(id)),
        Shape::Unit => format!("global::{ns}.Unit"),
        Shape::Simple(s) if s.streaming => format!("global::{ns}.ByteStream"),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => "global::System.Numerics.BigInteger",
            SimpleKind::BigDecimal => return format!("global::{ns}.BigDecimal"),
            SimpleKind::String => "string",
            SimpleKind::Boolean => "bool",
            SimpleKind::Byte => "sbyte",
            SimpleKind::Short => "short",
            SimpleKind::Integer => "int",
            SimpleKind::Long => "long",
            SimpleKind::Float => "float",
            SimpleKind::Double => "double",
            SimpleKind::Timestamp => "global::System.DateTimeOffset",
            SimpleKind::Document => "global::System.Text.Json.Nodes.JsonNode",
            SimpleKind::Blob => "byte[]",
        }
        .into(),
        Shape::List(list) => format!(
            "global::System.Collections.Generic.List<{}{}>",
            ty(service, &list.member.target, ns),
            if list.sparse { "?" } else { "" }
        ),
        Shape::Map(map) => format!(
            "global::System.Collections.Generic.Dictionary<{}, {}{}>",
            ty(service, &map.key.target, ns),
            ty(service, &map.value.target, ns),
            if map.sparse { "?" } else { "" }
        ),
        _ => format!("global::{ns}.Models.@{}", service.name_of(id)),
    }
}
fn collection_cycle(service: &Service, id: &ShapeId, stack: &mut BTreeSet<ShapeId>) -> bool {
    let target = match service.shape(id) {
        Some(Shape::List(list)) => &list.member.target,
        Some(Shape::Map(map)) => &map.value.target,
        _ => return false,
    };
    if !stack.insert(id.clone()) {
        return true;
    }
    let cycle = collection_cycle(service, target, stack);
    stack.remove(id);
    cycle
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
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    let client_name = format!("{}Client", pascal(service.id.name()));
    let mut diagnostics = Diagnostics::default();
    if !options.namespace.split('.').all(identifier)
        || options
            .namespace
            .split('.')
            .any(|n| n.chars().next().is_some_and(char::is_lowercase))
    {
        diagnostics.push(
            &service.id,
            "C# namespace must contain capitalized identifiers separated by dots",
        );
    }
    for (label, value) in [
        ("package id", &options.package_id),
        ("version", &options.package_version),
    ] {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            diagnostics.push(&service.id, format!("invalid NuGet {label}"));
        }
    }
    let mut names = BTreeSet::from([client_name.to_ascii_lowercase()]);
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
            continue;
        }
        let name = service.name_of(id);
        let upper = name.to_ascii_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
            || upper.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && matches!(upper.as_bytes()[3], b'1'..=b'9');
        if !identifier(name) || device || !names.insert(name.to_ascii_lowercase()) {
            diagnostics.push(id, "invalid or colliding C# model name");
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::from([name.to_string()]);
            for m in &s.members {
                if !members.insert(field(&m.name))
                    || ((m.optional_on_client() || matches!(shape, Shape::Union(_)))
                        && !members.insert(format!("{}Presence", field(&m.name))))
                {
                    diagnostics.push(&id.with_member(&m.name), "C# member name collision");
                }
            }
        }
        if let Shape::Enum(e) = shape {
            let mut values = BTreeSet::from([
                name.to_string(),
                "Value".into(),
                "ToString".into(),
                "Equals".into(),
                "GetHashCode".into(),
                "Deconstruct".into(),
                "Clone".into(),
                "PrintMembers".into(),
                "EqualityContract".into(),
            ]);
            for v in &e.values {
                if !values.insert(pascal(&v.name)) {
                    diagnostics.push(id, "C# enum constant name collision");
                }
            }
        }
        if let Shape::IntEnum(e) = shape {
            let mut values = BTreeSet::from([
                name.to_string(),
                "Value".into(),
                "ToString".into(),
                "Equals".into(),
                "GetHashCode".into(),
                "Deconstruct".into(),
                "Clone".into(),
                "PrintMembers".into(),
                "EqualityContract".into(),
            ]);
            for v in &e.values {
                if !values.insert(pascal(&v.name)) {
                    diagnostics.push(id, "C# integer enum constant name collision");
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
                            "C# streams only an HTTP payload; {} is not the streaming payload",
                            m.name
                        ),
                    );
                }
            }
        }
    }
    let mut methods = BTreeSet::from_iter(
        [
            client_name.as_str(),
            "Dispose",
            "Equals",
            "GetHashCode",
            "GetType",
            "ToString",
        ]
        .map(str::to_string),
    );
    for op in &service.operations {
        for suffix in ["", "Async"]
            .into_iter()
            .chain(op.paginated.is_some().then_some("PagesAsync"))
        {
            if !methods.insert(format!("{}{suffix}", pascal(op.name()))) {
                diagnostics.push(&op.id, "C# client method name collision");
            }
        }
        for waiter in op.waiters.keys() {
            for suffix in ["", "Async"] {
                if !methods.insert(format!("WaitUntil{}{suffix}", pascal(waiter))) {
                    diagnostics.push(&op.id, "C# waiter method name collision");
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let ns = &options.namespace;
    let header =
        format!("// Generated by Crucible. DO NOT EDIT.\n#nullable enable\nnamespace {ns};\n\n");
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let mut files = Vec::new();
    let mut registry = String::from(
        "internal static class ModelRegistry {\n    internal static readonly global::System.Collections.Generic.Dictionary<string, global::System.Type> Types = new() {\n",
    );
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
            continue;
        }
        let name = service.name_of(id);
        registry.push_str(&format!(
            "        [{}] = typeof({}),\n",
            literal(&id.to_string()),
            ty(service, id, ns)
        ));
        let mut model = format!(
            "// Generated by Crucible. DO NOT EDIT.\n#nullable enable\nnamespace {ns}.Models;\n\n"
        );
        match shape {
            Shape::IntEnum(e) => {
                model.push_str(&format!("public sealed record @{name}(int Value) {{\n    public override string ToString() => Value.ToString(global::System.Globalization.CultureInfo.InvariantCulture);\n"));
                for v in &e.values {
                    model.push_str(&format!(
                        "    public static readonly @{name} @{} = new({});\n",
                        pascal(&v.name),
                        v.value
                    ));
                }
                model.push_str("}\n");
            }
            Shape::List(list) => {
                if !collection_cycle(service, id, &mut BTreeSet::new()) {
                    continue;
                }
                model.push_str(&format!("public sealed class @{name} : global::System.Collections.Generic.List<{}> {{}}\n", ty(service, &list.member.target, ns)));
            }
            Shape::Map(map) => {
                if !collection_cycle(service, id, &mut BTreeSet::new()) {
                    continue;
                }
                model.push_str(&format!("public sealed class @{name} : global::System.Collections.Generic.Dictionary<{}, {}> {{}}\n", ty(service, &map.key.target, ns), ty(service, &map.value.target, ns)));
            }
            Shape::Enum(e) => {
                model.push_str(&format!("public sealed record @{name}(string Value) {{\n    public override string ToString() => Value;\n"));
                for v in &e.values {
                    model.push_str(&format!(
                        "    public static readonly @{name} @{} = new({});\n",
                        pascal(&v.name),
                        literal(&v.value)
                    ));
                }
                model.push_str("}\n");
            }
            Shape::Structure(s) | Shape::Union(s) => {
                model.push_str(&format!(
                    "public sealed class @{name}{} {{\n",
                    if s.error.is_some() {
                        format!(" : global::{ns}.ServiceError")
                    } else {
                        String::new()
                    }
                ));
                for m in &s.members {
                    let t = ty(service, &m.target, ns);
                    let f = field(&m.name);
                    if m.optional_on_client() || matches!(shape, Shape::Union(_)) {
                        let unwrap = if matches!(service.shape(&m.target), Some(Shape::Simple(s)) if !matches!(s.kind, SimpleKind::String | SimpleKind::Blob | SimpleKind::Document))
                        {
                            ".Value"
                        } else {
                            ""
                        };
                        model.push_str(&format!("    public global::{ns}.Optional<{t}> @{f}Presence {{ get; set; }}\n    public {t}? @{f} {{ get => @{f}Presence.IsSet && !@{f}Presence.IsNull ? @{f}Presence.Value : ({t}?)null; set => @{f}Presence = value is null ? global::{ns}.Optional<{t}>.Null : global::{ns}.Optional<{t}>.Of(value{unwrap}); }}\n"));
                    } else {
                        model.push_str(&format!(
                            "    public {t} @{f} {{ get; set; }} = default!;\n"
                        ));
                    }
                }
                if matches!(shape, Shape::Union(_)) {
                    model.push_str(&format!(
                        "    public global::{ns}.UnknownUnion? UnknownVariant {{ get; set; }}\n"
                    ));
                }
                model.push_str("}\n");
            }
            _ => unreachable!(),
        }
        files.push(File {
            path: format!("Models/{name}.cs"),
            contents: model,
        });
    }
    registry.push_str("    };\n}\n");
    files.push(File {
        path: "ModelRegistry.cs".into(),
        contents: format!("{header}{registry}"),
    });
    let mut client = format!(
        "{header}public sealed class {client_name} : global::System.IDisposable {{\n    private readonly ClientRuntime runtime;\n    public {client_name}(ClientConfig config) {{ runtime = new(config, {}, {}); }}\n    public void Dispose() => runtime.Dispose();\n",
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256
    );
    for op in &service.operations {
        let name = pascal(op.name());
        let input = op.input.as_ref().map(|id| ty(service, id, ns));
        let output = op
            .output
            .as_ref()
            .map_or(format!("global::{ns}.Unit"), |id| ty(service, id, ns));
        let parameter = input
            .as_ref()
            .map_or(String::new(), |t| format!("{t} input, "));
        let arg = if input.is_some() { "input" } else { "null" };
        for waiter in op.waiters.keys() {
            let waiter_name = pascal(waiter);
            client.push_str(&format!("    public global::System.Threading.Tasks.Task<{output}?> WaitUntil{waiter_name}Async({parameter}global::System.TimeSpan timeout, WaiterOptions? options = null, global::System.Threading.CancellationToken cancellationToken = default) => runtime.WaitAsync<{output}>({}, {}, {arg}, timeout, options, cancellationToken);\n    public {output}? WaitUntil{waiter_name}({parameter}global::System.TimeSpan timeout, WaiterOptions? options = null, global::System.Threading.CancellationToken cancellationToken = default) => global::System.Threading.Tasks.Task.Run(() => WaitUntil{waiter_name}Async({}timeout, options, cancellationToken), cancellationToken).GetAwaiter().GetResult();\n",literal(op.name()),literal(waiter),if input.is_some(){"input, "}else{""}));
        }
        client.push_str(&format!("    public global::System.Threading.Tasks.Task<{output}> @{name}Async({parameter}global::System.Threading.CancellationToken cancellationToken = default) => runtime.CallAsync<{output}>({}, {arg}, cancellationToken);\n    public {output} @{name}({parameter}global::System.Threading.CancellationToken cancellationToken = default) => global::System.Threading.Tasks.Task.Run(() => @{name}Async({}cancellationToken), cancellationToken).GetAwaiter().GetResult();\n",literal(op.name()),if input.is_some(){"input, "}else{""}));
        if op.paginated.is_some() {
            client.push_str(&format!("    public global::System.Collections.Generic.IAsyncEnumerable<{output}> @{name}PagesAsync({parameter}global::System.Threading.CancellationToken cancellationToken = default) => runtime.PagesAsync<{output}>({}, {arg}, cancellationToken);\n",literal(op.name())));
        }
    }
    client.push_str("}\n");
    files.push(File {
        path: format!("{client_name}.cs"),
        contents: client,
    });
    files.push(File{path:"Client.csproj".into(),contents:format!("<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework><Nullable>enable</Nullable><ImplicitUsings>enable</ImplicitUsings><LangVersion>12</LangVersion>\n    <PackageId>{}</PackageId><Version>{}</Version><AssemblyName>{}</AssemblyName><RootNamespace>{ns}</RootNamespace><IsTrimmable>false</IsTrimmable>\n  </PropertyGroup>\n  <ItemGroup><EmbeddedResource Include=\"schema.json\" LogicalName=\"crucible.schema.json\"/><EmbeddedResource Include=\"crucible.json\" LogicalName=\"crucible.provenance.json\"/></ItemGroup>\n</Project>\n",options.package_id,options.package_version,options.package_id)});
    let provenance=serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    files.push(File {
        path: "schema.json".into(),
        contents: schema,
    });
    files.push(File {
        path: "crucible.json".into(),
        contents: provenance,
    });
    for (name, body) in [
        ("ClientRuntime", include_str!("../runtime/ClientRuntime.cs")),
        ("ClientConfig", include_str!("../runtime/ClientConfig.cs")),
        ("Optional", include_str!("../runtime/Optional.cs")),
        ("ServiceError", include_str!("../runtime/ServiceError.cs")),
        ("NetworkError", include_str!("../runtime/NetworkError.cs")),
        ("UnknownUnion", include_str!("../runtime/UnknownUnion.cs")),
        ("Unit", include_str!("../runtime/Unit.cs")),
        ("ByteStream", include_str!("../runtime/ByteStream.cs")),
        (
            "ByteStreamContent",
            include_str!("../runtime/ByteStreamContent.cs"),
        ),
        (
            "ResponseBodyStream",
            include_str!("../runtime/ResponseBodyStream.cs"),
        ),
        ("BigDecimal", include_str!("../runtime/BigDecimal.cs")),
        (
            "BasicCredentials",
            include_str!("../runtime/BasicCredentials.cs"),
        ),
        (
            "AwsCredentials",
            include_str!("../runtime/AwsCredentials.cs"),
        ),
        (
            "CredentialProviders",
            include_str!("../runtime/CredentialProviders.cs"),
        ),
        (
            "CachedCredentials",
            include_str!("../runtime/CachedCredentials.cs"),
        ),
        (
            "SigningOptions",
            include_str!("../runtime/SigningOptions.cs"),
        ),
        ("SigV4Signer", include_str!("../runtime/SigV4Signer.cs")),
        ("Query", include_str!("../runtime/Query.cs")),
        ("Endpoints", include_str!("../runtime/Endpoints.cs")),
        ("Checksums", include_str!("../runtime/Checksums.cs")),
        (
            "ChecksumPolicy",
            include_str!("../runtime/ChecksumPolicy.cs"),
        ),
        (
            "ChecksumException",
            include_str!("../runtime/ChecksumException.cs"),
        ),
        ("WaiterOptions", include_str!("../runtime/WaiterOptions.cs")),
        (
            "WaiterException",
            include_str!("../runtime/WaiterException.cs"),
        ),
        (
            "WaiterTimeoutException",
            include_str!("../runtime/WaiterTimeoutException.cs"),
        ),
        (
            "ClientRuntime.Waiters",
            include_str!("../runtime/ClientRuntime.Waiters.cs"),
        ),
    ] {
        files.push(File {
            path: format!("{name}.cs"),
            contents: format!("{header}{}", body.replace("\r\n", "\n")),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
