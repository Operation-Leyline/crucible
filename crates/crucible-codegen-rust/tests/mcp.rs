use crucible_codegen_rust::{Options, generate};
use crucible_smithy::Model;

fn emit(model: &str) -> Result<String, crucible_smithy::Diagnostics> {
    let service = Model::from_json(model)
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_rust::capabilities())
        .unwrap();
    generate(
        &service,
        &Options {
            mcp: true,
            ..Options::default()
        },
    )
}
#[test]
fn static_descriptors_match_golden() {
    let code = emit(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    assert!(code.contains("mcp_tools: Some(MCP_TOOLS)"));
    let descriptors = code.split("    /// External MCP tools;").nth(1).unwrap();
    let metadata = code
        .split("    /// The service, as the framework sees it.")
        .nth(1)
        .unwrap()
        .split("    /// Registers implementations")
        .next()
        .unwrap();
    let snapshot = format!("{metadata}{descriptors}");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/mcp.rs");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::write(&path, &snapshot).unwrap();
    }
    assert_eq!(snapshot, std::fs::read_to_string(path).unwrap());
}
#[test]
fn tool_name_diagnostics_are_collected() {
    let diagnostics = emit(include_str!(
        "../../crucible-smithy/tests/fixtures/mcp-tool-names.json"
    ))
    .unwrap_err();
    assert_eq!(diagnostics.len(), 2);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.message.starts_with("mcp.invalid_tool_name:"))
    );
}

#[test]
fn strict_inputs_remain_strict_after_external_projection() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let code = generate(
        &service,
        &Options {
            mcp: true,
            strict_inputs: true,
            ..Options::default()
        },
    )
    .unwrap();
    let descriptors = code.split("pub static MCP_TOOLS").nth(1).unwrap();
    let get = descriptors
        .lines()
        .find(|l| l.contains("name: \"GetCity\""))
        .unwrap();
    let literal = get
        .split("input_schema: ")
        .nth(1)
        .unwrap()
        .split(", output_schema:")
        .next()
        .unwrap();
    let text: String = serde_json::from_str(literal).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["$defs"]["example.weather.GetCityInput"]["additionalProperties"],
        false
    );
}
#[test]
fn streaming_exclusions_are_explicit_and_sorted() {
    let code = emit(include_str!(
        "../../crucible-smithy/tests/fixtures/mcp-streaming.json"
    ))
    .unwrap();
    assert_eq!(code.matches("mcp.streaming_excluded:").count(), 5);
    let notes = code.split("pub static MCP_EXCLUSIONS").nth(1).unwrap();
    assert!(notes.find("#Chat").unwrap() < notes.find("#Publish").unwrap());
    assert!(notes.find("#Publish").unwrap() < notes.find("#Subscribe").unwrap());
}
#[test]
fn server_keeps_internal_api_but_descriptors_use_external_projection() {
    let code = emit(include_str!(
        "../../crucible-smithy/tests/fixtures/internal-cors.json"
    ))
    .unwrap();
    assert!(code.contains("pub trait SecretOperation"));
    let descriptors = code.split("pub static MCP_TOOLS").nth(1).unwrap();
    assert!(!descriptors.contains("Secret"));
    assert!(!descriptors.contains("privateValue"));
}
