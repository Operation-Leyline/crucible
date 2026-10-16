use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crucible_codegen_docs::generate;
use crucible_ir::{Audience, project};
use serde_json::Value;

const WEATHER: &str = "service-docs-weather";
const CATALOG: &str = "service-docs";

fn package_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

/// The package's lowered service, operations tied to their resources.
fn documented(name: &str) -> crucible_ir::ir::Service {
    crucible_lang::load_service(&package_dir(name)).unwrap_or_else(|rendered| {
        panic!(
            "{name}:
{rendered}"
        )
    })
}

fn files(name: &str, audience: Audience) -> Vec<crucible_codegen_docs::File> {
    generate(&project(&documented(name), audience).unwrap()).unwrap()
}

/// The renderer is identical for every service; `golden/site` holds its single
/// reviewed copy, so a runtime edit shows up once rather than in every fixture.
const SITE_RUNTIME: [&str; 3] = ["index.html", "explorer.js", "explorer.css"];

fn golden(name: &str, package: &str, audience: Audience) {
    golden_files(name, files(package, audience));
}

fn golden_files(name: &str, files: Vec<crucible_codegen_docs::File>) {
    let files: Vec<_> = files
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

/// The typed permissions fixture, a `.cru` package; `edit` rewrites its
/// source first.
fn leyline_with(edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text));
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"))
}

fn leyline() -> crucible_ir::ir::Service {
    leyline_with(|text| text)
}

/// Typed permissions (docs/permissions.md §6.3): a Permissions page with a
/// table per tier, an Authorization section per operation, and the
/// registry and requirements in `describe.json`. Internal permissions and
/// pending reasons only in the server audience.
#[test]
fn permissions_match_reviewed_goldens() {
    let service = leyline();
    let generated = |audience| generate(&project(&service, audience).unwrap()).unwrap();
    golden_files("permissions-external", generated(Audience::External));
    golden_files("permissions-internal", generated(Audience::Internal));
    let external = generated(Audience::External);
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
    let internal = generated(Audience::Internal);
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
        describe["operations"]["example.docs#WriteItem"]["examples"][0]["input"]["body"]["item"]["amount"]
            .to_string(),
        "1234567890123456789"
    );
    assert!(
        describe["operations"]["example.docs#WriteItem"]["inputSchema"]["$defs"]["smithy.api.BigInteger"]
            ["type"]
            == "integer"
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
    let leyline = generate(&project(&leyline(), Audience::Internal).unwrap()).unwrap();
    for files in [
        files(WEATHER, Audience::Internal),
        files(CATALOG, Audience::Internal),
        leyline,
    ] {
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
fn model_file_order_does_not_change_description_or_pages() {
    let reordered = {
        let mut package = crucible_lang::load_package(&package_dir(CATALOG)).unwrap();
        package.files.reverse();
        let compiled = crucible_lang::compile(&package, None);
        let service = compiled.service.expect("the package compiles");
        generate(&project(&service, Audience::External).unwrap()).unwrap()
    };
    let original = files(CATALOG, Audience::External);
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
fn missing_targets_are_collected() {
    let mut service = documented(CATALOG);
    service
        .shapes
        .remove(&"example.docs#Content".parse().unwrap());
    let error = generate(&service).unwrap_err().to_string();
    assert!(error.matches("describe.target").count() >= 2, "{error}");
}

#[test]
fn documentation_requires_resource_metadata_and_detects_filename_collisions() {
    // An operation names its resource, so the reference needs the
    // resource's metadata, which the lowering carries.
    let mut weather = documented(WEATHER);
    assert!(generate(&weather).is_ok());
    weather.resources.clear();
    assert!(
        generate(&weather)
            .unwrap_err()
            .to_string()
            .contains("describe.resources")
    );
    let mut service = documented(CATALOG);
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
    let mut package = crucible_lang::load_package(&package_dir(WEATHER)).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("\r\n", "\n")
            .replace(
                "        location?: Location\n        @token",
                "        location?: Location\n        metadata?: document\n        @token",
            )
            .replace(
                "input { name: \"Lisbon\", location:",
                "input { name: \"Lisbon\", metadata: { \"__proto__\": { example: true } }, location:",
            );
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the package compiles:\n{rendered}"));
    let generated = generate(&project(&service, Audience::Internal).unwrap()).unwrap();
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
