use crucible_codegen_php::{Options, emit};
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
        package_name: "example/weather-client".into(),
        namespace: "Example\\Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    };
    let files = emit(&service, &options).unwrap();
    let again = emit(&service, &options).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| !f.path.starts_with("src/Runtime/"))
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(snapshot, fs::read_to_string(golden).expect("golden"));
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
    if let Some(output) = std::env::var_os("PHP_TEST_OUTPUT") {
        for file in files {
            let path = Path::new(&output).join(&file.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, file.contents).unwrap();
        }
    }
}

fn options() -> Options {
    Options {
        package_name: "example/weather-client".into(),
        namespace: "Example\\Weather".into(),
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
fn configuration_and_reserved_names_are_diagnosed() {
    let service = lower(&weather());
    for name in [
        "../escape",
        "Example/client",
        "example/",
        "example/../client",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for namespace in ["", "Example\\class", "Example\\__PROPERTY__", "../escape"] {
        let mut config = options();
        config.namespace = namespace.into();
        assert!(emit(&service, &config).is_err(), "{namespace}");
    }
    for name in [
        "__PROPERTY__",
        "String",
        "Decoder",
        "WeatherClient",
        "Schema",
        "Runtime",
    ] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        assert!(emit(&lower(&model), &options()).is_err(), "{name}");
    }
}
#[test]
fn model_field_and_union_collisions_are_collected() {
    let mut model = weather();
    for member in ["message", "messageValue"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][member] =
            serde_json::json!({"target":"smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["unknownVariant"] =
        serde_json::json!({"target":"smithy.api#String"});
    assert_eq!(emit(&lower(&model), &options()).err().unwrap().len(), 2);
}
#[test]
fn recursive_aliases_are_diagnosed_before_doc_type_expansion() {
    let mut model = weather();
    model["shapes"]["example.weather#CitySummaries"]["member"]["target"] =
        serde_json::json!("example.weather#CitySummaries");
    assert!(
        Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .err()
            .unwrap()
            .to_string()
            .contains("recursive collection")
    );
}
#[test]
fn reserved_enum_constants_are_escaped_with_collision_checks() {
    let mut model = weather();
    model["shapes"]["example.weather#Sky"]["members"]["DEFAULT"] =
        serde_json::json!({"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"default"}});
    let files = emit(&lower(&model), &options()).unwrap();
    assert!(
        files
            .iter()
            .find(|f| f.path.ends_with("Sky.php"))
            .unwrap()
            .contents
            .contains("const DEFAULT_VALUE = 'default'")
    );
    model["shapes"]["example.weather#Sky"]["members"]["DEFAULT_VALUE"] = serde_json::json!({"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"different"}});
    assert!(emit(&lower(&model), &options()).is_err());
}
fn lower_supported(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_php::capabilities())
        .unwrap()
}
fn behaviors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/client-behaviors.json"
    ))
    .unwrap()
}
#[test]
fn waiters_generate_methods_and_collisions_are_diagnosed() {
    let files = emit(&lower_supported(&behaviors()), &options()).unwrap();
    let client = &files
        .iter()
        .find(|f| f.path == "src/ServiceClient.php")
        .unwrap()
        .contents;
    assert!(client.contains(r"public function waitUntilReady(\Example\Weather\Input $input, float $timeout, ?float $minDelay = null, ?float $maxDelay = null, ?Runtime\CancellationToken $cancellation = null): ?\Example\Weather\Output"));
    assert!(client.contains("$this->runtime->wait('Read', 'Ready', $input, $timeout"));
    let mut model = behaviors();
    model["shapes"]["example.behavior#WaitUntilReady"] = serde_json::json!({"type":"operation","traits":{"smithy.api#http":{"method":"POST","uri":"/wait"},"smithy.api#auth":[]}});
    model["shapes"]["example.behavior#Service"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"target":"example.behavior#WaitUntilReady"}));
    let diagnostics = emit(&lower_supported(&model), &options()).err().unwrap();
    assert!(
        diagnostics.to_string().contains("WaitUntilReady"),
        "{diagnostics}"
    );
}
#[test]
fn streaming_blobs_use_byte_streams_and_response_targets() {
    let model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-java/tests/streaming.json"
    ))
    .unwrap();
    let files = emit(&lower_supported(&model), &options()).unwrap();
    let file = |name: &str| &files.iter().find(|f| f.path == name).unwrap().contents;
    assert!(
        file("src/UploadInput.php")
            .contains(r"public readonly \Example\Weather\Runtime\ByteStream $body")
    );
    let client = file("src/StreamingClient.php");
    assert!(client.contains(r"public function download(?Runtime\CancellationToken $cancellation = null, mixed $responseTarget = null)"));
    assert!(client.contains(r"public function upload(\Example\Weather\UploadInput $input, ?Runtime\CancellationToken $cancellation = null): Runtime\Unit"));
}

#[test]
fn rpc_v2_and_extended_auth_generate_packages() {
    for fixture in [
        include_str!("../../crucible-smithy/tests/fixtures/rpcv2-cbor.json"),
        include_str!("../../crucible-smithy/tests/fixtures/rpcv2-json.json"),
    ] {
        let service = lower_supported(&serde_json::from_str(fixture).unwrap());
        let files = emit(&service, &options()).unwrap();
        let manifest = &files
            .iter()
            .find(|f| f.path == "composer.json")
            .unwrap()
            .contents;
        assert!(!manifest.contains("ext-openssl"));
        assert!(files.iter().any(|f| f.path == "src/Runtime/Cbor.php"));
    }
    let service = lower_supported(
        &serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/rpc-auth.json"
        ))
        .unwrap(),
    );
    let files = emit(&service, &options()).unwrap();
    let file = |path: &str| &files.iter().find(|f| f.path == path).unwrap().contents;
    // SigV4a signs with ECDSA through the openssl extension.
    assert!(file("composer.json").contains("\"ext-openssl\": \"*\""));
    assert!(
        file("src/Runtime/DigestAuthentication.php")
            .contains(r"namespace Example\Weather\Runtime;")
    );
    assert!(file("src/schema.json").contains(r#""kind":"sigV4a""#));
}

fn vault() -> (crucible_smithy::ir::Service, Options) {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_php::capabilities())
    .unwrap();
    let options = Options {
        package_name: "example/vault".into(),
        namespace: "VaultSdk".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: false,
    };
    (service, options)
}

fn vault_files() -> Vec<crucible_codegen_php::File> {
    let (service, options) = vault();
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
    let secret = file("src/Secret.php");
    let client = file("src/VaultClient.php");
    for (text, source) in [
        (" * A stored secret.", secret),
        (
            " * @param string $name The <b>name</b> of the secret.",
            secret,
        ),
        (" * What a secret unlocks.", file("src/Kind.php")),
        (" * A website login.", file("src/Kind.php")),
        (" * Rarely rotated.", file("src/Level.php")),
        (" * A way to prove identity.", file("src/Credential.php")),
        (" * A password.", file("src/CredentialPassword.php")),
        (
            " * The caller may not read this secret.",
            file("src/AccessDenied.php"),
        ),
        (" * Reads one secret.", client),
        (" * Stores secrets.", client),
        (r" * @throws \VaultSdk\AccessDenied", client),
        (" * Read-only: retried automatically", client),
        (" * Idempotent: retried automatically", client),
        ("`clientToken` is filled with a fresh token", client),
        (" * Every page of `listSecrets()`", client),
    ] {
        assert!(source.contains(text), "{text}");
    }
    // `*/` in model text cannot end the docblock; `@since` and `[Secret]` cannot start a tag or link.
    assert!(client.contains(r"`*\/`"));
    assert!(client.contains(r"\[Secret]"));
    assert!(!client.contains("`*/`"));
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
    let secret = source("src/Secret.php");
    for member in ["password", "pin", "history", "byName"] {
        assert!(
            secret.contains(&format!(
                r"'{member}' => \VaultSdk\Runtime\Describe::redact($this->{member})"
            )),
            "{member}"
        );
    }
    for member in ["name", "aliases", "labels", "kind", "revision"] {
        assert!(
            !secret.contains(&format!(
                r"'{member}' => \VaultSdk\Runtime\Describe::redact"
            )),
            "{member}"
        );
    }
    assert!(secret.contains(r"'payload' => \VaultSdk\Runtime\Describe::bytes($this->payload)"));
    assert!(
        source("src/AccessDenied.php")
            .contains(r"'reason' => \VaultSdk\Runtime\Describe::redact($this->reason)")
    );
    assert!(source("src/CredentialPassword.php").contains("Describe::redact($this->value)"));
    assert!(source("src/CredentialPin.php").contains("Describe::redact($this->value)"));
    assert!(!source("src/CredentialUsername.php").contains("Describe::redact"));
    assert!(
        source("src/CredentialUnknownVariant.php").contains("['unknownVariant' => $this->name]")
    );
}

#[test]
fn vault_package_matches_golden() {
    let snapshot = vault_files()
        .iter()
        .filter(|f| !f.path.starts_with("src/Runtime/") && f.path.ends_with(".php"))
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
