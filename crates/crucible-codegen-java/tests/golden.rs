use crucible_codegen_java::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};
fn options() -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
        package_version: "0.0.0-SNAPSHOT".into(),
        endpoint: Some("https://weather.example".into()),
        content_sha256: true,
    }
}

#[test]
fn client_name_follows_service_identity() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    service.id = crucible_smithy::ShapeId::new("dev.leyline", "Leyline");
    let files = emit(&service, &options()).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/LeylineClient.java"))
        .unwrap();
    assert!(client.contents.contains("public final class LeylineClient"));
    assert!(
        client
            .contents
            .contains("public LeylineClient(ClientConfig config)")
    );
    assert!(!files.iter().any(|file| file.path.ends_with("/Client.java")));
}

#[test]
fn member_matching_first_package_segment_keeps_optional_initializer_valid() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CreateCityInput"]["members"]["ex"] =
        serde_json::json!({"target":"smithy.api#String"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut config = options();
    config.package_name = "ex.edge".into();
    let files = emit(&service, &config).unwrap();
    let source = files
        .iter()
        .find(|file| file.path.ends_with("CreateCityInput.java"))
        .unwrap();
    assert!(
        source
            .contents
            .contains("OptionalValue<java.lang.String> ex_ = ex.edge.OptionalValue.unset();")
    );
}
#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    for file in emit(&service, &options()).unwrap() {
        if file.path.ends_with(".java")
            && !file.path.contains("/model/")
            && !file.path.ends_with("ModelRegistry.java")
            && !file.path.ends_with("Client.java")
        {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}
#[test]
fn configuration_diagnostics_are_collected() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let mut options = options();
    options.package_name = "../escape".into();
    options.artifact_id = "bad<id".into();
    assert_eq!(emit(&service, &options).err().unwrap().len(), 2);
}

#[test]
fn native_defaults_follow_lowered_client_nullability() {
    let service = Model::from_json(include_str!("defaults.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let model = &files
        .iter()
        .find(|file| file.path.ends_with("/DefaultsOutput.java"))
        .unwrap()
        .contents;
    assert!(model.contains(
        "public java.lang.Long large = java.lang.Long.valueOf(\"9223372036854775807\");"
    ));
    assert!(model.contains("public java.lang.Integer count = java.lang.Integer.valueOf(\"17\");"));
    assert!(model.contains(
        "OptionalValue<java.lang.Integer> optionalCount = example.weather.OptionalValue.unset();"
    ));
    assert!(model.contains("new java.util.ArrayList<>()"));
    assert!(model.contains("new java.util.LinkedHashMap<>()"));
    assert!(model.contains("new example.weather.model.Mode(\"known\")"));
}

#[test]
fn streaming_blobs_are_byte_streams() {
    let service = Model::from_json(include_str!("streaming.json"))
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_java::capabilities())
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let model = |name: &str| {
        &files
            .iter()
            .find(|file| file.path.ends_with(&format!("/model/{name}.java")))
            .unwrap()
            .contents
    };
    assert!(model("UploadInput").contains("public example.weather.ByteStream body;"));
    assert!(model("DownloadOutput").contains("public example.weather.ByteStream body() {"));
    assert!(
        files
            .iter()
            .any(|file| file.path.ends_with("/example/weather/ByteStream.java"))
    );
}

#[test]
fn streams_outside_the_http_payload_are_diagnosed() {
    let model = serde_json::json!({"smithy":"2.0","shapes":{
        "test#Service":{"type":"service","version":"1","traits":{"aws.protocols#awsJson1_0":{}},"operations":[{"target":"test#Put"}]},
        "test#Put":{"type":"operation","input":{"target":"test#Input"}},
        "test#Stream":{"type":"blob","traits":{"smithy.api#streaming":{}}},
        "test#Input":{"type":"structure","members":{"body":{"target":"test#Stream","traits":{"smithy.api#required":{}}}}}
    }});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_java::capabilities())
        .unwrap();
    let error = emit(&service, &options()).err().unwrap();
    assert!(format!("{error:?}").contains("not the streaming payload"));
}

#[test]
fn reserved_names_are_diagnostics() {
    for name in ["class", "enum"] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({ "example.weather#CitySummary": name });
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();

        let err = emit(&service, &options()).err();
        assert!(
            err.is_some(),
            "Expected error for name {}, but got Ok",
            name
        );
        let diagnostics = err.unwrap().to_string();
        assert!(
            diagnostics.contains("invalid or colliding Java model name")
                || diagnostics.contains("Java member name collision"),
            "{diagnostics}"
        );
    }
}

fn docs_files() -> Vec<crucible_codegen_java::File> {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs-jvm.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    emit(&service, &options()).unwrap()
}

#[test]
fn smithy_documentation_cannot_break_out_of_javadoc() {
    for file in docs_files().iter().filter(|f| f.path.ends_with(".java")) {
        assert_eq!(
            file.contents.matches("/**").count(),
            file.contents.matches("*/").count(),
            "{} has a stray comment delimiter",
            file.path
        );
    }
    let client = docs_files()
        .into_iter()
        .find(|f| f.path.ends_with("/DocsClient.java"))
        .unwrap()
        .contents;
    assert!(client.contains("<b>someone</b>"));
    assert!(client.contains("<code>a *&#47; b</code>"));
    assert!(client.contains("&lt;b&gt;html&lt;/b&gt; &#64;param"));
    assert!(client.contains("@throws example.weather.model.Unavailable"));
}

#[test]
fn sensitive_members_are_redacted_in_to_string() {
    let files = docs_files();
    let source = |name: &str| {
        &files
            .iter()
            .find(|f| f.path.ends_with(name))
            .unwrap()
            .contents
    };
    let input = source("/model/GreetInput.java");
    // Direct, via a sensitive target shape, and via a collection of one.
    for member in ["password", "token", "secrets"] {
        assert!(
            input.contains(&format!(
                "\"{member}\", example.weather.ModelFormat.redact({member})"
            )),
            "{member} must be redacted"
        );
    }
    assert!(input.contains("\"names\", names"));
    // `@sensitive` on the shape covers its untagged members.
    assert!(source("/model/Classified.java").contains("ModelFormat.redact(detail)"));
    assert!(source("/model/Profile.java").contains("ModelFormat.redact(pin)"));
    assert!(source("/model/Choice.java").contains("ModelFormat.redact(code)"));
    assert!(source("/model/Unavailable.java").contains("ModelFormat.redact(retryToken)"));
}

#[test]
fn backslash_u_in_documentation_cannot_reach_javacs_unicode_pass() {
    // javac decodes backslash-u everywhere in a source file, comments included.
    let fixture = include_str!("../../crucible-smithy/tests/fixtures/client-docs-jvm.json")
        .replace(
            r#""smithy.api#documentation": "How fast.""#,
            r#""smithy.api#documentation": "C:\\users\\x \\u002a/ `\\u002a/`""#,
        );
    let service = Model::from_json(&fixture).unwrap().lower(None).unwrap();
    let files = emit(&service, &options()).unwrap();
    let mode = &files
        .iter()
        .find(|f| f.path.ends_with("/model/Mode.java"))
        .unwrap()
        .contents;
    assert!(
        mode.contains("C:&#92;users&#92;x &#92;u002a/ <code>&#92;u002a/</code>"),
        "{mode}"
    );
    for file in files.iter().filter(|f| f.path.ends_with(".java")) {
        assert!(
            !file.contents.contains(r"\u002a"),
            "{} keeps a backslash-u escape",
            file.path
        );
    }
}

/// The typed permissions fixture (`tests/fixtures/permissions`) as a client
/// sees it (docs/permissions.md §6.3), hostile documentation included: every
/// generated file that names a permission, checked in under
/// `tests/golden/permissions/`. No file names an internal permission, and a
/// checked-in file no generated one matches any more is reported as stale
/// (blessing removes it).
#[test]
fn permissions_match_goldens() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    // Every internal value, an alias of an internal permission's included.
    let internal: Vec<String> = full
        .permissions
        .values()
        .flat_map(|resource| {
            resource.permissions.iter().filter_map(move |p| {
                let primary = p
                    .alias_of
                    .as_deref()
                    .and_then(|symbol| resource.permission(symbol))
                    .unwrap_or(p);
                (primary.tier == crucible_smithy::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/permissions");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut expected = std::collections::BTreeSet::new();
    for file in emit(&service, &options()).unwrap() {
        for value in &internal {
            assert!(
                !file.contents.contains(value.as_str()),
                "{} names {value}",
                file.path
            );
        }
        if !file.contents.contains("graph:read") {
            continue;
        }
        expected.insert(file.path.clone());
        let path = root.join(&file.path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}; rerun with CRUCIBLE_BLESS=1 and read the diff",
            file.path
        );
    }
    assert!(!expected.is_empty(), "no generated file names a permission");
    fn checked_in(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                checked_in(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut files = Vec::new();
    checked_in(&root, "", &mut files);
    let stale: Vec<String> = files
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    if bless {
        for path in &stale {
            std::fs::remove_file(root.join(path)).unwrap();
        }
    } else {
        assert!(
            stale.is_empty(),
            "stale permission goldens {stale:?}; rerun with CRUCIBLE_BLESS=1"
        );
    }
}

/// A permission spelled as another's Java constant is diagnosed against the
/// service, never renamed.
#[test]
fn colliding_permission_names_are_diagnosed() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let full = compiled.service.expect("the fixture compiles");
    let mut service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    for (symbol, value) in [
        ("READ", "graph:reread"),
        ("mailSend", "graph:mail"),
        ("mailsend", "graph:mail2"),
    ] {
        let mut clash = graph.permissions[0].clone();
        clash.symbol = symbol.into();
        clash.value = value.into();
        graph.permissions.push(clash);
    }
    let diagnostics = emit(&service, &options())
        .err()
        .expect("a collision is diagnosed")
        .to_string();
    assert!(
        diagnostics.contains("shares the Java constant `READ`"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("`mailsend` of `GraphPermission` shares the Java constant `MAILSEND`"),
        "{diagnostics}"
    );
}
