use crucible_codegen_ruby::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

#[test]
fn weather_models_generate() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let options = Options {
        package_name: "weather_client".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    };
    let files = emit(&service, &options).unwrap();
    let again = emit(&service, &options).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| !f.path.contains("/runtime/"))
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden)
            .expect("golden")
            .replace("\r\n", "\n")
    );
    assert_eq!(
        files
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
    if let Some(output) = std::env::var_os("RUBY_TEST_OUTPUT") {
        for file in files {
            let path = Path::new(&output).join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.contents).unwrap();
        }
    }
}

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    }
}
fn weather() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
fn lower(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn invalid_package_and_root_module_are_diagnosed() {
    let service = lower(&weather());
    for name in ["../escape", "Weather", "weather-client", "", "3client"] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for name in [
        "String", "Array", "Object", "Time", "ENV", "BEGIN", "bad", "",
    ] {
        let mut config = options();
        config.module_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
}

#[test]
fn ambiguous_ruby_identifiers_are_collected() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary":"Registry"});
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "toWire".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "to_wire_value".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    let diagnostics = emit(&lower(&model), &options())
        .err()
        .expect("diagnostics")
        .to_string();
    assert!(diagnostics.contains("model name"));
    assert!(diagnostics.contains("member name"));
}

#[test]
fn case_conversion_cannot_overwrite_model_files() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] = serde_json::json!({"example.weather#CitySummary":"Http_Value","example.weather#Location":"HttpValue"});
    assert!(emit(&lower(&model), &options()).is_err());
}

#[test]
fn top_level_support_names_are_diagnosed() {
    for name in ["WeatherClient", "Runtime", "Registry"] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        assert!(emit(&lower(&model), &options()).is_err(), "{name}");
    }
}

fn lower_supported(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_ruby::capabilities())
        .unwrap()
}

#[test]
fn waiters_generate_methods_and_collisions_are_diagnosed() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-java/tests/waiters.json"
    ))
    .unwrap();
    let files = emit(&lower_supported(&model), &options()).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/client.rb"))
        .unwrap();
    assert!(client.contents.contains("def wait_until_ready(input, timeout:, min_delay: nil, max_delay: nil, cancellation: nil)\n      @runtime.wait('Read', 'Ready', input,"));
    model["shapes"]["example.wait#Jobs"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"target":"example.wait#WaitUntilReady"}));
    model["shapes"]["example.wait#WaitUntilReady"] = serde_json::json!({"type":"operation","traits":{"smithy.api#http":{"method":"POST","uri":"/wait"}}});
    let diagnostics = emit(&lower_supported(&model), &options())
        .err()
        .expect("diagnostics")
        .to_string();
    assert!(
        diagnostics.contains("example.wait#WaitUntilReady: Ruby operation name collision")
            || diagnostics.contains("waiter method name collision"),
        "{diagnostics}"
    );
}

#[test]
fn streamed_responses_take_a_target_or_block() {
    let model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-java/tests/streaming.json"
    ))
    .unwrap();
    let files = emit(&lower_supported(&model), &options()).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/client.rb"))
        .unwrap();
    assert!(
        client
            .contents
            .contains("def download(response_target: nil, cancellation: nil, &block)")
    );
    assert!(
        client
            .contents
            .contains("def upload(input, cancellation: nil)")
    );
}

fn vault_files() -> Vec<crucible_codegen_ruby::File> {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_ruby::capabilities())
    .unwrap();
    let options = Options {
        package_name: "vault_client".into(),
        module_name: "Vault".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: false,
    };
    emit(&service, &options).unwrap()
}

#[test]
fn smithy_documentation_reaches_models_and_client() {
    let files = vault_files();
    let file = |path: &str| {
        &files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
    };
    let secret = file("lib/vault_client/models/secret.rb");
    let client = file("lib/vault_client/client.rb");
    for (text, source) in [
        ("# A stored secret.", secret),
        ("# The <b>name</b> of the secret.", secret),
        (
            "# What a secret unlocks.",
            file("lib/vault_client/models/kind.rb"),
        ),
        (
            "# A website login.",
            file("lib/vault_client/models/kind.rb"),
        ),
        (
            "# Rarely rotated.",
            file("lib/vault_client/models/level.rb"),
        ),
        (
            "# A way to prove identity.",
            file("lib/vault_client/models/credential.rb"),
        ),
        (
            "# A password.",
            file("lib/vault_client/models/credential.rb"),
        ),
        (
            "# The caller may not read this secret.",
            file("lib/vault_client/models/access_denied.rb"),
        ),
        ("# Reads one secret.", client),
        ("# Stores secrets.", client),
        ("# @raise [AccessDenied]", client),
        ("# Read-only: retried automatically", client),
        ("# Idempotent: retried automatically", client),
        ("`client_token` is filled with a fresh token", client),
        ("# Every page of {#list_secrets}", client),
    ] {
        assert!(source.contains(text), "{text}");
    }
    // A bare `[Secret]` would be a YARD reference that cannot resolve.
    assert!(client.contains(r"\[Secret]"));
    assert!(file(".yardopts").contains("--markup markdown"));
}

#[test]
fn sensitive_members_are_redacted_in_string_representations() {
    let files = vault_files();
    let source = |path: &str| {
        &files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
    };
    let secret = source("lib/vault_client/models/secret.rb");
    for member in ["password", "pin", "history", "by_name"] {
        assert!(
            secret.contains(&format!("{member}: Runtime::Describe.redact(@{member})")),
            "{member}"
        );
    }
    for member in ["name", "aliases", "labels", "kind", "revision"] {
        assert!(
            !secret.contains(&format!("Describe.redact(@{member})")),
            "{member}"
        );
    }
    assert!(secret.contains("payload: Runtime::Describe.bytes(@payload)"));
    assert!(
        source("lib/vault_client/models/access_denied.rb")
            .contains("reason: Runtime::Describe.redact(@reason)")
    );
    let credential = source("lib/vault_client/models/credential.rb");
    assert!(credential.contains("password: Runtime::Describe.redact(@value)"));
    assert!(credential.contains("pin: Runtime::Describe.redact(@value)"));
    assert!(credential.contains("username: @value"));
    assert!(credential.contains("unknown_variant: @name"));
}

#[test]
fn vault_package_matches_golden() {
    let snapshot = vault_files()
        .iter()
        .filter(|f| !f.path.contains("/runtime/") && f.path.ends_with(".rb"))
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/vault.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot.replace("\r\n", "\n"),
        fs::read_to_string(golden)
            .expect("golden")
            .replace("\r\n", "\n")
    );
}
