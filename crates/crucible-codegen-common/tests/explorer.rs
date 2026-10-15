use std::path::Path;

use crucible_codegen_common::describe;
use crucible_smithy::Model;

#[test]
fn explorer_retains_streaming_docs_without_buffered_wire_examples() {
    for fixture in ["mcp-streaming", "dynamic-bindings"] {
        let source = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../crucible-smithy/tests/fixtures/{fixture}.json")),
        )
        .unwrap();
        let service = Model::from_json(&source)
            .unwrap()
            .lower_documentation(None)
            .unwrap();
        let description = describe::describe_for_explorer(&service).unwrap();
        assert_eq!(description.operations.len(), service.operations.len());
        if fixture == "mcp-streaming" {
            assert!(
                description
                    .operations
                    .values()
                    .all(|op| op["callable"] == false)
            );
        } else {
            assert!(
                description
                    .operations
                    .values()
                    .any(|op| op["callable"] == true)
            );
        }
    }
}
