use crucible_codegen_r::{Options, emit};
use std::{fs, path::Path};

/// A package under `tests/models`, lowered.
fn load(name: &str) -> crucible_smithy::ir::Service {
    compile_with(name, |text| text)
        .unwrap_or_else(|rendered| panic!("{name} compiles:\n{rendered}"))
}

/// A package under `tests/models` with every source file rewritten first;
/// the rendered diagnostics when it does not compile.
fn compile_with(
    name: &str,
    edit: impl Fn(String) -> String,
) -> Result<crucible_smithy::ir::Service, String> {
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
fn weather_with(edit: impl Fn(String) -> String) -> crucible_smithy::ir::Service {
    compile_with("weather", edit).unwrap_or_else(|rendered| panic!("weather compiles:\n{rendered}"))
}

fn weather() -> crucible_smithy::ir::Service {
    load("weather")
}
fn options() -> Options {
    Options {
        package_name: "weather.client".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    }
}
#[test]
fn weather_package_is_deterministic() {
    let service = weather();
    let files = emit(&service, &options()).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| {
            !["R/json.R", "R/types.R", "R/codec.R", "R/runtime.R"].contains(&f.path.as_str())
        })
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
        fs::read_to_string(golden).unwrap().replace("\r\n", "\n")
    );
    let again = emit(&service, &options()).unwrap();
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
}
#[test]
fn invalid_metadata_and_collisions_are_diagnosed() {
    let service = weather();
    for name in [
        "x",
        "../escape",
        "bad_name",
        "bad-name",
        "1client",
        "client.",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err());
    }
    for version in ["1", "1.2\nImports: evil", "1.a", "1..2"] {
        let mut config = options();
        config.package_version = version.into();
        assert!(emit(&service, &config).is_err());
    }
    let service = weather_with(|text| {
        text.replace("CitySummary", "WeatherClient").replace(
            "        clientToken?: string\n",
            "        clientToken?: string\n        httpURL?: string\n        http_url?: string\n",
        )
    });
    let diagnostic = emit(&service, &options()).err().unwrap().to_string();
    assert!(diagnostic.contains("model export"));
    assert!(diagnostic.contains("member name"));
}

#[test]
fn runtime_names_and_static_nul_are_diagnosed() {
    for name in [
        "startsWith",
        "substring",
        "endsWith",
        "isTRUE",
        "seq_len",
        "seq_along",
        "suppressWarnings",
        "C_crucible_now",
    ] {
        let service = weather_with(|text| text.replace("CitySummary", name));
        assert!(emit(&service, &options()).is_err(), "{name}");
    }
    let static_nul = r#"document = {"bad\u{0}key": "value"}"#;
    let service = weather_with(|text| {
        text.replace(
            "    id: CityId\n",
            &format!("    id: CityId\n    staticNul: {static_nul}\n"),
        )
    });
    let diagnostic = emit(&service, &options()).err().unwrap().to_string();
    assert!(diagnostic.contains("CitySummary$staticNul"));
    assert!(diagnostic.contains("NUL"));
    let service = weather();
    let mut config = options();
    config.endpoint = Some("https://example.invalid/\0".into());
    assert!(emit(&service, &config).is_err());
}

#[test]
fn head_request_bodies_are_diagnosed() {
    let mut service = weather();
    service
        .operations
        .iter_mut()
        .find(|operation| operation.name() == "CreateCity")
        .unwrap()
        .method = "HEAD".into();
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .to_string()
            .contains("HEAD")
    );
}

#[test]
fn documentation_and_sensitive_debug_metadata_are_emitted_from_ir() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/weather");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n").replace(
            "        clientToken?: string\n",
            "        clientToken?: string\n        debugModel?: DebugModel\n",
        );
    }
    package.files.push(crucible_lang::SourceFile {
        path: "native-docs.cru".to_owned(),
        text: include_str!("../../../tests/conformance/native-docs.cru").to_owned(),
    });
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the model compiles:\n{rendered}"));
    let files = emit(&service, &options()).unwrap();
    let text = files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("A **documented** value."));
    assert!(text.contains("A sensitive member."));
    assert!(text.contains("Known secret value."));
    assert!(text.contains("@@param"));
    assert!(text.contains("S3method(format, crucible_model)"));
    assert!(text.contains("Sensitive Data Redacted"));
}

#[test]
fn model_names_cannot_collide_with_package_documentation() {
    for name in [
        "client",
        "operations",
        "runtime",
        "Client",
        "Operations",
        "Runtime",
    ] {
        let service = weather_with(|text| {
            let text = text.replace(
                "        clientToken?: string\n",
                &format!("        clientToken?: string\n        value?: {name}\n"),
            );
            if text.contains("service Weather") {
                text + &format!("\nstructure {name} {{\n}}\n")
            } else {
                text
            }
        });
        let diagnostics = emit(&service, &options()).err().unwrap().to_string();
        assert!(
            diagnostics.contains("R model export name collision"),
            "{name}: {diagnostics}"
        );
    }
}
