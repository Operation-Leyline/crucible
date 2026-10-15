//! Every `.cru` file under `docs/idl/examples` lexes losslessly and parses
//! without a diagnostic. The examples are the language's design artifacts,
//! so the parser and the design cannot drift apart.

use std::path::{Path, PathBuf};

use crucible_lang::ast::{BodyKind, Decl, ExprKind, MemberKind, OpEntry, Quantifier, StepKind};
use crucible_lang::{LineIndex, parse};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/idl/examples")
}

fn cru_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            cru_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "cru") {
            out.push(path);
        }
    }
}

fn read(relative: &str) -> String {
    let path = examples().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn every_example_round_trips_and_parses_cleanly() {
    let mut files = Vec::new();
    cru_files(&examples(), &mut files);
    assert!(files.len() >= 6, "found {files:?}");
    let mut failures = String::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("readable");
        let parsed = parse(&source);
        let rebuilt: String = parsed
            .tokens
            .iter()
            .map(|token| token.span.text(&source))
            .collect();
        assert_eq!(rebuilt, source, "{} is not lossless", path.display());
        let lines = LineIndex::new(&source);
        for diagnostic in &parsed.diagnostics {
            failures.push_str(&diagnostic.render(&path.display().to_string(), &source, &lines));
        }
        assert!(
            !parsed.file.items.is_empty(),
            "{} has no items",
            path.display()
        );
    }
    assert!(failures.is_empty(), "\n{failures}");
}

fn operation<'a>(
    file: &'a crucible_lang::ast::File,
    name: &str,
) -> &'a crucible_lang::ast::Operation {
    file.items
        .iter()
        .find_map(|item| match &item.decl {
            Decl::Operation(op) if op.name.name == name => Some(op),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no operation {name}"))
}

#[test]
fn weather_operations_attach_to_their_resource() {
    let parsed = parse(&read("weather/model/weather.cru"));
    let get = operation(&parsed.file, "GetCity");
    let attachment = get.attachment.as_ref().expect("attached");
    assert_eq!(attachment.role.name, "reads");
    assert_eq!(attachment.resource.name, "City");
    let input = get
        .entries
        .iter()
        .find_map(|entry| match entry {
            OpEntry::Body(body) if body.kind == BodyKind::Input => Some(body),
            _ => None,
        })
        .expect("input");
    assert!(matches!(
        &input.members[0].kind,
        MemberKind::Elided { name, optional: false, .. } if name.name == "cityId"
    ));
    let MemberKind::Field { name, optional, .. } = &input.members[1].kind else {
        panic!("field");
    };
    assert_eq!((name.name.as_str(), *optional), ("at", true));
    assert_eq!(input.members[1].annotations[0].name.name, "query");
}

#[test]
fn documentation_attaches_to_examples_and_declarations() {
    let parsed = parse(&read("weather/model/weather.cru"));
    let create = operation(&parsed.file, "CreateCity");
    let example = create
        .entries
        .iter()
        .find_map(|entry| match entry {
            OpEntry::Example(example) if example.title.value == "Create a city" => Some(example),
            _ => None,
        })
        .expect("example");
    assert_eq!(
        example.doc.as_ref().map(|doc| doc.text.as_str()),
        Some("Creates Lisbon with coordinates.")
    );
    let photo = parsed
        .file
        .items
        .iter()
        .find(|item| matches!(&item.decl, Decl::Scalar(s) if s.name.name == "Photo"))
        .expect("Photo");
    assert_eq!(
        photo.doc.as_ref().map(|d| d.text.as_str()),
        Some("A photo, carried unparsed.")
    );
}

#[test]
fn leyline_rules_parse_into_clauses() {
    let parsed = parse(&read("leyline-graph/policy/leyline.cru"));
    let semantics = parsed
        .file
        .items
        .iter()
        .find_map(|item| match &item.decl {
            Decl::Rule(rule) if rule.name.name == "ClassMatchesSemantics" => Some(rule),
            _ => None,
        })
        .expect("rule");
    assert_eq!(semantics.flags[0].name, "strict");
    assert_eq!(semantics.clauses.len(), 2);
    assert_eq!(semantics.clauses[1].quantifier, Quantifier::Every);
    let condition = semantics.clauses[1].condition.as_ref().expect("where");
    assert!(matches!(condition.kind, ExprKind::Or(..)));
    assert!(matches!(
        semantics.clauses[1].requirements[0].kind,
        ExprKind::Not(_)
    ));
    let excepts = parsed
        .file
        .items
        .iter()
        .filter(|item| matches!(item.decl, Decl::Except(_)))
        .count();
    assert_eq!(excepts, 1);
}

#[test]
fn migration_steps_parse() {
    let parsed = parse(&read("leyline-graph/migrations/2026-11-01.cru"));
    let Decl::Migration(migration) = &parsed.file.items[0].decl else {
        panic!("migration");
    };
    assert_eq!(migration.version.value, "2026-11-01");
    assert_eq!(migration.from.value, "2026-09-04");
    let verbs: Vec<&str> = migration
        .steps
        .iter()
        .map(|step| match &step.kind {
            StepKind::Rename { .. } => "rename",
            StepKind::Require { .. } => "require",
            StepKind::Add { .. } => "add",
            StepKind::Move { .. } => "move",
            StepKind::Reshape { .. } => "reshape",
            StepKind::RemoveOperation(_) => "remove",
            StepKind::Behavior(_) => "behavior",
            StepKind::Sunset(_) => "sunset",
            StepKind::Retype { .. } => "retype",
        })
        .collect();
    assert_eq!(
        verbs,
        [
            "rename", "require", "add", "add", "move", "require", "reshape", "reshape", "remove",
            "behavior", "sunset"
        ]
    );
    let StepKind::Add { name, mapping, .. } = &migration.steps[3].kind else {
        panic!("add");
    };
    assert_eq!(
        name.as_ref().map(|n| n.name.as_str()),
        Some("GraphNameReservedException")
    );
    assert_eq!(mapping.len(), 2);
    let StepKind::Require { how, .. } = &migration.steps[5].kind else {
        panic!("require");
    };
    assert!(matches!(
        how,
        crucible_lang::ast::RequireHow::Hook(hook) if hook.is_async
    ));
}

/// The example packages compile: weather without errors, and the others with only
/// "not supported yet" diagnostics for what the compiler does not lower yet.
#[test]
fn every_example_package_compiles() {
    for (package, clean) in [("weather", true), ("standards", false)] {
        let dir = examples().join(package);
        let loaded = match crucible_lang::load_package(&dir) {
            Ok(loaded) => loaded,
            Err(error) => {
                // Every manifest problem is one the compiler has decided but not built.
                assert!(
                    !clean
                        && error
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .all(|line| line.contains("not supported by the compiler yet")),
                    "{package}: {error}"
                );
                continue;
            }
        };
        let compiled = crucible_lang::compile(&loaded, None);
        let rendered = compiled.render();
        if clean {
            // Weather's one warning is deliberate; see weather_ir.rs.
            assert!(!compiled.has_errors(), "{package}:\n{rendered}");
            continue;
        }
        for diagnostic in &compiled.diagnostics {
            let message = &diagnostic.diagnostic.message;
            assert!(
                !diagnostic.diagnostic.is_error()
                    || message.contains("not supported")
                    || message.contains("declares no service"),
                "{package}: unexpected {message:?}\n{rendered}"
            );
        }
    }
}

/// Truncating any example at any point gives diagnostics, never a panic:
/// an editor compiles every half-typed state.
#[test]
fn truncated_examples_never_panic() {
    let mut files = Vec::new();
    cru_files(&examples(), &mut files);
    for path in &files {
        let source = std::fs::read_to_string(path).expect("readable");
        let cuts: Vec<usize> = source
            .char_indices()
            .map(|(at, _)| at)
            .step_by(13)
            .chain([source.len()])
            .collect();
        for cut in cuts {
            let text = &source[..cut];
            let package = crucible_lang::Package::new(
                "example.fuzz",
                vec![crucible_lang::SourceFile {
                    path: "fuzz.cru".into(),
                    text: text.to_owned(),
                }],
            );
            let compiled = crucible_lang::compile(&package, None);
            let _ = compiled.render();
            let _ = crucible_lang::format(text, &Default::default());
        }
    }
}

/// The leyline-graph example's rules, its own and org.standards', fire on an
/// operation and a shape that break them, and its exception is recorded.
#[test]
fn the_leyline_example_rules_fire() {
    let dir = examples().join("leyline-graph");
    let mut package = crucible_lang::load_package(&dir).expect("loads");
    let clean = crucible_lang::compile(&package, None);
    assert!(!clean.has_errors(), "{}", clean.render());
    let service = clean.service.expect("service");
    assert_eq!(service.exceptions.len(), 1);
    assert_eq!(service.exceptions[0].rule, "NoDocument");
    assert_eq!(service.exceptions[0].rule_package, "org.standards");
    assert_eq!(service.exceptions[0].target, "SearchTerms");

    package.files.push(crucible_lang::SourceFile {
        path: "broken.cru".into(),
        text: r#"
string Loose

/// Reads the loose string, but is neither classified nor audited, and a GET
/// that is not marked readonly is a lie the standard forbids.
operation GetLoose on Graph {
    http POST "/v1/loose"
    input {
        $organizationId
        $graphId
        loose?: Loose
    }
}
"#
        .into(),
    });
    package.files.push(crucible_lang::SourceFile {
        path: "excepts.cru".into(),
        text: "except Classified for GetLoose because \"no\"
except HttpMethodSemantics for GetGraph because \"stale\"
"
        .into(),
    });
    let broken = crucible_lang::compile(&package, None);
    let messages: Vec<&str> = broken
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    let said = |needle: &str| {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "no message has {needle:?}: {messages:#?}"
        );
    };
    // Its own rules.
    said("rule Classified: `GetLoose`");
    // org.standards' rules, which Leyline cannot weaken or except.
    said("rule org.standards#BoundedStrings: `Loose`");
    said("rule TenantInPath: `GetLoose`");
    said("rule `Classified` is strict: no exception can excuse it");
    said("`except HttpMethodSemantics for GetGraph` excuses nothing");
}
