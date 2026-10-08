#![cfg(feature = "rust")]

use crucible_build::model;

#[test]
fn service_descriptors_require_explicit_build_opt_in() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/weather.json");
    let disabled = model(&fixture).server().render().unwrap();
    assert!(!disabled.contains("MCP_TOOLS"));
    assert!(disabled.contains("mcp_tools: None"));
    let enabled = model(&fixture).server().mcp().render().unwrap();
    assert!(enabled.contains("mcp_tools: Some(MCP_TOOLS)"));
}
