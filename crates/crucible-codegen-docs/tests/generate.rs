use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crucible_codegen_docs::generate;
use crucible_smithy::{Audience, Model, project};
use serde_json::{Value, json};

const WEATHER: &str =
    include_str!("../../crucible-smithy/tests/fixtures/service-docs-weather.json");
const CATALOG: &str = include_str!("../../crucible-smithy/tests/fixtures/service-docs.json");
const PERMISSIONS: &str = include_str!("../../crucible-smithy/tests/fixtures/permissions.json");

fn files(source: &str, audience: Audience) -> Vec<crucible_codegen_docs::File> {
    let service = Model::from_json(source)
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    generate(&project(&service, audience).unwrap()).unwrap()
}

/// The renderer is identical for every service; `golden/site` holds its single
/// reviewed copy, so a runtime edit shows up once rather than in every fixture.
const SITE_RUNTIME: [&str; 3] = ["index.html", "explorer.js", "explorer.css"];

fn golden(name: &str, source: &str, audience: Audience) {
    let files: Vec<_> = files(source, audience)
        .into_iter()
        // describe.js repeats describe.json; the site test pins that.
        .filter(|f| f.path != "describe.js" && !SITE_RUNTIME.contains(&f.path.as_str()))
        .collect();
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

/// Typed permissions (docs/permissions.md §6.3): a Permissions page with a
/// table per tier, an Authorization section per operation, and the
/// registry and requirements in `describe.json`. Internal permissions and
/// pending reasons only in the server audience.
#[test]
fn permissions_match_reviewed_goldens() {
    golden("permissions-external", PERMISSIONS, Audience::External);
    golden("permissions-internal", PERMISSIONS, Audience::Internal);
    let external = files(PERMISSIONS, Audience::External);
    for file in &external {
        assert!(
            !file.contents.contains("organization:read"),
            "{}",
            file.path
        );
        assert!(
            !file.contents.contains("history module moves"),
            "{}",
            file.path
        );
    }
    let internal = files(PERMISSIONS, Audience::Internal);
    let page = internal
        .iter()
        .find(|f| f.path == "src/permissions.md")
        .unwrap();
    assert!(page.contents.contains("### Internal"), "{}", page.contents);
}

#[test]
fn site_files_match_reviewed_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/site");
    let generated = files(WEATHER, Audience::External);
    let mut found = BTreeSet::new();
    for file in generated
        .iter()
        .filter(|f| SITE_RUNTIME.contains(&f.path.as_str()))
    {
        found.insert(file.path.clone());
        let path = root.join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(&root).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
            file.contents,
            "{}",
            path.display()
        );
    }
    assert_eq!(found.len(), SITE_RUNTIME.len());
    let on_disk: BTreeSet<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(on_disk, found, "stale golden files");
}

#[test]
fn site_is_identical_across_services_and_loads_its_own_description() {
    let weather = files(WEATHER, Audience::External);
    let catalog = files(CATALOG, Audience::Internal);
    for name in SITE_RUNTIME {
        let pick = |files: &[crucible_codegen_docs::File]| {
            files
                .iter()
                .find(|f| f.path == name)
                .unwrap()
                .contents
                .clone()
        };
        assert_eq!(
            pick(&weather),
            pick(&catalog),
            "{name} must not be baked per service"
        );
    }
    for generated in [&weather, &catalog] {
        let get = |name: &str| &generated.iter().find(|f| f.path == name).unwrap().contents;
        // Parse JSON data so file:// and fetch preserve arbitrary map keys alike.
        assert_eq!(
            get("describe.js"),
            &format!(
                "globalThis.CRUCIBLE_DESCRIPTION = JSON.parse({});\n",
                serde_json::to_string(get("describe.json").trim_end()).unwrap()
            )
        );
        // A strict CSP (script-src 'self'; style-src 'self') forbids inline code.
        let html = get("index.html");
        assert!(
            !html.contains("style=") && !html.contains("<style"),
            "inline style"
        );
        assert!(
            !html.contains("javascript:") && !html.contains(" on"),
            "inline handler"
        );
        let scripts = html.matches("<script").count();
        assert_eq!(
            scripts,
            html.matches("<script src=").count(),
            "inline script"
        );
        assert_eq!(scripts, 2);
        assert!(get("explorer.js").len() + get("explorer.css").len() < 100 * 1024);
    }
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
    for source in [WEATHER, CATALOG, PERMISSIONS] {
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

#[test]
fn file_description_preserves_arbitrary_example_keys() {
    let mut source: Value = serde_json::from_str(WEATHER).unwrap();
    source["shapes"]["example.weather#ExampleDocument"] = json!({"type": "document"});
    source["shapes"]["example.weather#CreateCityInput"]["members"]["metadata"] =
        json!({"target": "example.weather#ExampleDocument"});
    source["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#examples"][0]["input"]["metadata"] =
        json!({"__proto__": {"example": true}});
    let generated = files(&source.to_string(), Audience::Internal);
    let script = &generated
        .iter()
        .find(|f| f.path == "describe.js")
        .unwrap()
        .contents;
    let json = &generated
        .iter()
        .find(|f| f.path == "describe.json")
        .unwrap()
        .contents;
    assert!(json.contains("__proto__"));
    let encoded = script
        .strip_prefix("globalThis.CRUCIBLE_DESCRIPTION = JSON.parse(")
        .unwrap()
        .strip_suffix(");\n")
        .unwrap();
    let decoded: String = serde_json::from_str(encoded).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&decoded).unwrap(),
        serde_json::from_str::<Value>(json).unwrap()
    );
}
