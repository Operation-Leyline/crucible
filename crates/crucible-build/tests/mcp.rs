use crucible_build::RustCodegen;

#[test]
fn service_descriptors_require_explicit_build_opt_in() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/weather.json");
    let disabled = RustCodegen::from_model(&fixture).render().unwrap();
    assert!(!disabled.contains("MCP_TOOLS"));
    assert!(disabled.contains("mcp_tools: None"));
    let enabled = RustCodegen::from_model(&fixture).mcp().render().unwrap();
    assert!(enabled.contains("mcp_tools: Some(MCP_TOOLS)"));
}
