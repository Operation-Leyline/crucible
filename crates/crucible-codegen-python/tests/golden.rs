use crucible_codegen_python::{Options, emit};
use serde_json::Value;
use std::fs;
use std::path::Path;

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

fn options() -> Options {
    Options {
        metadata: Default::default(),
        package_name: "weather-client".into(),
        module_name: "weather_client".into(),
        package_version: "0.0.0.dev0".into(),
        endpoint: Some("https://weather.example".into()),
    }
}

#[test]
fn weather_matches_goldens() {
    let service = weather();
    for file in emit(&service, &options()).unwrap() {
        if [
            "runtime.py",
            "stream.py",
            "auth.py",
            "policy.py",
            "telemetry.py",
        ]
        .iter()
        .any(|name| file.path.ends_with(name))
        {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}

#[test]
fn response_stream_api_and_guide_match_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let service =
        crucible_lang::load_service(&root.join("../../tests/consumers/rust-stream-client/model"))
            .unwrap();
    let config = Options {
        module_name: "streams_client".into(),
        package_name: "streams-client".into(),
        ..options()
    };
    for file in emit(&service, &config).unwrap() {
        if ![
            "src/streams_client/client.py",
            "src/streams_client/__init__.py",
            "README.md",
        ]
        .contains(&file.path.as_str())
        {
            continue;
        }
        let path = root.join("tests/golden/streaming").join(&file.path);
        let contents = format!("{}\n", file.contents.trim_end());
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            contents,
            "{}",
            file.path
        );
    }
}

#[test]
fn runtime_nullability_is_taken_from_ir() {
    let service = weather();
    let files = emit(&service, &options()).unwrap();
    let models = &files
        .iter()
        .find(|file| file.path.ends_with("/models.py"))
        .unwrap()
        .contents;
    let literal = models
        .lines()
        .find_map(|line| line.strip_prefix("_SCHEMA = json.loads("))
        .unwrap()
        .strip_suffix(')')
        .unwrap();
    let text: String = serde_json::from_str(literal).unwrap();
    let schema: Value = serde_json::from_str(&text).unwrap();
    let city = &schema["shapes"]["example.weather#CitySummary"]["members"];
    assert_eq!(city["id"]["optional"], false);
    assert_eq!(city["population"]["optional"], false);
    assert_eq!(city["updatedAt"]["optional"], true);
    assert_eq!(
        schema["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["optional"],
        false
    );
    assert_eq!(
        schema["shapes"]["example.weather#CreateCityInput"]["members"]["location"]["optional"],
        true
    );
}

#[test]
fn naming_collisions_are_collected_before_emission() {
    let service = weather_with(|text| {
        text.replace(
            "        clientToken?: string\n",
            "        clientToken?: string\n        fooBar?: string\n        foo_bar?: string\n",
        )
    });
    let mut config = options();
    config.module_name = "../escape".into();
    let errors = emit(&service, &config).err().expect("invalid names fail");
    assert_eq!(errors.len(), 2, "{errors}");
}

#[test]
fn reversed_shape_order_does_not_change_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather");
    let a = crucible_lang::load_service(&dir).unwrap();
    let mut package = crucible_lang::load_package(&dir).unwrap();
    package.files.reverse();
    let b = crucible_lang::compile(&package, None).service.unwrap();
    let files = |service| {
        emit(service, &options())
            .unwrap()
            .into_iter()
            .map(|f| (f.path, f.contents))
            .collect::<Vec<_>>()
    };
    assert_eq!(files(&a), files(&b));
}

#[test]
fn unit_only_service_has_valid_empty_exports() {
    let service = load("void-service");
    let files = emit(&service, &options()).unwrap();
    let init = files
        .iter()
        .find(|f| f.path.ends_with("__init__.py"))
        .unwrap();
    assert!(!init.contents.contains("from .models import"));
    assert!(
        init.contents
            .contains("from .client import ApiClient, AsyncApiClient")
    );
}

#[test]
fn service_client_export_collisions_are_diagnosed() {
    for name in ["WeatherClient", "AsyncWeatherClient"] {
        let service = weather_with(|text| text.replace("CitySummary", name));
        assert!(emit(&service, &options()).is_err(), "{name}");
    }
}

#[test]
fn documentation_and_formatting_use_the_lowered_contract() {
    let service = load("client-docs");
    let files = emit(&service, &options()).unwrap();
    let models = &files
        .iter()
        .find(|f| f.path.ends_with("models.py"))
        .unwrap()
        .contents;
    let client = &files
        .iter()
        .find(|f| f.path.ends_with("client.py"))
        .unwrap()
        .contents;
    assert!(models.contains("https://example.com"));
    assert!(models.contains("line two"));
    assert!(models.contains("Label marker."));
    assert!(client.contains("Modeled errors: Problem"));
    assert!(client.contains("Construct") || client.contains("Construct with"));
    assert!(models.contains("def __repr__"));
}

#[test]
fn formatting_helper_names_are_reserved() {
    let service = load("client-docs-python-collisions");
    let diagnostics = emit(&service, &options()).err().unwrap();
    assert!(diagnostics.to_string().contains("model_repr"));
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

/// No `permissions.py` without permissions; with them, the module and the
/// registry's record types in the bundled runtime.
#[test]
fn permissions_module_ships_only_with_permissions() {
    let weather = weather();
    let files = emit(&weather, &options()).unwrap();
    assert!(!files.iter().any(|f| f.path.ends_with("permissions.py")));
    let files = emit(&permissions_service(), &options()).unwrap();
    assert!(
        files
            .iter()
            .any(|f| f.path == "src/weather_client/permissions.py")
    );
    let runtime = &files
        .iter()
        .find(|f| f.path.ends_with("/runtime.py"))
        .unwrap()
        .contents;
    assert!(runtime.contains("class PermissionDescription:"));
    assert!(runtime.contains("class ResourcePermissions:"));
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
    let expected = r#"MAIL_SEND = "graph:mail-send""#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}

#[test]
fn error_documentation_is_opt_in_and_reserves_only_the_new_error_attribute() {
    let service = load("error-docs");
    let files = emit(&service, &options()).expect("error documentation is supported");
    let runtime = &files
        .iter()
        .find(|file| file.path.ends_with("/runtime.py"))
        .unwrap()
        .contents;
    assert!(runtime.contains("doc_url: str | None = None"));
    assert!(runtime.contains("converted.get(attribute, UNSET)"));
    assert!(!runtime.contains("BEGIN ERROR DOCUMENTATION"));
    let files = emit(&weather(), &options()).unwrap();
    let runtime = &files
        .iter()
        .find(|file| file.path.ends_with("/runtime.py"))
        .unwrap()
        .contents;
    assert!(!runtime.contains("doc_url"));
    assert!(!runtime.contains("BEGIN ERROR DOCUMENTATION"));
    let service = compile_with("error-docs", |text| {
        text.replace("detail?: Reason", "docUrl?: Reason")
    })
    .unwrap();
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .to_string()
            .contains("doc_url attribute")
    );
    let mut service = service;
    service.error_docs_base = None;
    emit(&service, &options()).expect("existing modeled doc_url is unchanged without errorDocs");
}
