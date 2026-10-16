//! `crucible.sdks/1` (docs/docs.md): every declared SDK's surface, named by
//! its generator, with the settings the SDK file resolves to.
#![cfg(all(feature = "rust", feature = "ts"))]

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen::sdk;
use crucible_codegen::sdks;
use crucible_ir::Audience;

#[cfg(feature = "all-clients")]
#[path = "common/clients.rs"]
mod clients;
mod common;

/// A service repository holding `text` as its SDK file and `model` as its
/// model package.
fn repository(model: &str, text: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join(".git")).unwrap();
    common::copy_package(&common::package_dir(model), &directory.path().join("model"));
    let path = directory.path().join(sdk::FILE_NAME);
    fs::write(&path, text).unwrap();
    (directory, path)
}

fn golden(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, text).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&path)
            .expect("golden exists; bless with CRUCIBLE_BLESS=1")
            .replace("\r\n", "\n"),
        text
    );
}

/// The SDK file's every SDK, described, at `audience`.
fn describe(path: &Path, model: &str, audience: Audience) -> Result<String, String> {
    let file = sdk::load(path).unwrap();
    let service = common::load(model);
    sdks::file(&file, &service, audience).map(|file| {
        assert_eq!(file.path, "sdks.json");
        file.contents
    })
}

const TS_AND_RUST: &str = r#"
schema_version = 1
model = "model"

[sdk.ts]
package = "@example/surface"
endpoint = { env = "SURFACE_ENDPOINT" }
middleware = ["content-sha256"]
out = "generated/ts"

[sdk.rust]
out = "generated/rust"
"#;

/// The allocators' suffixes are what the document names: `ListThings`'s
/// derived Rust methods and TypeScript paginator, `constructor_2`, and
/// `member_` builder setters and accessors.
#[test]
fn collisions_name_the_suffixed_methods() {
    let (_directory, path) = repository("client-docs-surface-collisions", TS_AND_RUST);
    let text = describe(&path, "client-docs-surface-collisions", Audience::External).unwrap();
    golden("collisions.sdks.json", &text);
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(document["format"], "crucible.sdks/1");
    let ts = &document["sdks"]["ts"];
    assert_eq!(ts["endpoint"], serde_json::Value::Null);
    assert_eq!(ts["endpointEnv"], "SURFACE_ENDPOINT");
    assert_eq!(ts["middleware"][0]["headers"][0], "x-amz-content-sha256");
    assert_eq!(
        ts["operations"]["example.surface#ListThings"]["paginators"][0],
        "paginateListThings_2"
    );
    let rust = &document["sdks"]["rust"];
    assert_eq!(rust["package"]["name"], "surface-sdk");
    assert_eq!(
        rust["operations"]["example.surface#ListThings"]["paginators"][0],
        "list_things_pages_2"
    );
}

/// Object keys are sorted at every depth; member arrays keep model order.
#[test]
fn keys_are_sorted_and_output_is_deterministic() {
    let (_directory, path) = repository("client-docs-surface-collisions", TS_AND_RUST);
    let first = describe(&path, "client-docs-surface-collisions", Audience::External).unwrap();
    assert_eq!(
        first,
        describe(&path, "client-docs-surface-collisions", Audience::External).unwrap()
    );
    fn sorted(value: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                let keys: Vec<_> = map.keys().collect();
                let mut ordered = keys.clone();
                ordered.sort();
                assert_eq!(keys, ordered);
                map.values().for_each(sorted);
            }
            serde_json::Value::Array(items) => items.iter().for_each(sorted),
            _ => {}
        }
    }
    sorted(&serde_json::from_str(&first).unwrap());
}

/// An external reference names no internal SDK; an internal one names both.
#[test]
fn a_reference_describes_the_sdks_its_audience_may_see() {
    let text = format!(
        "{TS_AND_RUST}\n[sdk.rust-internal]\nlanguage = \"rust\"\naudience = \"internal\"\npackage = \"surface-internal\"\nout = \"generated/rust-internal\"\n"
    );
    let (_directory, path) = repository("client-docs-surface-collisions", &text);
    let names = |audience| {
        let text = describe(&path, "client-docs-surface-collisions", audience).unwrap();
        let document: serde_json::Value = serde_json::from_str(&text).unwrap();
        document["sdks"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(names(Audience::External), ["rust", "ts"]);
    assert_eq!(names(Audience::Internal), ["rust", "rust-internal", "ts"]);
}

/// A file declaring another service is refused rather than describing SDKs
/// the reference is not about.
#[test]
fn a_file_for_another_service_is_refused() {
    let text = TS_AND_RUST.replace(
        "model = \"model\"",
        "model = \"model\"\nservice = \"example.other#Other\"",
    );
    let (_directory, path) = repository("client-docs-surface-collisions", &text);
    let error = describe(&path, "client-docs-surface-collisions", Audience::External).unwrap_err();
    assert!(
        error.contains("declares SDKs for example.other#Other"),
        "{error}"
    );
}

/// Every language's surface for Weather, with resolved defaults: Rust's
/// `{service}-sdk` package, each client's name, the endpoint read from the
/// environment, and TypeScript's default middleware.
#[cfg(feature = "all-clients")]
#[test]
fn weather_describes_every_language() {
    let text = r#"
schema_version = 1
model = "model"

[sdk.rust]
out = "generated/rust"

[sdk.ts]
package = "@example/weather"
endpoint = { env = "WEATHER_ENDPOINT" }
middleware = ["content-sha256"]
out = "generated/ts"

[sdk.python]
package = "example-weather"
module_name = "example_weather"
endpoint = "https://api.weather.example"
out = "generated/python"

[sdk.go]
package = "weather"
module_path = "example.com/weather"
out = "generated/go"

[sdk.java]
package = "com.example.weather"
group_id = "com.example"
artifact_id = "weather"
out = "generated/java"

[sdk.kotlin]
package = "com.example.weather"
group_id = "com.example"
artifact_id = "weather-kotlin"
out = "generated/kotlin"

[sdk.csharp]
package_id = "Example.Weather"
namespace = "Example.Weather"
out = "generated/csharp"

[sdk.swift]
package = "Weather"
module_name = "Weather"
out = "generated/swift"

[sdk.dart]
package = "weather"
out = "generated/dart"

[sdk.php]
package = "example/weather"
namespace = "Example\\Weather"
out = "generated/php"

[sdk.ruby]
package = "example_weather"
module_name = "ExampleWeather"
out = "generated/ruby"

[sdk.cpp]
package = "weather"
namespace = "example::weather"
out = "generated/cpp"

[sdk.c]
package = "weather"
prefix = "weather"
out = "generated/c"

[sdk.r]
package = "weather"
out = "generated/r"
"#;
    let (_directory, path) = repository("weather", text);
    let text = describe(&path, "weather", Audience::External).unwrap();
    golden("weather.sdks.json", &text);
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let sdks = document["sdks"].as_object().unwrap();
    assert_eq!(sdks.len(), 14);
    for (name, sdk) in sdks {
        assert_eq!(sdk["language"], *name);
        assert!(
            sdk["operations"]
                .as_object()
                .unwrap()
                .contains_key("example.weather#ListCities"),
            "{name}"
        );
        assert!(
            sdk["operations"]["example.weather#ListCities"]["paginators"].is_array(),
            "{name}"
        );
        // Every operation has a complete program; only the paginated one
        // has a paginated program, which uses its paginator.
        for (id, operation) in sdk["operations"].as_object().unwrap() {
            let snippet = operation["snippet"].as_str().unwrap();
            let method = operation["method"].as_str().unwrap();
            assert!(snippet.contains(method), "{name} {id}: {snippet}");
            let paginator = operation["paginators"][0].as_str();
            match (paginator, operation["paginatedSnippet"].as_str()) {
                (Some(paginator), Some(pages)) => {
                    assert!(pages.contains(paginator), "{name} {id}: {pages}");
                }
                (None, None) => {}
                (paginator, pages) => panic!("{name} {id}: {paginator:?} {pages:?}"),
            }
        }
        // The example's typed id and the bearer token's variable, never a
        // credential.
        let get_city = sdk["operations"]["example.weather#GetCity"]["snippet"]
            .as_str()
            .unwrap();
        assert!(get_city.contains("c-0a1b2c3d"), "{name}: {get_city}");
        assert!(get_city.contains("WEATHER_TOKEN"), "{name}: {get_city}");
    }
    // The raw request carries every header a caller without an SDK sends,
    // the digest the TypeScript SDK's default middleware adds among them.
    let http = &document["http"];
    assert_eq!(http["endpointEnv"], "WEATHER_ENDPOINT");
    assert_eq!(http["tokenEnv"], "WEATHER_TOKEN");
    let create = &http["operations"]["example.weather#CreateCity"];
    assert_eq!(create["method"], "POST");
    assert_eq!(create["example"], "Create a city");
    let header = |name: &str| {
        create["headers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|header| header["name"] == name)
            .map(|header| header["value"].as_str().unwrap().to_owned())
    };
    assert_eq!(header("Content-Type").as_deref(), Some("application/json"));
    assert_eq!(header("Crucible-Version").as_deref(), Some("2026-10-01"));
    assert_eq!(header("Authorization").as_deref(), Some("Bearer <token>"));
    assert_eq!(header("X-Expected-Revision").as_deref(), Some("7"));
    let body = create["body"].as_str().unwrap();
    let digest: String = <sha2::Sha256 as sha2::Digest>::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(header("x-amz-content-sha256"), Some(digest));
    let curl = create["snippet"].as_str().unwrap();
    assert!(
        curl.starts_with("curl -X POST \"$WEATHER_ENDPOINT\"'/cities'"),
        "{curl}"
    );
    assert!(
        curl.contains("-H \"Authorization: Bearer $WEATHER_TOKEN\""),
        "{curl}"
    );
    // A GET has no body, so no digest, and the raw payload is piped in.
    let get = &http["operations"]["example.weather#GetCity"];
    assert!(get["body"].is_null());
    assert!(
        get["headers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["name"] != "x-amz-content-sha256")
    );
    let photo = &http["operations"]["example.weather#PutCityPhoto"];
    assert_eq!(photo["bodyEncoding"], "base64");
    assert!(
        photo["snippet"]
            .as_str()
            .unwrap()
            .contains("| base64 -d | curl")
    );
    // A variable's endpoint is the deployment's: named, never read, and
    // generated in, so the client takes none.
    assert_eq!(sdks["ts"]["endpoint"], serde_json::Value::Null);
    assert_eq!(sdks["ts"]["endpointEnv"], "WEATHER_ENDPOINT");
    assert_eq!(sdks["ts"]["client"]["construct"], "new WeatherClient()");
    assert_eq!(sdks["python"]["endpoint"], "https://api.weather.example");
    assert_eq!(sdks["rust"]["package"]["name"], "weather-sdk");
}

/// A sample's raw request leaves out the idempotency token, as the SDK
/// snippets do: a fixed token in a copied command would replay one logical
/// request. Samples' `PutGraph` has no example, so its request is the
/// sample's, and its digest covers the body without the token.
#[test]
fn a_sample_request_carries_no_idempotency_token() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/samples\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n";
    let (_directory, path) = repository("samples", text);
    let text = describe(&path, "samples", Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let put = &document["http"]["operations"]["example.samples#PutGraph"];
    assert!(put["example"].is_null(), "{put}");
    let body = put["body"].as_str().unwrap();
    assert!(!body.contains("clientToken"), "{body}");
    assert!(body.contains("\"name\""), "{body}");
    let digest: String = <sha2::Sha256 as sha2::Digest>::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(put["snippet"].as_str().unwrap().contains(&digest), "{put}");
}

/// A structure or document payload is the whole body: its JSON, with the
/// digest over exactly those bytes (`payload-bindings`' `PutRecord` and
/// `PutDocument`).
#[test]
fn a_structured_payload_is_the_raw_request_body() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/payloads\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n";
    let (_directory, path) = repository("payload-bindings", text);
    let text = describe(&path, "payload-bindings", Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let operations = &document["http"]["operations"];
    let ids: Vec<&String> = operations.as_object().unwrap().keys().collect();
    for name in ["PutRecord", "PutDocument"] {
        let id = ids
            .iter()
            .find(|id| id.ends_with(&format!("#{name}")))
            .unwrap_or_else(|| panic!("{name} in {ids:?}"));
        let put = &operations[id.as_str()];
        let body = put["body"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: {put}"));
        assert_eq!(put["bodyEncoding"], "utf-8", "{name}: {put}");
        serde_json::from_str::<serde_json::Value>(body).unwrap();
        let digest: String = <sha2::Sha256 as sha2::Digest>::digest(body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let snippet = put["snippet"].as_str().unwrap();
        assert!(snippet.contains(&digest), "{name}: {snippet}");
        assert!(snippet.contains("--data-binary"), "{name}: {snippet}");
    }
}

/// A Rust SDK that declares `content-sha256` sends the digest whatever
/// `Config` it is given (a front door that signs requests refuses a write
/// without it), and its surface, snippets and AGENTS.md say so.
#[test]
fn a_rust_sdk_with_content_sha256_sends_the_digest() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.rust]\npackage = \"weather-sdk\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/rust\"\n";
    let (_directory, path) = repository("weather", text);
    let file = sdk::load(&path).unwrap();
    let sdk = &file.sdks["rust"];
    let crucible_codegen::Client::Rust(mut options) = sdk.client(&file, Some("1.0.0")).unwrap()
    else {
        unreachable!("a Rust SDK")
    };
    assert!(options.content_sha256);
    options.runtime = crucible_codegen::RuntimeDependency::Git {
        repository: "https://github.com/Operation-Leyline/crucible".into(),
        revision: "a".repeat(40),
        version_requirement: "*".into(),
    };
    let service = common::load("weather");
    let files = crucible_codegen_rust::emit_client_package(&service, &options).unwrap();
    let source = &files
        .iter()
        .find(|f| f.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(
        source.contains("Transport::new(config.content_sha256(true))"),
        "the client turns the digest on"
    );
    let agents = &files
        .iter()
        .find(|f| f.path == "AGENTS.md")
        .unwrap()
        .contents;
    assert!(agents.contains("x-amz-content-sha256"), "{agents}");
    // A bodiless GET or DELETE goes without it.
    assert!(
        agents.contains("and `x-amz-content-sha256` (content-sha256) on every POST, PUT and PATCH"),
        "{agents}"
    );
    let text = describe(&path, "weather", Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let rust = &document["sdks"]["rust"];
    assert_eq!(rust["middleware"][0]["headers"][0], "x-amz-content-sha256");
    let create = rust["operations"]["example.weather#CreateCity"]["snippet"]
        .as_str()
        .unwrap();
    assert!(create.contains("x-amz-content-sha256"), "{create}");
}

/// Samples' `Edges` example holds values at the edges of what each
/// language represents, a defaulted member it leaves out, and a fixed
/// idempotency token. Every language writes a snippet for it, writes the
/// default in, and leaves the token for the client to fill.
#[cfg(feature = "all-clients")]
#[test]
fn edge_values_reach_every_language_without_the_token() {
    let service = common::load("samples");
    for (language, client) in clients::clients() {
        let surface = client.surface(&service, Audience::External).unwrap();
        let edges = &surface.operations["example.samples#Edges"].snippet;
        assert!(
            !edges.contains("fixed-token-the-client-replaces"),
            "{language}: {edges}"
        );
        assert!(edges.contains("9223372036854775807"), "{language}: {edges}");
        assert!(
            edges.contains("123456789012345678901234567890"),
            "{language}: {edges}"
        );
        // `level` defaults to 3, which the example leaves out.
        assert!(edges.contains('3'), "{language}: {edges}");
    }
}

/// Every language's README.md and AGENTS.md show each import once per
/// code block, and where the client runs `content-sha256` they say which
/// requests carry its header: every POST, PUT and PATCH and any other
/// request with a body, not every request.
#[cfg(feature = "all-clients")]
#[test]
fn package_guides_show_each_import_once_and_where_the_digest_goes() {
    let service = common::load("weather");
    let imports = [
        "use ",
        "import ",
        "using ",
        "require ",
        "require_once ",
        "#include ",
        "library(",
        "from ",
    ];
    for (language, mut client) in clients::clients() {
        let digest = match &mut client {
            crucible_codegen::Client::Rust(options) => {
                options.content_sha256 = true;
                true
            }
            crucible_codegen::Client::Ts(options) => {
                options.middleware = vec![crucible_codegen::TsMiddleware::ContentSha256];
                true
            }
            _ => false,
        };
        let files = client.emit(&service).unwrap();
        let guides: Vec<_> = files
            .iter()
            .filter(|file| file.path.ends_with("README.md") || file.path.ends_with("AGENTS.md"))
            .collect();
        assert!(guides.len() >= 2, "{language}: no README.md and AGENTS.md");
        for guide in guides {
            let text = &guide.contents;
            for block in text.split("```").skip(1).step_by(2) {
                let mut seen = std::collections::BTreeSet::new();
                for line in block.lines().filter(|line| {
                    imports
                        .iter()
                        .any(|import| line.trim_start().starts_with(import))
                }) {
                    assert!(
                        seen.insert(line),
                        "{language} {}: {line:?} twice in\n{block}",
                        guide.path
                    );
                }
            }
            if digest {
                assert!(
                    text.contains("on every POST, PUT and PATCH")
                        && !text.contains("(content-sha256) on every request"),
                    "{language} {}:\n{text}",
                    guide.path
                );
            }
        }
    }
}

/// A paginated call's first page sends no page token, and a copied call
/// no idempotency token, wherever the request binds it (`page-tokens`).
/// Each operation's example sets the token to a value written nowhere
/// else (`page-token-left-out`) beside `filter-kept`: a language that
/// writes the example's `filter` writes its literals as the model gives
/// them, so it would write the token's too, were the token in its input.
/// The check is on the input each language constructs, not on a spelling
/// of the token's name it might happen to avoid. A page token with a
/// default is written at its default, `first-page`, which some languages'
/// constructors require.
#[cfg(feature = "all-clients")]
#[test]
fn a_snippet_constructs_no_page_or_idempotency_token_in_any_language() {
    let service = common::load("page-tokens");
    let operations = [
        "ListByBody",
        "ListByHeader",
        "ListByPayload",
        "ListByQuery",
        "ListDefaulted",
        "Submit",
    ];
    for (language, client) in clients::clients() {
        let surface = client.surface(&service, Audience::External).unwrap();
        for operation in operations {
            let described = &surface.operations[&format!("example.pagetokens#{operation}")];
            let mut snippets = vec![("snippet", described.snippet.as_str())];
            if operation != "Submit" {
                let pages = described.paginated_snippet.as_deref();
                snippets.push(("paginatedSnippet", pages.unwrap()));
            }
            for (field, snippet) in snippets {
                let at = format!("{language} {operation} {field}");
                assert!(snippet.contains("filter-kept"), "{at}: {snippet}");
                assert!(!snippet.contains("page-token-left-out"), "{at}: {snippet}");
                assert!(
                    !snippet.contains("idempotency-token-left-out"),
                    "{at}: {snippet}"
                );
                assert_eq!(
                    snippet.contains("first-page"),
                    operation == "ListDefaulted",
                    "{at}: {snippet}"
                );
            }
        }
    }
    // Samples' paginated operations have no example: their input is the
    // sample, whose token (`"nextToken"`) is none the service issued, and
    // `ListRevisions`' defaulted token is its default.
    let service = common::load("samples");
    for (language, client) in clients::clients() {
        let surface = client.surface(&service, Audience::External).unwrap();
        for (operation, token) in [("ListGraphs", "nextToken"), ("SearchGraphs", "pageToken")] {
            let described = &surface.operations[&format!("example.samples#{operation}")];
            let pages = described.paginated_snippet.as_deref().unwrap();
            for snippet in [described.snippet.as_str(), pages] {
                for quoted in [format!("\"{token}\""), format!("'{token}'")] {
                    assert!(
                        !snippet.contains(&quoted),
                        "{language} {operation}: {snippet}"
                    );
                }
            }
        }
        let revisions = &surface.operations["example.samples#ListRevisions"];
        let pages = revisions.paginated_snippet.as_deref().unwrap();
        for snippet in [revisions.snippet.as_str(), pages] {
            assert!(snippet.contains("first-page"), "{language}: {snippet}");
        }
    }
}

/// The raw request leaves each token out wherever it is bound: a page
/// token in the body, a header, the whole payload, or a query parameter
/// whose name is percent-encoded on the wire (`page%5Bcursor%5D`), and an
/// idempotency token in one too (`client%5Btoken%5D`). The member beside
/// each stays. A defaulted page token is left out too: the service reads
/// its absence as the default.
#[test]
fn a_raw_request_sends_no_page_or_idempotency_token() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/pagetokens\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n";
    let (_directory, path) = repository("page-tokens", text);
    let text = describe(&path, "page-tokens", Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let http = &document["http"];
    let http_text = http.to_string();
    assert!(!http_text.contains("page-token-left-out"), "{http_text}");
    assert!(
        !http_text.contains("idempotency-token-left-out"),
        "{http_text}"
    );
    let operation = |name: &str| &http["operations"][format!("example.pagetokens#{name}")];
    let header = |name: &str, header: &str| {
        operation(name)["headers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| h["name"].as_str().unwrap().eq_ignore_ascii_case(header))
            .map(|h| h["value"].as_str().unwrap().to_owned())
    };
    let uri = |name: &str| operation(name)["uri"].as_str().unwrap().to_owned();
    let body = |name: &str| -> serde_json::Value {
        serde_json::from_str(operation(name)["body"].as_str().unwrap()).unwrap()
    };

    assert_eq!(uri("ListByQuery"), "/by-query?filter=filter-kept");
    assert_eq!(uri("ListByHeader"), "/by-header?filter=filter-kept");
    assert_eq!(header("ListByHeader", "X-Page-Token"), None);
    assert_eq!(uri("ListDefaulted"), "/defaulted?filter=filter-kept");
    assert_eq!(
        body("ListByBody"),
        serde_json::json!({"filter": "filter-kept"})
    );
    let payload = operation("ListByPayload");
    assert!(payload["body"].is_null(), "{payload}");
    assert_eq!(header("ListByPayload", "Content-Type"), None);
    assert_eq!(
        header("ListByPayload", "X-Filter").as_deref(),
        Some("filter-kept")
    );
    assert_eq!(uri("Submit"), "/submit");
    assert_eq!(body("Submit"), serde_json::json!({"filter": "filter-kept"}));

    // Samples' sampled tokens, in the query and the body.
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/samples\"\nout = \"generated/ts\"\n";
    let (_directory, path) = repository("samples", text);
    let text = describe(&path, "samples", Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let operations = &document["http"]["operations"];
    let list = &operations["example.samples#ListGraphs"];
    let uri = list["uri"].as_str().unwrap();
    assert!(uri.starts_with("/graphs?maxResults="), "{uri}");
    assert!(!uri.contains("nextToken"), "{uri}");
    let search = &operations["example.samples#SearchGraphs"];
    let body: serde_json::Value = serde_json::from_str(search["body"].as_str().unwrap()).unwrap();
    assert!(body.get("query").is_some(), "{body}");
    assert!(body.get("pageToken").is_none(), "{body}");
    assert_eq!(
        operations["example.samples#ListRevisions"]["uri"],
        "/revisions"
    );
}

/// A service whose operations are not all authenticated says which send
/// the token: `hardening` has a public operation beside bearer ones.
#[test]
fn public_operations_are_named_where_the_token_is_described() {
    let service = common::load("hardening");
    let headers = crucible_codegen_common::snippet::added_headers(&service, &[]);
    assert!(
        headers.contains(&"Authorization: Bearer <token> (except on public operations)".to_owned()),
        "{headers:?}"
    );
}

/// An instant before year 1 (which RFC 3339 parsers refuse) is built from
/// its seconds where the language parses calendar text otherwise.
#[cfg(feature = "all-clients")]
#[test]
fn instants_outside_the_calendar_are_built_from_seconds() {
    let service = common::load_with("samples", |package| {
        for file in &mut package.files {
            file.text = file
                .text
                .replace("first: \"0001-01-01T00:00:00Z\"", "first: -62167219200");
        }
    });
    for (language, client) in clients::clients() {
        let expected = match language {
            "ts" => "new Date(-62167219200000)",
            "rust" => "runtime::time::Timestamp::from_timestamp(-62167219200, 0)",
            "ruby" => "Runtime::Timestamp.new(-62167219200, 0)",
            "php" => r"Runtime\Timestamp(-62167219200, 0)",
            "dart" => "Timestamp(BigInt.from(-62167219200), 0)",
            "java" => "Instant.ofEpochSecond(-62167219200L, 0L)",
            "kotlin" => "Instant.fromEpochSeconds(-62167219200L, 0)",
            _ => continue,
        };
        let surface = client.surface(&service, Audience::External).unwrap();
        let edges = &surface.operations["example.samples#Edges"].snippet;
        assert!(edges.contains(expected), "{language}: {expected}\n{edges}");
    }
}

/// A sender-signed operation is called by a third party, never through an
/// SDK: it has no SDK method, snippet or raw request, and the packages'
/// README.md and AGENTS.md never name it (`sender-signed`'s
/// `RecordPaymentEvent` and `ReportLeakedKeys`).
#[test]
fn sender_operations_have_no_snippet_request_or_guide() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/hooks\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n\n[sdk.rust]\npackage = \"hooks-sdk\"\nout = \"generated/rust\"\n";
    let (_directory, path) = repository("sender-signed", text);
    let text = describe(&path, "sender-signed", Audience::External).unwrap();
    let senders = ["RecordPaymentEvent", "ReportLeakedKeys"];
    for sender in senders {
        assert!(!text.contains(sender), "{sender} in sdks.json");
    }
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        document["http"]["operations"]
            .as_object()
            .unwrap()
            .contains_key("example.hooks#GetAccount"),
        "{}",
        document["http"]
    );
    let file = sdk::load(&path).unwrap();
    let service = common::load("sender-signed");
    for sdk in file.sdks.values() {
        let mut client = sdk.client(&file, Some("1.0.0")).unwrap();
        if let crucible_codegen::Client::Rust(options) = &mut client {
            options.runtime = crucible_codegen::RuntimeDependency::Git {
                repository: "https://github.com/Operation-Leyline/crucible".into(),
                revision: "a".repeat(40),
                version_requirement: "*".into(),
            };
        }
        let files = client.emit(&service).unwrap();
        for guide in files
            .iter()
            .filter(|file| file.path.ends_with("README.md") || file.path.ends_with("AGENTS.md"))
        {
            for sender in senders {
                let method = sender[..1].to_ascii_lowercase() + &sender[1..];
                let snake = "record_payment_event";
                assert!(
                    !guide.contents.contains(sender)
                        && !guide.contents.contains(&method)
                        && !guide.contents.contains(snake),
                    "{} {}: {sender}",
                    sdk.name,
                    guide.path
                );
            }
        }
    }
}

/// An idempotency token bound as the whole payload, which an example sets,
/// is left out of the raw request with its body, so a repeated command is
/// a new write rather than a replay; the SDK snippets leave it to the
/// client (`payload-token`).
#[test]
fn a_payload_token_never_reaches_a_copied_command() {
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/tokens\"\nmiddleware = [\"content-sha256\"]\nout = \"generated/ts\"\n\n[sdk.rust]\npackage = \"tokens-sdk\"\nout = \"generated/rust\"\n";
    let (_directory, path) = repository("payload-token", text);
    let text = describe(&path, "payload-token", Audience::External).unwrap();
    assert!(
        !text.contains("fixed-token-the-command-leaves-out"),
        "{text}"
    );
    let document: serde_json::Value = serde_json::from_str(&text).unwrap();
    let redeem = &document["http"]["operations"]["example.tokens#Redeem"];
    assert_eq!(redeem["example"], "Redeem an offer");
    assert!(redeem["body"].is_null(), "{redeem}");
    // An empty POST's digest is of zero bytes.
    assert!(
        redeem["snippet"]
            .as_str()
            .unwrap()
            .contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        "{redeem}"
    );
}

/// An epoch-seconds example in scientific notation (`1e-10`, `1.7684694e9`)
/// is a timestamp in every language's snippet, never a number.
#[cfg(feature = "all-clients")]
#[test]
fn epoch_examples_in_scientific_notation_are_timestamps() {
    let service = common::load_with("samples", |package| {
        for file in &mut package.files {
            file.text = file
                .text
                .replace("early: -1.5", "early: 1.7684694e9")
                .replace("ancient: -62135596801", "ancient: 1e-10");
        }
    });
    let edges = service
        .operations
        .iter()
        .find(|op| op.name() == "Edges")
        .unwrap();
    let call = crucible_codegen_common::snippet::call(&service, edges);
    let Some(crucible_codegen_common::snippet::Node {
        kind: crucible_codegen_common::snippet::Kind::Structure(members),
        ..
    }) = &call.input
    else {
        panic!("a structure input")
    };
    for name in ["early", "ancient"] {
        let (_, node) = members.iter().find(|(m, _)| m.name == name).unwrap();
        let crucible_codegen_common::snippet::Kind::Timestamp(instant) = &node.kind else {
            panic!("{name} is not a timestamp: {:?}", node.kind)
        };
        let expected = if name == "early" { 1_768_469_400 } else { 0 };
        assert_eq!((instant.seconds, instant.nanos), (expected, 0), "{name}");
    }
    for (language, client) in clients::clients() {
        let surface = client.surface(&service, Audience::External).unwrap();
        let snippet = &surface.operations["example.samples#Edges"].snippet;
        assert!(!snippet.contains("1e-10"), "{language}: {snippet}");
    }
}

/// A `bigInteger` example past the thousand digits arithmetic writes out,
/// up to the 10,000 the server reads, reaches every language's snippet and
/// the raw request as its digits in full, whatever its notation: a
/// TypeScript `BigInt` literal or Go's parser refuses an exponent.
#[cfg(feature = "all-clients")]
#[test]
fn big_integer_examples_are_written_in_full_digits() {
    let plain = format!("1{}", "0".repeat(1000));
    let widest = format!("-1{}", "0".repeat(9999));
    let edit = |text: &str| {
        text.replace(
            "huge: 123456789012345678901234567890",
            &format!("huge: {plain}"),
        )
        .replace(
            "negativeHuge: -123456789012345678901234567890",
            "negativeHuge: -1e9999",
        )
    };
    let service = common::load_with("samples", |package| {
        for file in &mut package.files {
            file.text = edit(&file.text);
        }
    });
    for (language, client) in clients::clients() {
        let surface = client.surface(&service, Audience::External).unwrap();
        let edges = &surface.operations["example.samples#Edges"].snippet;
        for digits in [&plain, &widest] {
            assert!(
                edges.contains(digits.as_str()),
                "{language}: {} digits missing",
                digits.len()
            );
        }
        assert!(!edges.contains("e9999"), "{language}: an exponent");
        assert!(!edges.contains("e+9999"), "{language}: an exponent");
    }
    let text = "schema_version = 1\nmodel = \"model\"\n\n[sdk.ts]\npackage = \"@example/samples\"\nout = \"generated/ts\"\n";
    // The request is built from the edited model, as describe.json shows it.
    let (_directory, path) = repository("samples", text);
    let file = sdk::load(&path).unwrap();
    let document = sdks::file(&file, &service, Audience::External).unwrap();
    let document: serde_json::Value = serde_json::from_str(&document.contents).unwrap();
    let body = document["http"]["operations"]["example.samples#Edges"]["body"]
        .as_str()
        .unwrap();
    assert!(
        body.contains(&format!("\"huge\":{plain}")),
        "the plain digits"
    );
    assert!(
        body.contains(&format!("\"negativeHuge\":{widest}")),
        "the exponent's digits"
    );
}
