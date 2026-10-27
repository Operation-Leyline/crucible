use crucible_codegen_csharp::{Options, emit};
use crucible_ir::{ShapeId, ir::Service};
use std::{fs, path::Path};
fn model_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}
fn weather() -> Service {
    crucible_lang::load_service(&model_dir("weather")).unwrap()
}
/// The weather package with `edit` applied to every source file.
fn weather_with(edit: impl Fn(&str) -> String) -> Service {
    let mut package = crucible_lang::load_package(&model_dir("weather")).unwrap();
    for file in &mut package.files {
        file.text = edit(&file.text);
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled.service.unwrap_or_else(|| {
        panic!(
            "the model compiles:
{rendered}"
        )
    })
}
fn options() -> Options {
    Options {
        metadata: Default::default(),
        namespace: "Example.Weather".into(),
        package_id: "Example.Weather".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: Some("https://weather.example".into()),
    }
}
#[test]
fn weather_matches_goldens() {
    let service = weather();
    for file in emit(&service, &options()).unwrap() {
        if file.path.ends_with(".cs")
            && !file.path.starts_with("Models/")
            && !["WeatherClient.cs", "ModelRegistry.cs"].contains(&file.path.as_str())
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
    options.namespace = "../escape".into();
    options.package_id = "bad<id".into();
    assert_eq!(emit(&service, &options).err().unwrap().len(), 2);
}

#[test]
fn model_name_diagnostics_are_collected() {
    let mut service = weather_with(|text| {
        text.replace(
            "location?: Location",
            "location?: Location\n        message?: string\n        messageValue?: string",
        )
    });
    service.rename_shape(&ShapeId::new("example.weather", "CitySummary"), "CON");
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 2);
}

#[test]
fn service_client_name_is_reserved() {
    let mut service = weather();
    service.rename_shape(
        &ShapeId::new("example.weather", "CitySummary"),
        "WeatherClient",
    );
    assert!(emit(&service, &options()).is_err());
}

fn docs_files() -> Vec<crucible_codegen_csharp::File> {
    let service = crucible_lang::load_service(&model_dir("client-docs-jvm")).unwrap();
    let mut options = options();
    options.namespace = "Example.Docs".into();
    emit(&service, &options).unwrap()
}

#[test]
fn model_documentation_is_well_formed_xml_doc() {
    let files = docs_files();
    for file in files.iter().filter(|f| f.path.ends_with(".cs")) {
        for line in file
            .contents
            .lines()
            .filter(|l| l.trim_start().starts_with("///"))
        {
            let text = line.trim_start().trim_start_matches('/').trim();
            // Raw angle brackets from the model must arrive escaped.
            assert!(
                !text.contains("<b>") && !text.contains("<T>"),
                "{}: {line}",
                file.path
            );
        }
    }
    let client = &files
        .iter()
        .find(|f| f.path.ends_with("DocsClient.cs"))
        .unwrap()
        .contents;
    assert!(client.contains("&lt;b&gt;html&lt;/b&gt; @param"));
    assert!(client.contains("<see href=\"https://example.com/a_b\">docs</see>"));
    assert!(client.contains("<exception cref=\"global::Example.Docs.Models.Unavailable\">"));
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
    let input = source("Models/GreetInput.cs");
    // Direct, via a sensitive target shape, and via a collection of one.
    for member in ["Password", "Token", "Secrets"] {
        assert!(
            input.contains(&format!(
                "(\"{member}\", global::Example.Docs.ModelFormat.Redact(@{member}Presence))"
            )),
            "{member} must be redacted"
        );
    }
    assert!(input.contains("(\"Names\", @NamesPresence)"));
    // `@sensitive` on the shape covers its untagged members.
    assert!(source("Models/Classified.cs").contains("ModelFormat.Redact(@Detail"));
    assert!(source("Models/Profile.cs").contains("ModelFormat.Redact(@PinPresence)"));
    assert!(source("Models/Choice.cs").contains("ModelFormat.Redact(@CodePresence)"));
    assert!(source("Models/Unavailable.cs").contains("ModelFormat.Redact(@RetryTokenPresence)"));
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

/// A permission spelled as another's C# member, or as a member the record
/// has, is diagnosed against the service, never renamed.
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
    for (symbol, value) in [("Read", "graph:reread"), ("value", "graph:value")] {
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
        diagnostics.contains("is the C# member `Read`"),
        "{diagnostics}"
    );
    assert!(
        diagnostics.contains("is the C# member `Value`"),
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
    let expected = r#"GraphPermission @MailSend = new("graph:mail-send")"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}

#[test]
fn doc_links_are_see_cref_elements() {
    let service = crucible_lang::load_service(&model_dir("doc-links")).unwrap();
    let service = crucible_ir::project(&service, crucible_ir::Audience::External).unwrap();
    let options = Options {
        metadata: Default::default(),
        namespace: "Example.Links".into(),
        package_id: "Example.Links".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    let files = emit(&service, &options).unwrap();
    let all: String = files.iter().map(|f| f.contents.as_str()).collect();
    let client = &files
        .iter()
        .find(|f| f.path == "LinksClient.cs")
        .unwrap()
        .contents;
    // Fully qualified, as `<exception cref>` spells them.
    assert!(all.contains("A widget; <see cref=\"global::Example.Links.LinksClient.CreateWidgetAsync\"/> makes one."), "{all}");
    assert!(
        all.contains(
            "The name <see cref=\"global::Example.Links.Models.CreateWidgetInput.Name\"/> chose."
        ),
        "{all}"
    );
    assert!(all.contains("One of <see cref=\"global::Example.Links.Models.Color\"/>; <see cref=\"global::Example.Links.Models.Color.BLUE\"/> by default."), "{all}");
    assert!(client.contains("Reads the <see cref=\"global::Example.Links.Models.Widget\"/> that <see cref=\"global::Example.Links.LinksClient.CreateWidgetAsync\"/> made, by <see cref=\"global::Example.Links.Models.GetWidgetInput.WidgetId\"/>,"), "{client}");
    assert!(
        client.contains("Fails with <see cref=\"global::Example.Links.Models.WidgetError\"/>."),
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
            "Gadget is a resource, <see cref=\"global::Example.Links.LinksClient\"/> is this"
        ),
        "{client}"
    );
    assert!(
        client.contains("String a prelude type. Internal: Reveal and Secret."),
        "{client}"
    );
    assert!(client.contains("<c>[NotALink]</c> in code"), "{client}");
    assert!(!all.contains("Models.Secret\"") && !all.contains("RevealAsync\""));
}

#[test]
fn configured_error_documentation_preserves_unconfigured_runtime() {
    let service = crucible_lang::load_service(&model_dir("error-docs")).unwrap();
    let files = emit(&service, &options()).expect("configured error documentation is supported");
    let runtime = files
        .iter()
        .find(|file| file.path == "ServiceError.cs")
        .unwrap();
    assert!(runtime.contents.contains("DocUrl"));
    let ordinary = emit(&weather(), &options()).unwrap();
    let runtime = ordinary
        .iter()
        .find(|file| file.path == "ServiceError.cs")
        .unwrap();
    assert!(!runtime.contents.contains("DocUrl"));
}

#[test]
fn documentation_member_collision_is_configured_only() {
    let mut service = crucible_lang::load_service(&model_dir("error-docs")).unwrap();
    let id = service
        .shapes
        .keys()
        .find(|id| id.name() == "Denied")
        .unwrap()
        .clone();
    if let crucible_ir::ir::Shape::Structure(shape) = service.shapes.get_mut(&id).unwrap() {
        shape
            .members
            .iter_mut()
            .find(|m| m.name == "message")
            .unwrap()
            .name = "docUrl".into();
    }
    let diagnostics = emit(&service, &options())
        .err()
        .expect("configured documentation collision");
    assert!(format!("{diagnostics:?}").contains("DocUrl"));
    service.error_docs_base = None;
    emit(&service, &options()).expect("no-base modeled DocUrl remains allowed");
}
