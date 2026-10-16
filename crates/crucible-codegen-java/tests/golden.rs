use crucible_codegen_java::{Options, emit};
use std::{fs, path::Path};

/// A package under `tests/models`, lowered.
fn load(name: &str) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_service(&dir)
        .unwrap_or_else(|rendered| panic!("{name} compiles:\n{rendered}"))
}

/// A package under `tests/models` with every source file rewritten first.
fn load_with(name: &str, edit: impl Fn(String) -> String) -> crucible_ir::ir::Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text).replace("\r\n", "\n"));
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("{name} compiles:\n{rendered}"))
}

fn weather() -> crucible_ir::ir::Service {
    load("weather")
}
fn options() -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
        package_version: "0.0.0-SNAPSHOT".into(),
        endpoint: Some("https://weather.example".into()),
    }
}

#[test]
fn client_name_follows_service_identity() {
    let mut service = weather();
    service.id = crucible_ir::ShapeId::new("dev.leyline", "Leyline");
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
    let service = load_with("weather", |text| {
        text.replace(
            "        clientToken?: string\n",
            "        clientToken?: string\n        ex?: string\n",
        )
    });
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
    let service = weather();
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
    let service = weather();
    let mut options = options();
    options.package_name = "../escape".into();
    options.artifact_id = "bad<id".into();
    assert_eq!(emit(&service, &options).err().unwrap().len(), 2);
}

#[test]
fn native_defaults_follow_lowered_client_nullability() {
    let service = load("defaults");
    let files = emit(&service, &options()).unwrap();
    let model = &files
        .iter()
        .find(|file| file.path.ends_with("/ReadOutput.java"))
        .unwrap()
        .contents;
    assert!(model.contains(
        "public java.lang.Long large = java.lang.Long.valueOf(\"9223372036854775807\");"
    ));
    assert!(model.contains("public java.lang.Integer count = java.lang.Integer.valueOf(\"17\");"));
    assert!(model.contains("new java.util.ArrayList<>()"));
    assert!(model.contains("new java.util.LinkedHashMap<>()"));
    assert!(model.contains("new example.weather.model.Mode(\"known\")"));
}

#[test]
fn reserved_names_are_diagnostics() {
    for name in ["class", "enum"] {
        let service = load_with("weather", |text| text.replace("CitySummary", name));
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
    let service = load("client-docs-jvm");
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
    let service = load_with("client-docs-jvm", |text| {
        text.replace("/// How fast.", r"/// C:\users\x \u002a/ `\u002a/`")
    });
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
                (primary.tier == crucible_ir::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_ir::project(&full, crucible_ir::Audience::External).unwrap();
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
    let mut service = crucible_ir::project(&full, crucible_ir::Audience::External).unwrap();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    for (symbol, value) in [
        ("READ", "graph:reread"),
        ("mailSend", "graph:mail"),
        ("mail_send", "graph:mail2"),
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
        diagnostics
            .contains("`mail_send` of `GraphPermission` shares the Java constant `MAIL_SEND`"),
        "{diagnostics}"
    );
}

/// A hyphen in a permission's symbol (`mail-send`, which `.cru` allows)
/// starts a word of the constant's name, since no client language allows
/// one in a name (`constant_symbol`).
#[test]
fn hyphenated_permission_symbols_spell_words() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let full = compiled.service.expect("the fixture compiles");
    let mut service = crucible_ir::project(&full, crucible_ir::Audience::External).unwrap();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut hyphenated = graph.permissions[0].clone();
    hyphenated.symbol = "mail-send".into();
    hyphenated.value = "graph:mail-send".into();
    graph.permissions.push(hyphenated);
    let files = emit(&service, &options()).unwrap();
    let expected = r#"GraphPermission MAIL_SEND = new GraphPermission("graph:mail-send")"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}

#[test]
fn doc_links_are_javadoc_link_tags() {
    let service =
        crucible_ir::project(&load("doc-links"), crucible_ir::Audience::External).unwrap();
    let options = Options {
        group_id: "example".into(),
        artifact_id: "links-client".into(),
        package_name: "example.links".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    let files = emit(&service, &options).unwrap();
    let source = |name: &str| {
        files
            .iter()
            .find(|f| f.path.ends_with(name))
            .unwrap()
            .contents
            .clone()
    };
    let (widget, color, client) = (
        source("/model/Widget.java"),
        source("/model/Color.java"),
        source("/LinksClient.java"),
    );
    // Fully qualified references, labelled with the generated names.
    assert!(
        widget.contains(
            "A widget; {@link example.links.LinksClient#createWidget createWidget} makes one."
        ),
        "{widget}"
    );
    assert!(widget.contains("The name {@link example.links.model.CreateWidgetInput#name CreateWidgetInput.name} chose."), "{widget}");
    assert!(widget.contains("One of {@link example.links.model.Color Color}; {@link example.links.model.Color#BLUE Color.BLUE} by default."), "{widget}");
    assert!(
        color.contains("Red, unlike {@link example.links.model.Color#BLUE Color.BLUE}."),
        "{color}"
    );
    assert!(client.contains("Reads the {@link example.links.model.Widget Widget} that {@link example.links.LinksClient#createWidget createWidget} made, by {@link example.links.model.GetWidgetInput#widgetId GetWidgetInput.widgetId},"), "{client}");
    assert!(
        client.contains("Fails with {@link example.links.model.WidgetError WidgetError}."),
        "{client}"
    );
    // The service is the client; a resource, a prelude type, `[text][Name]`
    // and internal API are plain text, and code stays code.
    assert!(
        client.contains("read it back with the read call;"),
        "{client}"
    );
    assert!(
        client.contains(
            "Gadget is a resource, {@link example.links.LinksClient LinksClient} is this service"
        ),
        "{client}"
    );
    assert!(
        client.contains(" and String a prelude type. Internal: Reveal and Secret.</p>"),
        "{client}"
    );
    assert!(client.contains("<code>[NotALink]</code>"), "{client}");
    for file in &files {
        assert!(
            !file.contents.contains("#Secret") && !file.contents.contains("Reveal}"),
            "{}",
            file.path
        );
    }
}
