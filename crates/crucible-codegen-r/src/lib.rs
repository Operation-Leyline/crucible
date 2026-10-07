//! Native R package generation from the shared lowered Smithy IR.
use crucible_smithy::{
    Diagnostics,
    ir::{Service, Shape},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub package_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}
pub struct File {
    pub path: String,
    pub contents: String,
}

pub fn field(name: &str) -> String {
    let mut result = String::new();
    let chars: Vec<_> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase()
            && i > 0
            && (chars[i - 1].is_ascii_lowercase()
                || chars[i - 1].is_ascii_digit()
                || chars.get(i + 1).is_some_and(char::is_ascii_lowercase))
        {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    if [
        "if", "else", "repeat", "while", "function", "for", "in", "next", "break", "true", "false",
        "null", "inf", "nan", "na", "...", "metadata",
    ]
    .contains(&result.as_str())
    {
        result.push_str("_value");
    }
    result
}
fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("string")
}
fn function(name: &str) -> String {
    format!("`{name}`")
}
fn contains_nul(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => value.contains('\0'),
        serde_json::Value::Array(values) => values.iter().any(contains_nul),
        serde_json::Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.contains('\0') || contains_nul(value)),
        _ => false,
    }
}

pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
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
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut diagnostics = Diagnostics::default();
    let client_name = format!("{}Client", service.id.name());
    if service.version.contains('\0')
        || options
            .endpoint
            .as_ref()
            .is_some_and(|value| value.contains('\0'))
    {
        diagnostics.push(
            &service.id,
            "R strings cannot represent NUL in service metadata or endpoint options",
        );
    }
    let package = &options.package_name;
    if package.len() < 2
        || !package.starts_with(|c: char| c.is_ascii_alphabetic())
        || package.ends_with('.')
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.')
    {
        diagnostics.push(&service.id, "R package name must contain at least two letters/digits/dots, begin with a letter, and not end with a dot");
    }
    if options.package_version.split(['.', '-']).count() < 2
        || !options
            .package_version
            .split(['.', '-'])
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
    {
        diagnostics.push(
            &service.id,
            "R package version must contain at least two numeric components",
        );
    }
    let mut exported: BTreeSet<String> = [
        client_name.as_str(),
        "client_config",
        "cancellation_token",
        "long",
        "big_integer",
        "big_decimal",
        "timestamp",
        "json_number",
        "json_parse",
        "json_encode",
        "json_object",
        "UNSET",
        "basic_credentials",
        "aws_credentials",
        "credential_chain",
        "environment_credentials",
        "profile_credentials",
        "cached_credentials",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut schema = crucible_codegen_common::schema(service, field);
    let mut files = Vec::new();
    let mut model_files = BTreeSet::new();
    for (id, shape) in &service.shapes {
        let description = &schema["shapes"][id.to_string()];
        if let Some(members) = description["members"].as_object() {
            for (name, member) in members {
                if contains_nul(member) {
                    diagnostics.push(
                        &id.with_member(name),
                        "R strings cannot represent NUL in member defaults",
                    );
                }
            }
        } else if contains_nul(description) {
            diagnostics.push(
                id,
                "R strings cannot represent NUL in enum values or defaults",
            );
        }
        let name = service.name_of(id);
        if !matches!(
            shape,
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
        ) {
            continue;
        }
        if !exported.insert(name.to_owned())
            || !model_files.insert(name.to_ascii_lowercase())
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || [
                "list",
                "c",
                "structure",
                "names",
                "length",
                "stop",
                "function",
                "if",
                "else",
                "return",
                "switch",
                "for",
                "while",
                "repeat",
                "next",
                "break",
                "TRUE",
                "FALSE",
                "NULL",
                "NA",
                "Inf",
                "NaN",
                "get",
                "attr",
                "inherits",
                "paste",
                "paste0",
                "sprintf",
                "substr",
                "sub",
                "gsub",
                "grepl",
                "strsplit",
                "tolower",
                "toupper",
                "trimws",
                "unclass",
                "floor",
                "trunc",
                "abs",
                "min",
                "max",
                "any",
                "anyDuplicated",
                "vapply",
                "lapply",
                "Filter",
                "match",
                "integer",
                "character",
                "numeric",
                "logical",
                "raw",
                "rawToChar",
                "charToRaw",
                "validUTF8",
                "enc2utf8",
                "bitwOr",
                "bitwAnd",
                "unname",
                "invisible",
                "readBin",
                "writeBin",
                "regexec",
                "regexpr",
                "regmatches",
                "nchar",
                "rev",
                "strrep",
                "C_crucible_now",
                "startsWith",
                "endsWith",
                "isTRUE",
                "seq_len",
                "seq_along",
                "suppressWarnings",
                "tryCatch",
                "identical",
                "emptyenv",
                "ceiling",
                "strtoi",
                "utf8ToInt",
                "intToUtf8",
                "head",
                "intersect",
                "unlist",
                "setdiff",
                "setequal",
                "nzchar",
                "order",
                "sort",
                "gregexpr",
                "lengths",
                "readLines",
                "format",
                "print",
                "cat",
                "difftime",
                "missing",
                "all",
                "isFALSE",
                "C_crucible_crc",
            ]
            .contains(&name)
        {
            diagnostics.push(id, "R model export name collision or invalid name");
        }
        let mut out = String::from("# Generated from the lowered Smithy model.\n");
        match shape {
            Shape::Structure(s) => {
                let mut members = BTreeSet::new();
                let mut args = Vec::new();
                let mut values = Vec::new();
                for m in &s.members {
                    let f = field(&m.name);
                    if !members.insert(f.clone()) { diagnostics.push(&id.with_member(&m.name), "R member name collision"); }
                    args.push(format!("{}{}",function(&f),if m.optional_on_client(){" = UNSET"}else{""}));
                    values.push(format!("{} = {}",literal(&f),function(&f)));
                    schema["shapes"][id.to_string()]["members"][&m.name]["optional"] = json!(m.optional_on_client());
                }
                out.push_str(&format!("{} <- function({}) .crucible_model({}, list({}))\n", function(name),args.join(", "),literal(&id.to_string()),values.join(", ")));
            }
            Shape::Enum(_) => out.push_str(&format!("{} <- function(value) structure(.crucible_string(value), class = c({}, 'crucible_enum', 'character'))\n",function(name),literal(name))),
            Shape::IntEnum(_) => out.push_str(&format!("{} <- function(value) .crucible_int_enum(value, {})\n", function(name), literal(name))),
            Shape::Union(_) => out.push_str(&format!("{} <- function(member, value = NULL) .crucible_union({}, member, value)\n",function(name),literal(&id.to_string()))),
            _ => unreachable!(),
        }
        files.push(File {
            path: format!("R/model-{name}.R"),
            contents: out,
        });
    }
    let mut methods = BTreeSet::from([
        "close".to_owned(),
        "call".to_owned(),
        "pages".to_owned(),
        "wait".to_owned(),
    ]);
    let mut client = format!(
        "# Generated native operation methods.\n{} <- function(config = client_config()) {{\n  client <- .crucible_client(config, {}, {})\n",
        function(&client_name),
        options
            .endpoint
            .as_ref()
            .map_or("NULL".into(), |e| literal(e)),
        if options.content_sha256 {
            "TRUE"
        } else {
            "FALSE"
        }
    );
    for op in &service.operations {
        if contains_nul(&schema["operations"][op.name()]) {
            diagnostics.push(
                &op.id,
                "R strings cannot represent NUL in operation bindings",
            );
        }
        if op.method == "HEAD" && (!op.request.body.is_empty() || op.request.payload.is_some()) {
            diagnostics.push(&op.id, "R HEAD operations cannot have a request body");
        }
        let name = field(op.name());
        for suffix in std::iter::once("").chain(op.paginated.is_some().then_some("_pages")) {
            let method = format!("{name}{suffix}");
            if !methods.insert(method.clone()) {
                diagnostics.push(&op.id, "R operation method collision");
            }
            client.push_str(&format!("  client${} <- function(input{}, cancellation = NULL) client${}({}, input, cancellation)\n",function(&method),if op.input.is_none(){" = NULL"}else{""},if suffix.is_empty(){"call"}else{"pages"},literal(op.name())));
        }
    }
    for op in &service.operations {
        for waiter in op.waiters.keys() {
            let method = format!("wait_until_{}", field(waiter));
            if !methods.insert(method.clone()) {
                diagnostics.push(&op.id, "R waiter method collision");
            }
            client.push_str(&format!(
                "  client${} <- function(input{}, timeout, min_delay = NULL, max_delay = NULL, cancellation = NULL) client$wait({}, {}, input, timeout, min_delay, max_delay, cancellation)\n",
                function(&method),
                if op.input.is_none() { " = NULL" } else { "" },
                literal(op.name()),
                literal(waiter)
            ));
        }
    }
    client.push_str("  client\n}\n");
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    files.push(File {
        path: "R/client.R".into(),
        contents: client,
    });
    for (name, source) in [
        ("json", include_str!("../runtime/json.R")),
        ("types", include_str!("../runtime/types.R")),
        ("codec", include_str!("../runtime/codec.R")),
        ("query", include_str!("../runtime/query.R")),
        ("endpoints", include_str!("../runtime/endpoints.R")),
        ("auth", include_str!("../runtime/auth.R")),
        ("checksums", include_str!("../runtime/checksums.R")),
        ("waiters", include_str!("../runtime/waiters.R")),
        ("runtime", include_str!("../runtime/runtime.R")),
    ] {
        files.push(File {
            path: format!("R/{name}.R"),
            contents: crucible_codegen_common::service_exception_runtime(service, source),
        });
    }
    files.push(File {
        path: "src/clock.c".into(),
        contents: include_str!("../runtime/clock.c")
            .replace("CRUCIBLE_PACKAGE", &package.replace('.', "_")),
    });
    files.push(File {
        path: "src/checksums.c".into(),
        contents: include_str!("../runtime/checksums.c").into(),
    });
    let schema_text = serde_json::to_string(&schema).expect("schema");
    let chars: Vec<_> = schema_text.chars().collect();
    let chunks = chars
        .chunks(2048)
        .map(|s| literal(&s.iter().collect::<String>()))
        .collect::<Vec<_>>()
        .join(",\n");
    files.push(File {
        path: "R/zzz-schema.R".into(),
        contents: format!(".crucible_schema <- json_parse(paste0({chunks}))\n"),
    });
    files.push(File {
        path: "NAMESPACE".into(),
        contents: format!(
            "useDynLib({}, .registration = TRUE, .fixes = \"C_\")\n",
            literal(package)
        ) + &exported
            .iter()
            .map(|s| format!("export({})\n", literal(s)))
            .collect::<String>()
            + &["format", "print"]
                .iter()
                .flat_map(|generic| {
                    ["crucible_aws_credentials", "crucible_basic_credentials"]
                        .map(|class| format!("S3method({generic}, {class})\n"))
                })
                .collect::<String>(),
    });
    files.push(File {path:"DESCRIPTION".into(),contents:format!("Package: {package}\nVersion: {}\nTitle: Generated Client for {}\nDescription: Native client generated from the Crucible Smithy semantic model.\nAuthors@R: person('SDK', 'Maintainer', role = c('aut', 'cre'), email = 'sdk@example.invalid')\nLicense: file LICENSE\nEncoding: UTF-8\nDepends: R (>= 4.1.0)\nImports: curl (>= 4.3.2), jsonlite (>= 1.7.3), openssl (>= 1.4.6), stats\n",options.package_version,service.name_of(&service.id))});
    files.push(File{path:"LICENSE".into(),contents:"Package owner must supply distribution terms before publishing. Generated model code inherits the model owner's licensing requirements.\n".into()});
    files.push(File{path:"inst/crucible.json".into(),contents:serde_json::to_string_pretty(&json!({"generator":"crucible-codegen-r","generatorVersion":env!("CARGO_PKG_VERSION"),"packageVersion":options.package_version,"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema_text.as_bytes())),"contentSha256":options.content_sha256})).expect("metadata")});
    let aliases = exported
        .iter()
        .map(|s| format!("\\alias{{{s}}}\n"))
        .collect::<String>();
    files.push(File{path:"man/client.Rd".into(),contents:format!("\\name{{{package}-client}}\n{aliases}\\title{{Generated Smithy Client}}\n\\description{{Native model constructors and an HTTP client for the generated service.}}\n\\details{{Use {client_name}(client_config(endpoint = URL)) to construct a client. Model constructors accept named members. long() accepts a decimal string to preserve signed 64-bit integers. timestamp() accepts exact seconds and nanoseconds. UNSET omits an optional member; NULL represents JSON null. A page method returns a lazy function; call it until it returns NULL. Call client$close() to release client resources.}}\n")});
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
