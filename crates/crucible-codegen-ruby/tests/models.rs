use crucible_codegen_ruby::{Options, emit};
use std::{fs, path::Path};

/// A package under `tests/models`, lowered.
fn load(name: &str) -> crucible_ir::ir::Service {
    compile_with(name, |text| text)
        .unwrap_or_else(|rendered| panic!("{name} compiles:\n{rendered}"))
}

/// A package under `tests/models` with every source file rewritten first;
/// the rendered diagnostics when it does not compile.
fn compile_with(
    name: &str,
    edit: impl Fn(String) -> String,
) -> Result<crucible_ir::ir::Service, String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text).replace("\r\n", "\n"));
    }
    let compiled = crucible_lang::compile(&package, None);
    compiled.service.clone().ok_or_else(|| compiled.render())
}

/// The weather package with its sources rewritten first.
fn weather_with(edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    compile_with("weather", edit).unwrap_or_else(|rendered| panic!("weather compiles:\n{rendered}"))
}

fn weather() -> crucible_ir::ir::Service {
    load("weather")
}

#[test]
fn weather_models_generate() {
    let service = weather();
    let options = Options {
        package_name: "weather_client".into(),
        module_name: "Weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
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
    }
}
#[test]
fn invalid_package_and_root_module_are_diagnosed() {
    let service = weather();
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
    let service = weather_with(|text| {
        text.replace("CitySummary", "Registry").replace(
            "        clientToken?: string\n",
            "        clientToken?: string\n        toWire?: string\n        to_wire_value?: string\n",
        )
    });
    let diagnostics = emit(&service, &options())
        .err()
        .expect("diagnostics")
        .to_string();
    assert!(diagnostics.contains("model name"));
    assert!(diagnostics.contains("member name"));
}

#[test]
fn case_conversion_cannot_overwrite_model_files() {
    let service = weather_with(|text| {
        text.replace("CitySummary", "Http_Value")
            .replace("Location", "HttpValue")
    });
    assert!(emit(&service, &options()).is_err());
}

#[test]
fn top_level_support_names_are_diagnosed() {
    for name in ["WeatherClient", "Runtime", "Registry"] {
        let service = weather_with(|text| text.replace("CitySummary", name));
        assert!(emit(&service, &options()).is_err(), "{name}");
    }
}

fn vault_files() -> Vec<crucible_codegen_ruby::File> {
    let service = load("client-docs-vault");
    let options = Options {
        package_name: "vault_client".into(),
        module_name: "Vault".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    emit(&service, &options).unwrap()
}

#[test]
fn model_documentation_reaches_models_and_client() {
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
    // The doc link `[Secret]` is a YARD reference.
    assert!(client.contains("and {Secret} for"));
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
                (primary.tier == crucible_ir::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_ir::project(&full, crucible_ir::Audience::External).unwrap();
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
fn permissions_service() -> crucible_ir::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    crucible_ir::project(&full, crucible_ir::Audience::External).unwrap()
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

/// No permissions file without permissions; with them, the module and the
/// registry's record types, required before the client.
#[test]
fn permissions_ship_only_with_permissions() {
    let files = emit(&weather(), &options()).unwrap();
    assert!(!files.iter().any(|f| f.path.contains("permission")));
    let files = emit(&permissions_service(), &options()).unwrap();
    let entry = &files
        .iter()
        .find(|f| f.path == "lib/weather_client.rb")
        .unwrap()
        .contents;
    for name in [
        "runtime/permission_description",
        "runtime/resource_permissions",
        "permissions",
    ] {
        assert!(
            entry.contains(&format!("require_relative 'weather_client/{name}'")),
            "{name}"
        );
        assert!(
            files
                .iter()
                .any(|f| f.path == format!("lib/weather_client/{name}.rb")),
            "{name}"
        );
    }
    assert!(
        entry.find("weather_client/permissions'").unwrap()
            < entry.find("weather_client/client'").unwrap()
    );
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
    let mut service = crucible_ir::project(&full, crucible_ir::Audience::External).unwrap();
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
    let expected = r#"MAIL_SEND = new('graph:mail-send')"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}
