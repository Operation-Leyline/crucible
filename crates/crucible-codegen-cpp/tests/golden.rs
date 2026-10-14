use crucible_codegen_cpp::{Options, emit};

fn options() -> Options {
    Options {
        package_name: "leyline_client".into(),
        namespace: "leyline".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
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

/// The typed permissions fixture as a client sees it; `edit` rewrites its
/// `.cru` source first.
fn leyline_with(edit: impl Fn(String) -> String) -> crucible_smithy::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    for file in &mut package.files {
        file.text = edit(std::mem::take(&mut file.text));
    }
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    let service = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap()
}

/// The messages of the diagnostics `emit` reports for `service`.
fn diagnostics(service: &crucible_smithy::ir::Service) -> Vec<String> {
    match emit(service, &options()) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics.iter().map(|d| d.message.clone()).collect(),
    }
}

/// Two permissions of one resource with one C++ factory are a diagnostic,
/// never a rename; a model named like a permission type is not, since the
/// types live in `<ns>::permissions`.
#[test]
fn colliding_permission_factories_are_diagnostics() {
    let service = leyline_with(|text| text.replace("permission audit =", "permission READ ="));
    let messages = diagnostics(&service);
    assert!(
        messages.contains(&"permission `READ` of `Graph` shares the C++ factory `GraphPermission::known_read` with another".to_owned()),
        "{messages:?}"
    );
    let service = leyline_with(|text| text.replace("HistoryRef", "GraphPermission"));
    assert_eq!(diagnostics(&service), Vec::<String>::new());
}

/// A model without permissions gets no permission files.
#[test]
fn a_model_without_permissions_has_no_permission_files() {
    let weather = crucible_smithy::Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let files = emit(&weather, &options()).unwrap();
    assert!(files.iter().all(|file| !file.path.contains("permissions")));
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
    let mut service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
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
    let expected = r#"static GraphPermission known_mail_send()"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}
