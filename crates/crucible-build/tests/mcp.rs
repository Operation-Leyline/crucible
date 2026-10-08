use crucible_build::RustCodegen;

#[test]
fn service_descriptors_require_explicit_build_opt_in() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/weather.json");
    assert!(
        !RustCodegen::from_model(&fixture)
            .render()
            .unwrap()
            .contains("MCP_TOOLS")
    );
    assert!(
        RustCodegen::from_model(&fixture)
            .mcp()
            .render()
            .unwrap()
            .contains("MCP_TOOLS")
    );
}
