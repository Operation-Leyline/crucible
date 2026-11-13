use crucible_codegen_dart::{Options, emit};

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
    for file in emit(&service, &permission_options()).unwrap() {
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

/// The typed permissions fixture (`tests/fixtures/permissions`) projected for
/// a client, as `crucible generate` lowers it.
fn permissions_service() -> crucible_ir::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    crucible_ir::project(&full, crucible_ir::Audience::External).unwrap()
}

/// The fixture with a permission `{symbol}` added to `graph`, whose name
/// the language spells like an existing one's.
fn permissions_with(symbol: &str) -> crucible_ir::ir::Service {
    let mut service = permissions_service();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut twin = graph.permissions[0].clone();
    twin.symbol = symbol.into();
    twin.value = "graph:twin".into();
    twin.alias_of = None;
    graph.permissions.push(twin);
    service
}

fn permission_options() -> Options {
    Options {
        metadata: Default::default(),
        package_name: "leyline".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: None,
    }
}

#[test]
fn permission_constants_that_dart_spells_alike_are_diagnosed() {
    let constant = emit(&permissions_with("READ"), &permission_options())
        .err()
        .unwrap();
    assert!(
        constant
            .to_string()
            .contains("spells the Dart constant `read`"),
        "{constant}"
    );
}

/// The role registry (docs/roles.md §7 "Clients") from
/// `tests/models/roles-granted-by`: the `roles.dart` library, open `Role`
/// and `Scope` classes and `roles` in catalogue order, public roles, scopes
/// and permissions only, whatever the audience; checked in as
/// `tests/golden/roles/lib/roles.dart`.
#[test]
fn role_registry_matches_golden() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/roles-granted-by");
    let service = crucible_lang::load_service(&dir).unwrap();
    let options = |package: &str| Options {
        metadata: Default::default(),
        package_name: package.into(),
        package_version: "0.0.0-dev".into(),
        endpoint: None,
    };
    let roles = |audience| {
        let projected = crucible_ir::project(&service, audience).unwrap();
        let files = emit(&projected, &options("notes")).unwrap();
        assert!(
            files.iter().any(|f| f.path == "lib/src/runtime/roles.dart"),
            "the registry's record types are bundled"
        );
        files
            .into_iter()
            .find(|f| f.path == "lib/roles.dart")
            .expect("a roles library")
            .contents
    };
    let external = roles(crucible_ir::Audience::External);
    assert_eq!(roles(crucible_ir::Audience::Internal), external);
    for hidden in ["auditor", "backOffice", "note:purge", "workspace:audit"] {
        assert!(!external.contains(hidden), "{hidden}");
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/roles/lib/roles.dart");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &external).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n"),
        external,
        "rerun with CRUCIBLE_BLESS=1 and read the diff"
    );
    // A package named for the library is refused.
    let clash = emit(&service, &options("roles")).err().unwrap();
    assert!(
        clash.to_string().contains("where the roles library goes"),
        "{clash}"
    );
    // A model without roles gets neither file.
    let files = emit(&permissions_service(), &permission_options()).unwrap();
    assert!(!files.iter().any(|f| f.path.ends_with("roles.dart")));
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
    let files = emit(&service, &permission_options()).unwrap();
    let expected = r#"static const mailSend = GraphPermission("graph:mail-send");"#;
    assert!(
        files.iter().any(|file| file.contents.contains(expected)),
        "no file spells `mail-send` as {expected}"
    );
}
