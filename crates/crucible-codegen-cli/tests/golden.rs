use std::fs;
use std::path::{Path, PathBuf};

use crucible_codegen_cli::{Options, emit};
use crucible_ir::ir::Service;
use serde_json::Value;

fn package_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn load(name: &str) -> Service {
    crucible_lang::load_service(&package_dir(name))
        .unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"))
}

fn weather() -> Service {
    load("weather")
}

fn options() -> Options {
    Options {
        bin_name: "weather".into(),
        package_version: "0.1.0".into(),
        endpoint: Some("http://localhost:8080".into()),
        runtime_path: PathBuf::from("../crucible"),
    }
}

/// Weather with each `(old, new)` pair replaced in its sources.
fn edited(edits: &[(&str, &str)]) -> Service {
    let mut package = crucible_lang::load_package(&package_dir("weather")).unwrap();
    for (old, new) in edits {
        let mut found = false;
        for file in &mut package.files {
            file.text = file.text.replace("\r\n", "\n");
            if file.text.contains(old) {
                file.text = file.text.replace(old, new);
                found = true;
            }
        }
        assert!(found, "weather has no `{old}`");
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the edited weather compiles:\n{rendered}"))
}

#[test]
fn weather_matches_golden_package() {
    let service = weather();
    for file in emit(&service, &options()).unwrap() {
        // SDK output has its own authoritative golden in codegen-rust.
        if file.path == "src/client.rs" {
            assert_eq!(
                file.contents,
                crucible_codegen_rust::generate_client(&service).unwrap()
            );
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/weather")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        let golden = fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        assert_eq!(
            golden, file.contents,
            "{}; review output before CRUCIBLE_BLESS=1",
            file.path
        );
    }
}

#[test]
fn collects_command_flag_and_sdk_diagnostics() {
    let service = edited(&[(
        "        $cityId\n        @query\n        at?: DateTime\n",
        "        $cityId\n        @query\n        at?: DateTime\n        @query\n        inputJSON?: string\n        @query\n        cityID?: string\n",
    )]);
    let message = emit(&service, &options()).unwrap_err().to_string();
    assert!(
        message.contains("flag `--input-json` is reserved"),
        "{message}"
    );
    assert!(message.contains("flag `--city-id` collides"), "{message}");
}

#[test]
fn command_collisions_are_reported_against_operation() {
    let mut service = weather();
    let mut duplicate = service.operations[0].clone();
    duplicate.id = "example.weather#Create_City".parse().unwrap();
    service.operations.push(duplicate);
    let errors = emit(&service, &options()).unwrap_err().to_string();
    assert!(
        errors.contains("CLI command `create-city` collides"),
        "{errors}"
    );
}

#[test]
fn rejects_invalid_package_metadata_without_emitting_files() {
    let service = weather();
    let options = Options {
        bin_name: "../weather\n[dependencies]".into(),
        package_version: "1.0\n".into(),
        runtime_path: PathBuf::new(),
        ..options()
    };
    let errors = emit(&service, &options).unwrap_err();
    assert_eq!(errors.len(), 3);
}

#[test]
fn descriptor_uses_client_presence_and_timestamp_semantics() {
    let service = weather();
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_array()
        .unwrap();
    for member in members {
        // An input's members are required in clients as the model declares them.
        assert_eq!(
            member["required"],
            member["name"] == "name",
            "{}",
            member["name"]
        );
    }
    let coordinate_members = schema["shapes"]["example.weather#Coordinates"]["members"]
        .as_array()
        .unwrap();
    assert!(
        coordinate_members
            .iter()
            .all(|member| member["required"] == true)
    );
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(code.contains("client.list_cities_pages(input)"));
    assert!(code.contains("Output::Jsonl"));
    assert!(!code.contains("from_value(invocation.input).map_err(Error::input)"));
}

#[test]
fn emission_does_not_depend_on_operation_order() {
    let mut service = weather();
    let expected = emit(&service, &options()).unwrap();
    service.operations.reverse();
    let actual = emit(&service, &options()).unwrap();
    for (expected, actual) in expected.iter().zip(actual.iter()) {
        // The SDK assumes lowering's sorted input; the CLI owns only its files.
        if expected.path != "src/client.rs" {
            assert_eq!(expected, actual);
        }
    }
}

#[test]
fn timestamp_arguments_use_the_sdk_member_format() {
    let service = edited(&[(
        "        location?: Location\n",
        "        location?: Location\n        observedAt?: timestamp { format epoch-seconds }\n        sentAt?: timestamp\n",
    )]);
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_array()
        .unwrap();
    assert_eq!(
        members
            .iter()
            .find(|member| member["name"] == "observedAt")
            .unwrap()["kind"],
        "number"
    );
    assert_eq!(
        members
            .iter()
            .find(|member| member["name"] == "sentAt")
            .unwrap()["kind"],
        "string"
    );
}

#[test]
fn rejects_windows_device_names_for_portable_packages() {
    let service = weather();
    for name in ["con", "prn", "aux", "nul", "com1", "com9", "lpt1", "lpt9"] {
        let options = Options {
            bin_name: name.into(),
            ..options()
        };
        let errors = emit(&service, &options).unwrap_err().to_string();
        assert!(
            errors.contains("Windows device names are reserved"),
            "{name}: {errors}"
        );
    }
    for name in ["console", "com10", "lpt0"] {
        let options = Options {
            bin_name: name.into(),
            ..options()
        };
        assert!(emit(&service, &options).is_ok(), "{name}");
    }
}

#[test]
fn empty_services_do_not_emit_unused_dispatch_bindings() {
    let mut service = weather();
    service.operations.clear();
    let files = emit(&service, &options()).unwrap();
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(!code.contains("write_json"));
    assert!(!code.contains("let client ="));
    assert!(!code.contains("match invocation"));
    assert!(code.contains("service has no operations"));
}

#[test]
fn descriptor_preserves_root_shape_sensitivity() {
    let mut service = weather();
    let id = "example.weather#CreateCityInput".parse().unwrap();
    let Some(crucible_ir::ir::Shape::Structure(input)) = service.shapes.get_mut(&id) else {
        panic!("CreateCityInput is a structure");
    };
    input.sensitive = true;
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        schema["shapes"]["example.weather#CreateCityInput"]["sensitive"],
        true
    );
    assert_eq!(
        schema["shapes"]["example.weather#GetCityInput"]["sensitive"],
        false
    );
}

#[test]
fn input_version_is_a_member_flag_separate_from_root_version() {
    let service = edited(&[(
        "        $cityId\n        @query\n        at?: DateTime\n",
        "        $cityId\n        @query\n        at?: DateTime\n        @query\n        version?: string\n",
    )]);
    let files = emit(&service, &options()).unwrap();
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path == "schema.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    let members = schema["shapes"]["example.weather#GetCityInput"]["members"]
        .as_array()
        .unwrap();
    let version = members
        .iter()
        .find(|member| member["name"] == "version")
        .unwrap();
    assert_eq!(version["flag"], "version");
    assert_eq!(schema["version"], "0.1.0");
}

#[test]
fn unit_responses_do_not_bind_unit_values() {
    let service = weather();
    let files = emit(&service, &options()).unwrap();
    let code = &files
        .iter()
        .find(|file| file.path == "src/lib.rs")
        .unwrap()
        .contents;
    assert!(code.contains("client.r#ping().await.map_err(Error::request)?;"));
    assert!(!code.contains("let output = client.r#ping()"));
    assert!(code.contains("write_json(&())"));
}

fn member_of<'a>(schema: &'a Value, shape: &str, name: &str) -> &'a Value {
    schema["shapes"][shape]["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == name)
        .unwrap()
}

fn schema_of(service: &Service) -> Value {
    let files = emit(service, &options()).unwrap();
    let file = files.iter().find(|file| file.path == "schema.json");
    serde_json::from_str(&file.unwrap().contents).unwrap()
}

#[test]
fn query_timestamps_keep_the_document_kind_and_bound_format() {
    let service = edited(&[(
        "        location?: Location\n",
        "        location?: Location\n        @query\n        since?: timestamp\n",
    )]);
    let schema = schema_of(&service);
    let member = member_of(&schema, "example.weather#CreateCityInput", "since");
    assert_eq!(member["kind"], "string", "SDK parses the document codec");
    assert_eq!(member["timestamp_format"], "date-time");
    assert!(member["bound_format"].is_null(), "no distinct bound format");
}

#[test]
fn mcp_command_is_reserved_with_a_shape_diagnostic() {
    let service = load("mcp-cli-reserved");
    let diagnostics = emit(&service, &options()).unwrap_err();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.shape.to_string() == "example.mcp#Mcp"
                && d.message == "CLI command `mcp` is reserved")
    );
}

#[test]
fn tools_reuse_shared_emission_and_selected_audience() {
    let service = edited(&[(
        "operation GetCity reads City",
        "internal operation GetCity reads City",
    )]);
    for audience in [
        crucible_ir::Audience::External,
        crucible_ir::Audience::Internal,
    ] {
        let selected = crucible_ir::project(&service, audience).unwrap();
        let files = emit(&selected, &options()).unwrap();
        let tools = &files
            .iter()
            .find(|f| f.path == "src/mcp.rs")
            .unwrap()
            .contents;
        let shared = crucible_codegen_common::mcp::emit(
            &selected,
            &Default::default(),
            &mut Default::default(),
            "::crucible_cli_runtime::__generated::mcp",
        );
        assert_eq!(tools, &shared);
        assert_eq!(
            tools.contains("name: \"GetCity\""),
            audience == crucible_ir::Audience::Internal
        );
    }
}

#[test]
fn mcp_tool_name_diagnostics_reuse_the_shared_fixture() {
    let service = load("mcp-tool-names");
    let diagnostics = emit(&service, &options()).unwrap_err();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.message.starts_with("mcp.invalid_tool_name:"))
            .count(),
        2
    );
}
