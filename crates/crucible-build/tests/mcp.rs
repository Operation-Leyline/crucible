#![cfg(feature = "rust")]

use crucible_build::model;

fn mcp_argument(code: &str) -> &str {
    // service_info's fourth argument is the MCP descriptor list.
    code.split_once("::__generated::service_info(\n")
        .expect("generated service constructor")
        .1
        .lines()
        .nth(3)
        .unwrap()
        .trim()
}

#[test]
fn service_descriptors_require_explicit_build_opt_in() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-smithy/tests/fixtures/weather.json");
    let disabled = model(&fixture).server().render().unwrap();
    assert!(!disabled.contains("MCP_TOOLS"));
    assert_eq!(mcp_argument(&disabled), "None,");
    let enabled = model(&fixture).server().mcp().render().unwrap();
    assert_eq!(mcp_argument(&enabled), "Some(MCP_TOOLS),");
}
