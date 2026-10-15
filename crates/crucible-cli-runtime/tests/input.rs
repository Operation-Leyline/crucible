use std::ffi::OsString;

use crucible_cli_runtime::{Invocation, Output, parse};
use serde_json::{Value, json};

fn descriptor() -> Value {
    json!({
        "name":"weather", "version":"1", "env_prefix":"CRUCIBLE_RUNTIME_TEST_UNUSED", "endpoint":"https://example.com",
        "operations":[{"name":"update", "operation":"Update", "input":"Input", "paginated":true}, {"name":"ping", "operation":"Ping", "input":null}],
        "shapes":{
            "Input":{"kind":"structure", "members":[
                {"name":"id", "flag":"id", "target":"String", "required":true},
                {"name":"enabled", "flag":"enabled", "target":"Bool"},
                {"name":"count", "flag":"count", "target":"Int"},
                {"name":"options", "flag":"options", "target":"Nested"},
                {"name":"items", "flag":"items", "target":"List"},
                {"name":"data", "flag":"data", "target":"Blob"},
                {"name":"choice", "flag":"choice", "target":"Union"},
                {"name":"secret", "flag":"secret", "target":"Int", "sensitive":true},
                {"name":"secrets", "flag":"secrets", "target":"Map", "sensitive":true},
                {"name":"ordinaryMap", "flag":"ordinary-map", "target":"Map"}
            ]},
            "Nested":{"kind":"structure", "members":[{"name":"value", "flag":"value", "target":"String"}]},
            "Union":{"kind":"union", "members":[{"name":"text", "flag":"text", "target":"String"}, {"name":"nested", "flag":"nested", "target":"Nested"}]},
            "List":{"kind":"list", "target":"Nested"},
            "Map":{"kind":"map", "target":"Nested"},
            "String":{"kind":"string"}, "Bool":{"kind":"boolean"}, "Int":{"kind":"integer"}, "Blob":{"kind":"blob"}
        }
    })
}

fn run(args: &[&str]) -> Result<Option<Invocation>, crucible_cli_runtime::Error> {
    parse(&descriptor().to_string(), args.iter().map(OsString::from))
}

#[test]
fn explicit_flags_overlay_json_without_collapsing_presence() {
    let invocation = run(&[
        "weather",
        "update",
        "--input-json",
        r#"{"id":"old","enabled":null,"options":null}"#,
        "--id",
        "",
        "--count",
        "0",
        "--enabled",
        "false",
    ])
    .unwrap()
    .unwrap();
    assert_eq!(
        invocation.input,
        json!({"id":"", "count":0, "enabled":false, "options":null})
    );
    assert!(invocation.paginate);
    assert_eq!(invocation.output, Output::Json);
    let invocation = run(&[
        "weather",
        "update",
        "--id",
        "null",
        "--enabled",
        "--no-paginate",
        "--output",
        "jsonl",
    ])
    .unwrap()
    .unwrap();
    assert_eq!(invocation.input, json!({"id":"null", "enabled":true}));
    assert!(!invocation.paginate);
    assert_eq!(invocation.output, Output::Jsonl);
}

#[test]
fn validates_after_merging_and_rejects_unknown_nested_fields() {
    assert!(run(&["weather", "update", "--input-json", r#"{"id":"from-json"}"#]).is_ok());
    for (args, message) in [
        (
            vec!["weather", "update"],
            "missing required member input.id",
        ),
        (
            vec![
                "weather",
                "update",
                "--id",
                "id",
                "--options",
                r#"{"typo":1}"#,
            ],
            "unknown member input.options.typo",
        ),
        (
            vec![
                "weather",
                "update",
                "--id",
                "id",
                "--items",
                r#"[{"typo":1}]"#,
            ],
            "unknown member input.items[0].typo",
        ),
        (
            vec!["weather", "ping", "--input-json", r#"{"typo":1}"#],
            "takes no input",
        ),
        (
            vec![
                "weather",
                "update",
                "--id",
                "id",
                "--choice",
                r#"{"text":"x","nested":{}}"#,
            ],
            "exactly one union member",
        ),
    ] {
        let error = run(&args).err().unwrap();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains(message), "{error}");
    }
}

#[test]
fn sensitive_scalar_errors_do_not_echo_values() {
    let error = run(&[
        "weather",
        "update",
        "--id",
        "id",
        "--secret",
        "credential-secret",
    ])
    .err()
    .unwrap();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("--secret"));
    assert!(!error.to_string().contains("credential-secret"));
}

#[test]
fn sensitive_ancestry_and_map_keys_are_not_exposed_in_validation_errors() {
    let error = run(&[
        "weather",
        "update",
        "--id",
        "id",
        "--secrets",
        r#"{"credential-secret":{"sensitive-unknown":1}}"#,
    ])
    .err()
    .unwrap();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("input.secrets[value]"));
    assert!(!error.to_string().contains("credential-secret"));
    assert!(!error.to_string().contains("sensitive-unknown"));
    let error = run(&[
        "weather",
        "update",
        "--id",
        "id",
        "--ordinary-map",
        r#"{"credential-secret":{"typo":1}}"#,
    ])
    .err()
    .unwrap();
    assert!(error.to_string().contains("input.ordinaryMap[value].typo"));
    assert!(!error.to_string().contains("credential-secret"));
}

#[test]
fn sensitive_root_shapes_hide_unknown_member_names() {
    let mut spec = descriptor();
    spec["shapes"]["Input"]["sensitive"] = json!(true);
    let error = parse(
        &spec.to_string(),
        [
            "weather",
            "update",
            "--input-json",
            r#"{"id":"id","credential-secret":1}"#,
        ]
        .into_iter()
        .map(OsString::from),
    )
    .err()
    .unwrap();
    assert_eq!(error.exit_code(), 2);
    assert!(error.to_string().contains("unknown member in input"));
    assert!(!error.to_string().contains("credential-secret"));
}

#[test]
fn reads_json_and_raw_binary_files() {
    let path = std::env::temp_dir().join(format!(
        "crucible-cli-runtime-input-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, r#"{"id":"file"}"#).unwrap();
    let binary = path.with_extension("bin");
    std::fs::write(&binary, [0, 255, 1, 128]).unwrap();
    let invocation = run(&[
        "weather",
        "update",
        "--input-json",
        &format!("file://{}", path.display()),
        "--data",
        &format!("fileb://{}", binary.display()),
    ])
    .unwrap()
    .unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(binary).unwrap();
    assert_eq!(invocation.input, json!({"id":"file", "data":"AP8BgA=="}));
}

#[test]
fn reserved_member_flags_are_diagnosed_before_clap_panics() {
    let mut spec = descriptor();
    spec["shapes"]["Input"]["members"][0]["flag"] = json!("endpoint-url");
    let error = parse(&spec.to_string(), [OsString::from("weather")])
        .err()
        .unwrap();
    assert!(error.to_string().contains("reserved flag --endpoint-url"));
}

#[test]
fn root_version_does_not_reserve_operation_version_members() {
    let mut spec = descriptor();
    spec["shapes"]["Input"]["members"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"version", "flag":"version", "target":"String"}));
    let spec = spec.to_string();
    assert!(
        parse(
            &spec,
            ["weather", "--version"].into_iter().map(OsString::from)
        )
        .unwrap()
        .is_none()
    );
    let invocation = parse(
        &spec,
        ["weather", "update", "--id", "id", "--version", "2026-10"]
            .into_iter()
            .map(OsString::from),
    )
    .unwrap()
    .unwrap();
    assert_eq!(invocation.input, json!({"id":"id", "version":"2026-10"}));
}

#[test]
fn query_timestamps_accept_the_bound_and_document_formats() {
    let mut spec = descriptor();
    spec["shapes"]["Input"]["members"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({"name":"since", "flag":"since", "target":"Time", "kind":"number",
            "timestamp_format":"epoch-seconds", "bound_format":"date-time"}),
        );
    spec["shapes"]["Time"] = json!({"kind":"timestamp"});
    let spec = spec.to_string();
    for (raw, expected) in [
        ("2023-11-14T22:13:20Z", 1_700_000_000),
        ("1700000000", 1_700_000_000),
    ] {
        let args = ["weather", "update", "--id", "a", "--since", raw];
        let invocation = parse(&spec, args.iter().map(OsString::from))
            .unwrap()
            .unwrap();
        assert_eq!(invocation.input["since"], json!(expected), "{raw}");
    }
}

#[test]
fn mcp_uses_existing_configuration_without_operation_input() {
    let invocation = run(&[
        "weather",
        "mcp",
        "--endpoint-url",
        "https://agent.example.com",
    ])
    .unwrap()
    .unwrap();
    assert_eq!(invocation.operation, "mcp");
    assert_eq!(invocation.input, Value::Null);
    assert!(!invocation.paginate);
}
