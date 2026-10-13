//! Loading a package from disk and importing the bundled `crucible.errors`.

use std::path::PathBuf;

use crucible_lang::{check_manifest, compile, is_package, load_package};
use crucible_smithy::ShapeId;

fn package_dir(name: &str, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("crucible.toml"), manifest).unwrap();
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

const SERVICE: &str = r#"
service Shop {
    version "1"
    protocol json
    operations [Ping]
    errors [ValidationException, InternalServerException]
}

readonly operation Ping {
    http GET "/ping"
    output { ok: boolean }
}
"#;

#[test]
fn editor_overlays_use_the_packages_source_selection() {
    let manifest = "[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\n";
    let dir = package_dir(
        "editor-overlays",
        manifest,
        &[
            ("model/a.cru", "string Saved\n"),
            ("model/ops/existing.cru", "string Existing\n"),
            (
                "model/child/crucible.toml",
                "[model]\nnamespace = \"example.child\"\n",
            ),
            ("model/child/saved.cru", "string Child\n"),
        ],
    );
    let overlay = |path: &str, text: &str| crucible_lang::SourceFile {
        path: dir.join(path).display().to_string(),
        text: text.into(),
    };
    let mut overlays = vec![
        overlay("model/ops/new.cru", "string NestedNew\n"),
        overlay("excluded.cru", "string Excluded\n"),
        overlay("model/new.cru", "string New\n"),
        overlay("model/a.cru", "string Edited\n"),
        overlay("model/child/new.cru", "string ChildNew\n"),
        overlay("model/readme.txt", "string NotAModel\n"),
    ];
    // Editors may spell the same Windows file with different path casing.
    if cfg!(windows) {
        overlays[3].path = overlays[3].path.to_lowercase();
    }
    let package =
        crucible_lang::package::load_package_with_overlays(&dir, manifest, &overlays).unwrap();
    let texts: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.text.as_str())
        .collect();
    assert_eq!(
        texts,
        [
            "string Edited\n",
            "string New\n",
            "string Existing\n",
            "string NestedNew\n"
        ]
    );
    let disk = load_package(&dir).unwrap();
    assert_eq!(disk.files.len(), 2);
    assert_eq!(disk.files[0].text, "string Saved\n");
}

#[test]
fn a_packages_first_source_can_be_an_unsaved_buffer() {
    let manifest = "[model]\nnamespace = \"example.shop\"\n";
    let dir = package_dir("editor-first-source", manifest, &[]);
    let overlays = [crucible_lang::SourceFile {
        path: dir.join("first.cru").display().to_string(),
        text: "string First\n".into(),
    }];
    let package =
        crucible_lang::package::load_package_with_overlays(&dir, manifest, &overlays).unwrap();
    assert_eq!(package.namespace, "example.shop");
    assert_eq!(package.files.len(), 1);
    assert_eq!(package.files[0].path, overlays[0].path);
    assert_eq!(package.files[0].text, overlays[0].text);
    assert!(
        load_package(&dir)
            .unwrap_err()
            .contains("declares no .cru files")
    );
}

#[test]
fn a_package_loads_its_manifest_and_every_cru_file_in_order() {
    let dir = package_dir(
        "loads",
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n\n[model.naming]\noutput = \"{Operation}Response\"\n",
        &[
            ("b/service.cru", SERVICE),
            ("a/types.cru", "string Note { length ..10 }\n"),
        ],
    );
    assert!(is_package(&dir));
    let package = load_package(&dir).unwrap();
    assert_eq!(package.namespace, "example.shop");
    assert_eq!(package.imports, ["crucible.errors"]);
    assert!(
        package.files[0].path.ends_with("types.cru"),
        "{:?}",
        package.files[0].path
    );
    let compiled = compile(&package, None);
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let service = compiled.service.unwrap();
    assert_eq!(
        service.operations[0]
            .output
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("example.shop#PingResponse")
    );
    // Imported shapes keep their own namespace.
    assert!(
        service
            .errors
            .contains(&ShapeId::new("crucible.errors", "ValidationException"))
    );
    assert!(
        service
            .shape(&ShapeId::new(
                "crucible.errors",
                "ValidationExceptionReason"
            ))
            .is_some()
    );
}

#[test]
fn source_paths_have_no_dot_components() {
    // `sources = ["."]` is the package directory itself: its files print as
    // `dir/ops/x.cru`, not `dir/./ops/x.cru`. (`PathBuf` equality ignores
    // `.` components, so the spellings are compared as text.)
    let dir = package_dir(
        "dot-sources",
        "[model]\nnamespace = \"example.shop\"\nsources = [\".\", \"./ops\", \"ops/.\"]\n",
        &[("service.cru", SERVICE), ("ops/types.cru", "string Note\n")],
    );
    let package = load_package(&dir).unwrap();
    let paths: Vec<&str> = package.files.iter().map(|f| f.path.as_str()).collect();
    let expected = [
        dir.join("ops").join("types.cru").display().to_string(),
        dir.join("service.cru").display().to_string(),
    ];
    assert_eq!(paths, expected);
}

#[test]
fn a_policy_only_manifest_is_not_a_package() {
    let dir = package_dir(
        "policy-only",
        "schema_version = 1\n[rules.naming]\nseverity = \"error\"\n",
        &[],
    );
    assert!(!is_package(&dir));
}

#[test]
fn imports_must_be_known_and_names_must_not_collide() {
    let dir = package_dir(
        "unknown-import",
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.nothing\"]\n",
        &[("service.cru", SERVICE)],
    );
    // Loading refuses it, as the editor's manifest check does.
    let error = load_package(&dir).unwrap_err();
    assert!(
        error.contains("unknown import `crucible.nothing`"),
        "{error}"
    );
    // Compiled directly, the bad import does not hide the package's own problems.
    let mut package = crucible_lang::Package::new(
        "example.shop",
        vec![crucible_lang::SourceFile {
            path: "service.cru".into(),
            text: format!("{SERVICE}\nstructure Extra {{ a: Missing }}\n"),
        }],
    );
    package.imports = vec!["crucible.nothing".into()];
    let rendered = compile(&package, None).render();
    assert!(
        rendered.contains("unknown import `crucible.nothing`"),
        "{rendered}"
    );
    assert!(rendered.contains("unknown type `Missing`"), "{rendered}");

    let dir = package_dir(
        "collision",
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n",
        &[(
            "service.cru",
            &format!(
                "{SERVICE}\nerror ValidationException {{\n    status 400\n    message: string\n}}\n"
            ),
        )],
    );
    let compiled = compile(&load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    assert!(
        rendered.contains("service.cru")
            && rendered.contains("also declared in the imported package crucible.errors"),
        "{rendered}"
    );
}

#[test]
fn a_package_without_a_service_still_reports_every_other_problem() {
    let dir = package_dir(
        "no-service",
        "[model]\nnamespace = \"example.shop\"\n",
        &[("types.cru", "structure A { a: Missing }\n")],
    );
    let rendered = compile(&load_package(&dir).unwrap(), None).render();
    assert!(rendered.contains("declares no service"), "{rendered}");
    assert!(rendered.contains("unknown type `Missing`"), "{rendered}");
}

#[test]
fn manifest_problems_are_reported() {
    let dir = package_dir("no-namespace", "[model]\n", &[("service.cru", SERVICE)]);
    assert!(
        load_package(&dir)
            .unwrap_err()
            .contains("must name the package namespace")
    );
    let dir = package_dir("no-files", "[model]\nnamespace = \"example.shop\"\n", &[]);
    assert!(
        load_package(&dir)
            .unwrap_err()
            .contains("declares no .cru files")
    );
}

#[test]
fn manifest_problems_point_at_their_keys() {
    let dir = package_dir("manifest-check", "", &[("model/a.cru", "")]);
    let check = |text: &str| -> Vec<(String, String)> {
        check_manifest(&dir, text)
            .into_iter()
            .map(|d| (d.span.text(text).to_owned(), d.message))
            .collect()
    };
    assert!(check("[model]\nnamespace = \"example.shop\"\nsources = [\"model\"]\n").is_empty());
    // A policy-only manifest is not checked here.
    assert!(check("schema_version = 1\n[rules.naming]\nseverity = \"error\"\n").is_empty());

    let problems = check(
        "[model]\nnamespace = \"9bad\"\nsources = [\"nowhere\"]\nimports = [\"crucible.nothing\"]\n[model.naming]\ninput = \"Request\"\n",
    );
    let at: Vec<&str> = problems.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(
        at,
        [
            "\"9bad\"",
            "\"nowhere\"",
            "\"crucible.nothing\"",
            "\"Request\""
        ],
        "{problems:?}"
    );

    let missing = check("[model]\nsources = [\"model\"]\n");
    assert!(
        missing[0].1.contains("must name the package namespace"),
        "{missing:?}"
    );

    let typo = check("[model]\nnamespace = \"a\"\nnamspace = \"b\"\n");
    assert!(
        typo[0].1.contains("unknown field `namspace`") && typo[0].0.contains("namspace"),
        "{typo:?}"
    );

    let syntax = check("[model\n");
    assert_eq!(syntax.len(), 1, "{syntax:?}");
}

#[test]
fn sources_stay_inside_the_package() {
    let dir = package_dir(
        "escaping",
        "[model]\nnamespace = \"example.shop\"\nsources = [\"../shared\"]\n",
        &[("service.cru", SERVICE)],
    );
    assert!(
        load_package(&dir)
            .unwrap_err()
            .contains("must stay inside the package directory")
    );
    let text = "[model]\nnamespace = \"example.shop\"\nsources = [\"../shared\", \"/abs\"]\n";
    let problems: Vec<String> = check_manifest(&dir, text)
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems[0].contains("inside the package") && problems[1].contains("relative"),
        "{problems:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_directory_symlink_loop_is_not_followed() {
    let dir = package_dir(
        "symlink-loop",
        "[model]\nnamespace = \"example.shop\"\n",
        &[("service.cru", SERVICE)],
    );
    std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();
    let package = load_package(&dir).unwrap();
    assert_eq!(package.files.len(), 1);
}

#[test]
fn the_manifest_checks_cover_language_naming_and_namespaces() {
    let dir = package_dir("manifest-more", "", &[("model/a.cru", "")]);
    let messages = |text: &str| -> Vec<String> {
        check_manifest(&dir, text)
            .into_iter()
            .map(|d| d.message)
            .collect()
    };
    assert!(messages("[model]\nnamespace = \"smithy.api\"\n")[0].contains("reserved"));
    assert!(messages("[model]\nnamespace = \"crucible.errors\"\n")[0].contains("reserved"));
    assert!(
        messages("[model]\nnamespace = \"a\"\nlanguage = 999\n")[0].contains("language version 1")
    );
    assert!(messages("[model]\nnamespace = \"a\"\nlanguage = 1\n").is_empty());
    let same = messages(
        "[model]\nnamespace = \"a\"\n[model.naming]\ninput = \"{Operation}Body\"\noutput = \"{Operation}Body\"\n",
    );
    assert!(same[0].contains("must differ"), "{same:?}");
    let spaced =
        messages("[model]\nnamespace = \"a\"\n[model.naming]\ninput = \"{Operation}-in put\"\n");
    assert!(spaced[0].contains("make a name"), "{spaced:?}");
    let drive = messages("[model]\nnamespace = \"a\"\nsources = [\"C:foo\"]\n");
    assert!(!drive.is_empty() || !cfg!(windows), "{drive:?}");
}

#[test]
fn a_syntax_error_in_one_file_does_not_hide_another_files_problems() {
    let package = crucible_lang::Package::new(
        "example.shop",
        vec![
            crucible_lang::SourceFile {
                path: "a.cru".into(),
                text: "structure A {\n    a: = \n}\n".into(),
            },
            crucible_lang::SourceFile {
                path: "b.cru".into(),
                text: format!("{SERVICE}\nstructure B {{ b: Missing }}\n"),
            },
        ],
    );
    let rendered = compile(&package, None).render();
    assert!(rendered.contains("a.cru"), "{rendered}");
    assert!(rendered.contains("unknown type `Missing`"), "{rendered}");
}

#[test]
fn a_bad_namespace_is_reported_when_compiling() {
    let rendered = compile(&crucible_lang::Package::new("smithy.api", vec![]), None).render();
    assert!(rendered.contains("reserved"), "{rendered}");
}

/// One shape of `smithy/errors.smithy`, read with just enough of the IDL for
/// that file: what the bundled `.cru` copy must agree with.
#[derive(Debug, Default, PartialEq)]
struct SmithyShape {
    kind: String,
    docs: Option<String>,
    status: Option<u16>,
    retryable: bool,
    /// (name, required, header) for members, (name, value) as (name, false, value) for enums.
    members: Vec<(String, bool, Option<String>)>,
}

fn smithy_errors() -> std::collections::BTreeMap<String, SmithyShape> {
    let text = include_str!("../../../smithy/errors.smithy");
    let mut shapes = std::collections::BTreeMap::new();
    let mut current: Option<(String, SmithyShape)> = None;
    let (mut docs, mut status, mut retryable, mut required, mut header) =
        (Vec::new(), None, false, false, None);
    for line in text.lines().map(str::trim) {
        if let Some(doc) = line.strip_prefix("///") {
            docs.push(doc.trim().to_owned());
        } else if let Some(code) = line.strip_prefix("@httpError(") {
            status = code.trim_end_matches(')').parse().ok();
        } else if line == "@retryable" {
            retryable = true;
        } else if line == "@required" {
            required = true;
        } else if let Some(name) = line.strip_prefix("@httpHeader(\"") {
            header = Some(name.trim_end_matches("\")").to_owned());
        } else if let Some(rest) = ["structure ", "enum ", "list "]
            .iter()
            .find_map(|kind| line.strip_prefix(kind).map(|rest| (kind.trim(), rest)))
        {
            let (kind, rest) = rest;
            let name = rest.trim_end_matches('{').trim().to_owned();
            let shape = SmithyShape {
                kind: kind.to_owned(),
                docs: (!docs.is_empty()).then(|| docs.join("\n")),
                status: status.take(),
                retryable: std::mem::take(&mut retryable),
                members: Vec::new(),
            };
            docs.clear();
            current = Some((name, shape));
        } else if line == "}" {
            if let Some((name, shape)) = current.take() {
                shapes.insert(name, shape);
            }
        } else if let Some((_, shape)) = current.as_mut()
            && !line.is_empty()
            && !line.starts_with("//")
        {
            let (name, value) = line
                .split_once(" = ")
                .map(|(n, v)| (n, Some(v.trim_matches('"').to_owned())))
                .unwrap_or_else(|| (line.split(':').next().unwrap_or(line), None));
            let member = if shape.kind == "enum" {
                (name.trim().to_owned(), false, value)
            } else {
                (
                    name.trim().to_owned(),
                    std::mem::take(&mut required),
                    header.take(),
                )
            };
            shape.members.push(member);
        }
    }
    shapes
}

#[test]
fn the_bundled_errors_match_smithy_errors_smithy() {
    use crucible_smithy::ir::Shape;
    let expected = smithy_errors();
    assert_eq!(expected.len(), 8, "{expected:#?}");
    let errors: Vec<&str> = expected
        .iter()
        .filter(|(_, shape)| shape.status.is_some())
        .map(|(name, _)| name.as_str())
        .collect();
    let mut package = crucible_lang::Package::new(
        "example.shop",
        vec![crucible_lang::SourceFile {
            path: "service.cru".into(),
            text: format!(
                "service Shop {{\n    version \"1\"\n    protocol json\n    operations [Ping]\n    errors [{}]\n}}\nreadonly operation Ping {{\n    http GET \"/ping\"\n}}\n",
                errors.join(", ")
            ),
        }],
    );
    package.imports = vec!["crucible.errors".into()];
    let compiled = compile(&package, None);
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let service = compiled.service.unwrap();
    for (name, smithy) in &expected {
        let shape = service
            .shape(&ShapeId::new("crucible.errors", name))
            .unwrap_or_else(|| panic!("{name} is not bundled"));
        let actual = match shape {
            Shape::Structure(s) => SmithyShape {
                kind: "structure".into(),
                docs: s.docs.clone(),
                status: s.error.as_ref().map(|e| e.status),
                retryable: s.error.as_ref().is_some_and(|e| e.retryable.is_some()),
                members: s
                    .members
                    .iter()
                    .map(|m| {
                        let header = s.error.as_ref().and_then(|e| {
                            e.headers
                                .iter()
                                .find(|b| b.member == m.name)
                                .map(|b| b.name.clone())
                        });
                        (m.name.clone(), m.required, header)
                    })
                    .collect(),
            },
            Shape::Enum(e) => SmithyShape {
                kind: "enum".into(),
                docs: e.docs.clone(),
                members: e
                    .values
                    .iter()
                    .map(|v| (v.name.clone(), false, Some(v.value.clone())))
                    .collect(),
                ..SmithyShape::default()
            },
            Shape::List(l) => SmithyShape {
                kind: "list".into(),
                docs: l.docs.clone(),
                members: vec![("member".into(), false, None)],
                ..SmithyShape::default()
            },
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(&actual, smithy, "{name}");
    }
}
