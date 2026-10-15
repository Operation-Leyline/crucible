use std::collections::BTreeSet;
use std::path::Path;

use crucible_codegen_common::describe;
use crucible_smithy::Model;
use serde_json::{Value, json};

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

/// Pin real describe output alongside independent expected Smithy vectors.
/// Node consumes the generated contract, never the AST or traits themselves.
#[test]
fn browser_request_vector_contracts() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crucible-smithy/tests/fixtures");
    let mut cases = Vec::new();
    let mut descriptions = serde_json::Map::new();
    for (fixture, operation_names) in [("dynamic-bindings", vec!["Get"])] {
        let source: Value = serde_json::from_str(
            &std::fs::read_to_string(fixtures.join(format!("{fixture}.json"))).unwrap(),
        )
        .unwrap();
        let service = Model::from_json(&source.to_string())
            .unwrap()
            .lower_documentation(None)
            .unwrap();
        let mut description = describe::describe_for_explorer(&service).unwrap();
        description
            .operations
            .retain(|_, op| operation_names.contains(&op["name"].as_str().unwrap()));
        let mut reachable = BTreeSet::new();
        let mut pending: Vec<String> = description
            .operations
            .values()
            .flat_map(|op| {
                ["input", "output"]
                    .into_iter()
                    .filter_map(|key| op[key].as_str().map(str::to_owned))
                    .chain(
                        op["errors"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|id| id.as_str().unwrap().to_owned()),
                    )
            })
            .collect();
        while let Some(id) = pending.pop() {
            if reachable.insert(id.clone())
                && let Some(shape) = description.shapes.get(&id)
            {
                pending.extend(
                    shape["members"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|m| m["target"].as_str().unwrap().to_owned()),
                );
            }
        }
        description.shapes.retain(|id, _| reachable.contains(id));
        let key = format!("{fixture}/{}", service.protocol.trait_id());
        descriptions.insert(key.clone(), serde_json::to_value(&description).unwrap());
        for operation in description.operations.values() {
            let id = operation["id"].as_str().unwrap();
            let vectors = source["shapes"][id]["traits"]["smithy.test#httpRequestTests"]
                .as_array()
                .unwrap();
            // These vectors cover scalar/null/blob encoding, repeated query,
            // greedy labels, prefix headers and the modeled request body.
            for vector in vectors
                .iter()
                .filter(|v| !v["id"].as_str().unwrap().contains("Server"))
                .take(2)
            {
                let op = service
                    .operations
                    .iter()
                    .find(|op| op.id.to_string() == id)
                    .unwrap();
                let mut params = vector.get("params").cloned().unwrap_or_else(|| json!({}));
                // smithy.test params carry blobs as plain text; lowered examples
                // already carry base64. Normalize at the test boundary.
                if let Some(input) = &op.input {
                    normalize(&service, input, &mut params);
                }
                let input: serde_json::Map<String, Value> = op
                    .input
                    .as_ref()
                    .and_then(|id| service.shape(id))
                    .into_iter()
                    .flat_map(|s| s.members())
                    .filter_map(|m| {
                        params.get(&m.name).map(|v| {
                            (
                                m.wire_name.clone(),
                                service.wire_example(
                                    m,
                                    v,
                                    op.request.timestamp_formats.get(&m.name).copied(),
                                ),
                            )
                        })
                    })
                    .collect();
                let expected = vector.clone();
                cases.push(json!({"name":format!("{} {}",service.protocol.trait_id(),vector["id"].as_str().unwrap()),"description":key,"operation":id,"input":input,"expected":expected}));
            }
        }
    }
    let content = format!(
        "{}\n",
        serde_json::to_string_pretty(&json!({"descriptions":descriptions,"cases":cases})).unwrap()
    );
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/explorer/request-vectors.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &content).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(golden)
            .unwrap()
            .replace("\r\n", "\n"),
        content
    );
}

fn normalize(
    service: &crucible_smithy::ir::Service,
    id: &crucible_smithy::ShapeId,
    value: &mut Value,
) {
    use base64::Engine;
    use crucible_smithy::ir::{Shape, SimpleKind};
    match service.shape(id).unwrap() {
        Shape::Simple(s) if s.kind == SimpleKind::Blob => {
            if let Some(text) = value.as_str() {
                *value = json!(base64::engine::general_purpose::STANDARD.encode(text));
            }
        }
        Shape::List(list) => {
            if let Some(items) = value.as_array_mut() {
                for item in items {
                    normalize(service, &list.member.target, item);
                }
            }
        }
        Shape::Map(map) => {
            if let Some(items) = value.as_object_mut() {
                for item in items.values_mut() {
                    normalize(service, &map.value.target, item);
                }
            }
        }
        Shape::Structure(s) | Shape::Union(s) => {
            for m in &s.members {
                if let Some(item) = value.get_mut(&m.name) {
                    normalize(service, &m.target, item);
                }
            }
        }
        _ => {}
    }
}
