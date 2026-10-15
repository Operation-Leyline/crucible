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

/// The typed permissions fixture (`tests/fixtures/permissions`) as a client
/// sees it (docs/permissions.md §6.3), hostile documentation included: every
/// generated file that names a permission, checked in under
/// `tests/golden/permissions/`. No file names an internal permission, and a
/// checked-in file no generated one matches any more is reported as stale
/// (blessing removes it).
#[test]
fn permissions_match_goldens() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    // Every internal value, an alias of an internal permission's included.
    let internal: Vec<String> = full
        .permissions
        .values()
        .flat_map(|resource| {
            resource.permissions.iter().filter_map(move |p| {
                let primary = p
                    .alias_of
                    .as_deref()
                    .and_then(|symbol| resource.permission(symbol))
                    .unwrap_or(p);
                (primary.tier == crucible_smithy::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/permissions");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut expected = std::collections::BTreeSet::new();
    for file in emit(&service, &options()).unwrap() {
        for value in &internal {
            assert!(
                !file.contents.contains(value.as_str()),
                "{} names {value}",
                file.path
            );
        }
        if !file.contents.contains("graph:read") {
            continue;
        }
        expected.insert(file.path.clone());
        let path = root.join(&file.path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}; rerun with CRUCIBLE_BLESS=1 and read the diff",
            file.path
        );
    }
    assert!(!expected.is_empty(), "no generated file names a permission");
    fn checked_in(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                checked_in(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut files = Vec::new();
    checked_in(&root, "", &mut files);
    let stale: Vec<String> = files
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    if bless {
        for path in &stale {
            std::fs::remove_file(root.join(path)).unwrap();
        }
    } else {
        assert!(
            stale.is_empty(),
            "stale permission goldens {stale:?}; rerun with CRUCIBLE_BLESS=1"
        );
    }
}

/// The permissions fixture as a client sees it.
fn permissions_service() -> crucible_smithy::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap()
}

/// `READ` names the same constant as `read`: one resource's permissions
/// must stay distinct after the language's naming, or one would vanish.
#[test]
fn permission_constant_collisions_are_diagnosed() {
    let mut service = permissions_service();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut twin = graph.permissions[0].clone();
    assert_eq!(twin.symbol, "read");
    twin.symbol = "READ".into();
    twin.value = "graph:read-again".into();
    graph.permissions.push(twin);
    let errors = emit(&service, &options()).err().expect("a collision fails");
    assert!(
        errors.to_string().contains("`READ` of `Graph` shares"),
        "{errors}"
    );
}

/// No `Permissions` namespace without permissions; with them, a class per
/// resource, the registry file Composer loads and the record types.
#[test]
fn permissions_ship_only_with_permissions() {
    let manifest = |files: &[crucible_codegen_php::File]| -> serde_json::Value {
        let file = files.iter().find(|f| f.path == "composer.json").unwrap();
        serde_json::from_str(&file.contents).unwrap()
    };
    let files = emit(&lower(&weather()), &options()).unwrap();
    assert!(!files.iter().any(|f| f.path.contains("Permission")));
    assert!(manifest(&files)["autoload"].get("files").is_none());
    let files = emit(&permissions_service(), &options()).unwrap();
    assert_eq!(
        manifest(&files)["autoload"]["files"],
        serde_json::json!(["src/Permissions/registry.php"])
    );
    for path in [
        "src/Permissions/GraphPermission.php",
        "src/Permissions/HistoryPermission.php",
        "src/Permissions/OrganizationPermission.php",
        "src/Permissions/registry.php",
        "src/Runtime/PermissionDescription.php",
        "src/Runtime/ResourcePermissions.php",
    ] {
        assert!(files.iter().any(|f| f.path == path), "{path}");
    }
}

/// A hyphen in a permission's symbol (`mail-send`, which `.cru` allows)
/// starts a word of the constant's name, since no client language allows
/// one in a name (`constant_symbol`).
#[test]
fn hyphenated_permission_symbols_spell_words() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let full = compiled.service.expect("the fixture compiles");
    let mut service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut hyphenated = graph.permissions[0].clone();
    hyphenated.symbol = "mail-send".into();
    hyphenated.value = "graph:mail-send".into();
    graph.permissions.push(hyphenated);
    let files = emit(&service, &options()).unwrap();
    let expected = r#"public const MAIL_SEND = 'graph:mail-send';"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}
