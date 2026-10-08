use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crucible_codegen_docs::generate;
use crucible_smithy::{Audience, Model, project};
use serde_json::{Value, json};

const WEATHER: &str =
    include_str!("../../crucible-smithy/tests/fixtures/service-docs-weather.json");
const CATALOG: &str = include_str!("../../crucible-smithy/tests/fixtures/service-docs.json");

fn files(source: &str, audience: Audience) -> Vec<crucible_codegen_docs::File> {
    let service = Model::from_json(source)
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    generate(&project(&service, audience).unwrap()).unwrap()
}

fn golden(name: &str, source: &str, audience: Audience) {
    let files = files(source, audience);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let mut expected = BTreeSet::new();
    for file in files {
        let path = root.join(&file.path);
        expected.insert(file.path.clone());
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
    fn visit(root: &Path, current: &Path, found: &mut BTreeSet<String>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut found = BTreeSet::new();
    visit(&root, &root, &mut found);
    assert_eq!(found, expected, "stale golden files");
}

#[test]
fn weather_external_and_internal_match_reviewed_goldens() {
    golden("weather-external", WEATHER, Audience::External);
    golden("weather-internal", WEATHER, Audience::Internal);
}
#[test]
fn rpc_metadata_examples_and_shapes_match_reviewed_goldens() {
    golden("catalog-external", CATALOG, Audience::External);
    golden("catalog-internal", CATALOG, Audience::Internal);
}

#[test]
fn audience_filters_values_and_badges_are_visible_at_every_level() {
    let external = files(WEATHER, Audience::External);
    let internal = files(WEATHER, Audience::Internal);
    assert!(
        !external
            .iter()
            .any(|f| f.contents.contains("INTERNAL_EXAMPLE_SECRET"))
    );
    assert!(
        internal
            .iter()
            .any(|f| f.contents.contains("INTERNAL_EXAMPLE_SECRET"))
    );
    assert!(
        internal
            .iter()
            .any(|f| f.path.ends_with("API_Status.md") && f.contents.contains("Internal API"))
    );
    assert!(
        internal.iter().any(|f| f.path.ends_with("CitySummary.md")
            && f.contents.contains("<strong>Internal</strong>"))
    );
    let files = files(CATALOG, Audience::External);
    let describe: Value = serde_json::from_str(
        &files
            .iter()
            .find(|f| f.path == "describe.json")
            .unwrap()
            .contents,
    )
    .unwrap();
    assert_eq!(
        describe["operations"]["example.docs#WriteItem"]["examples"][0]["input"]["body"]["item"]["amount"],
        "1234567890123456789"
    );
    assert!(
        describe["operations"]["example.docs#WriteItem"]["inputSchema"]["$defs"]["smithy.api.BigInteger"]
            ["type"]
            == "string"
    );
    assert!(
        describe["service"]["documentation"]["html"]
            .as_str()
            .unwrap()
            .contains("&lt;script&gt;")
    );
}

#[test]
fn all_links_point_to_generated_pages() {
    for source in [WEATHER, CATALOG] {
        let files = files(source, Audience::Internal);
        let paths: BTreeSet<_> = files.iter().map(|f| f.path.as_str()).collect();
        for file in files.iter().filter(|f| f.path.ends_with(".md")) {
            for segment in file.contents.split("](").skip(1) {
                let Some((target, _)) = segment.split_once(')') else {
                    continue;
                };
                if !target.ends_with(".md") {
                    continue;
                }
                let mut components: Vec<_> = file.path.split('/').collect();
                components.pop();
                for component in target.split('/') {
                    if component == ".." {
                        components.pop();
                    } else {
                        components.push(component);
                    }
                }
                assert!(
                    paths.contains(components.join("/").as_str()),
                    "{} -> {target}",
                    file.path
                );
            }
        }
    }
}

#[test]
fn raw_payload_syntax_uses_the_resolved_content_type_once() {
    for file in files(WEATHER, Audience::External) {
        if file.path.ends_with("API_GetCityPhoto.md") || file.path.ends_with("API_PutCityPhoto.md")
        {
            assert_eq!(file.contents.matches("Content-Type: image/jpeg").count(), 1);
        }
    }
}

#[test]
fn model_object_order_does_not_change_description_or_pages() {
    let mut value: Value = serde_json::from_str(CATALOG).unwrap();
    let mut reversed: Vec<_> = value["shapes"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    reversed.reverse();
    value["shapes"] = Value::Object(reversed.into_iter().collect());
    let original = files(CATALOG, Audience::External);
    let reordered = files(&value.to_string(), Audience::External);
    assert_eq!(
        original
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        reordered
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unsupported_protocols_and_missing_targets_are_collected() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/service-docs-unsupported.json"
    ))
    .unwrap()
    .lower_documentation(None, None)
    .unwrap();
    let mut other = Model::from_json(CATALOG)
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    service.shapes = std::mem::take(&mut other.shapes);
    service.operations = other.operations;
    service
        .shapes
        .remove(&"example.docs#Content".parse().unwrap());
    let error = generate(&service).unwrap_err().to_string();
    assert!(error.contains("describe.protocol"), "{error}");
    assert!(error.matches("describe.target").count() >= 2, "{error}");
}

#[test]
fn streaming_examples_are_refused_without_partial_output() {
    let mut service = Model::from_json(WEATHER)
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let op = service
        .operations
        .iter_mut()
        .find(|op| op.id.name() == "CreateCity")
        .unwrap();
    op.request.payload = Some(crucible_smithy::ir::Payload {
        member: "city".into(),
        kind: crucible_smithy::ir::PayloadKind::Blob,
        media_type: "application/octet-stream".into(),
        streaming: true,
        requires_length: false,
    });
    let error = generate(&service).unwrap_err().to_string();
    assert!(error.contains("describe.example_streaming"));
}

#[test]
fn aws_json_target_is_resolved_without_http_trait_reconstruction() {
    let mut value: Value = serde_json::from_str(CATALOG).unwrap();
    let traits = value["shapes"]["example.docs#Catalog"]["traits"]
        .as_object_mut()
        .unwrap();
    traits.remove("smithy.protocols#rpcv2Json");
    traits.insert("aws.protocols#awsJson1_1".into(), json!({}));
    let files = files(&value.to_string(), Audience::External);
    let op = files
        .iter()
        .find(|f| f.path.ends_with("API_WriteItem.md"))
        .unwrap();
    assert!(op.contents.contains("X-Amz-Target: Catalog.WriteItem"));
    assert!(
        op.contents
            .contains("Content-Type: application/x-amz-json-1.1")
    );
}

#[test]
fn documentation_requires_the_resource_projection_and_detects_filename_collisions() {
    let ordinary = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    assert!(
        generate(&ordinary)
            .unwrap_err()
            .to_string()
            .contains("describe.resources")
    );
    let mut service = Model::from_json(CATALOG)
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let mut duplicate = service.operations[0].clone();
    duplicate.id = "example.docs#listitems".parse().unwrap();
    duplicate.resource = None;
    service.operations.push(duplicate);
    assert!(
        generate(&service)
            .unwrap_err()
            .to_string()
            .contains("docs.filename")
    );
}
