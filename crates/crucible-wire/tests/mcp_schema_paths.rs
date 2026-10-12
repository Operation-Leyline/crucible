//! An MCP tool argument that breaks its schema is rejected with where and
//! which keyword, never the value, and never a client's map key: the path
//! keeps declared property names and list indexes and says `[key]` for a
//! key the schema does not declare.

#![cfg(feature = "mcp")]

use crucible_wire::mcp::{ToolDescriptor, ToolRegistry};
use serde_json::json;

static TOOLS: [ToolDescriptor; 1] = [ToolDescriptor {
    name: "Configure",
    operation: "Configure",
    description: "",
    input_schema: r##"{
        "type": "object",
        "properties": {
            "settings": {"type": "object", "additionalProperties": {"$ref": "#/$defs/Setting"}},
            "items": {"type": "array", "items": {"type": "object", "properties": {"limit": {"type": "integer"}}}},
            "choice": {"anyOf": [{"$ref": "#/$defs/Setting"}, {"type": "null"}]}
        },
        "additionalProperties": false,
        "$defs": {"Setting": {"type": "object", "properties": {"limit": {"type": "integer"}}}}
    }"##,
    output_schema: None,
    read_only: false,
    idempotent: false,
    destructive: false,
    errors: &[],
}];

fn rejection(arguments: serde_json::Value) -> String {
    let registry = ToolRegistry::new(&TOOLS).unwrap();
    registry.arguments("Configure", arguments).unwrap_err()
}

#[test]
fn map_keys_are_replaced_and_declared_names_kept() {
    let message = rejection(json!({"settings": {"hunter2": {"limit": "wrong"}}}));
    assert_eq!(
        message,
        "/settings/[key]/limit does not satisfy the schema's `type`"
    );
}

#[test]
fn indexes_and_references_through_alternatives_are_followed() {
    let message = rejection(json!({"items": [{"limit": 1}, {"limit": "wrong"}]}));
    assert_eq!(
        message,
        "/items/1/limit does not satisfy the schema's `type`"
    );
    let message = rejection(json!({"choice": {"limit": "wrong"}}));
    assert!(message.starts_with("/choice"), "{message}");
    assert!(!message.contains("wrong"), "{message}");
}

#[test]
fn an_undeclared_top_level_key_is_not_named() {
    let message = rejection(json!({"hunter2": 1}));
    assert!(!message.contains("hunter2"), "{message}");
}
