//! `crucible.sdks/1` (docs/docs.md): every declared SDK's surface, named by
//! its generator, with the settings the SDK file resolves to.
#![cfg(all(feature = "rust", feature = "ts"))]

use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen::sdk;
use crucible_codegen::sdks;
use crucible_ir::Audience;

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
        curl.starts_with("curl -X POST \"$WEATHER_ENDPOINT/cities\""),
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
