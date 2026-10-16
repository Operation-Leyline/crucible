//! What the compiler refuses while it projects each older release and checks
//! it (docs/api-versions-clients.md 5b, decisions 68-71): a name an older
//! release still uses, reused; a step's older side removed since; a shape
//! renamed since a step named it; a step whose newer side was removed under
//! an exception; a projection that is not its frozen release. Each package
//! under `tests/models/` is refused at the place its diagnostic names.

use std::path::{Path, PathBuf};

use crucible_lang::{LineIndex, compile, guard_in, load_package, versions_in};
use crucible_policy::Config;

fn model(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}

fn policy(dir: &Path) -> Config {
    let manifest = dir.join("crucible.toml");
    if Config::declares_policy(&std::fs::read_to_string(&manifest).unwrap()) {
        Config::load(&manifest).unwrap()
    } else {
        Config::default()
    }
}

/// One diagnostic, as `file:line:col: severity: message`.
struct Found {
    at: String,
    error: bool,
    message: String,
}

/// What the guard and the versions report for the package, as a build
/// shows it: the guard's own problems and findings, then what the versions
/// add.
fn check(name: &str) -> (Vec<String>, Vec<Found>) {
    let dir = model(name);
    let loaded = load_package(&dir).unwrap();
    let own = loaded.files.len();
    let compiled = compile(&loaded, None);
    assert!(compiled.service.is_some(), "{name}: {}", compiled.render());
    let config = policy(&dir);
    let releases = crucible_lang::Releases::default();
    let guard = guard_in(&loaded, &compiled, own, &config, &releases)
        .unwrap()
        .unwrap();
    let mut guarded: Vec<String> = guard
        .undeclared
        .iter()
        .filter(|finding| finding.severity == crucible_policy::Severity::Error)
        .map(|finding| format!("{} {}", finding.rule, finding.shape))
        .collect();
    guarded.extend(
        guard
            .problems
            .iter()
            .filter(|item| item.diagnostic.is_error())
            .map(|item| item.diagnostic.message.clone()),
    );
    let mut versions = versions_in(&loaded, &compiled, own, &config, &releases);
    versions.without(&guard);
    let found = versions
        .diagnostics
        .iter()
        .map(|item| {
            let file = &compiled.files[item.file.expect("every problem has a place")];
            let (line, column) =
                LineIndex::new(&file.text).line_col(&file.text, item.diagnostic.span.start);
            let base = Path::new(&file.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            Found {
                at: format!("{base}:{line}:{column}"),
                error: item.diagnostic.is_error(),
                message: item.diagnostic.message.clone(),
            }
        })
        .collect();
    (guarded, found)
}

/// The package's one problem: an error at `at` whose message starts with
/// `message`. The guard, which sees only the newest migration, passes it.
fn refused(name: &str, at: &str, message: &str) {
    let (guarded, found) = check(name);
    assert_eq!(guarded, Vec::<String>::new(), "{name}: the guard passes it");
    let [only] = &found[..] else {
        panic!(
            "{name}: one problem, not {:#?}",
            found
                .iter()
                .map(|f| format!("{}: {}", f.at, f.message))
                .collect::<Vec<_>>()
        );
    };
    assert!(only.error, "{name}: {}", only.message);
    assert_eq!(only.at, at, "{name}: {}", only.message);
    assert!(
        only.message.starts_with(message),
        "{name}: {}",
        only.message
    );
}

#[test]
fn an_older_member_name_cannot_be_reused() {
    refused(
        "version-reused-member-name",
        "shop.cru:13:9",
        "`GetItemOutput.name` is the older name of `GetItemOutput.title`, which release \"v1\" still uses (`rename GetItem.output.name -> title` in the migration to \"v2\")",
    );
}

#[test]
fn a_member_removed_under_an_exception_cannot_be_added_again() {
    refused(
        "version-readded-member",
        "shop.cru:13:9",
        "`GetItemOutput.size` is added again, but release \"v1\" still has the `size` the migration to \"v2\" removed",
    );
}

#[test]
fn a_removed_operations_name_cannot_be_reused() {
    refused(
        "version-reused-operation-name",
        "shop.cru:14:20",
        "CountItems names an operation release \"v1\" still serves as removed (`remove operation CountItems` in the migration to \"v2\")",
    );
}

#[test]
fn a_moved_operations_route_cannot_be_reused() {
    refused(
        "version-reused-route",
        "shop.cru:14:20",
        "FindItem is routed at `GET \"/items/{id}\"`, where release \"v1\" still routes GetItem (`move GetItem from GET \"/items/{id}\"` in the migration to \"v2\")",
    );
}

#[test]
fn a_restored_shapes_id_cannot_be_reused() {
    refused(
        "version-reused-shape-id",
        "shop.cru:21:11",
        "Tag is declared again as a different shape, but release \"v1\" still has the Tag that `remove operation ListTags` restores there",
    );
}

#[test]
fn a_downgrade_value_must_survive() {
    refused(
        "version-lost-downgrade",
        "shop.cru:16:29",
        "Kind had no value LARGE in release \"v1\" as the current model sees it; what a step names on its older side must survive while release \"v1\" is supported",
    );
}

#[test]
fn a_shape_renamed_since_a_step_named_it_is_refused_at_the_step() {
    refused(
        "version-renamed-shape",
        "shop.cru:18:12",
        "`rename Item.name -> title` names example.renamed#Item, which release \"v2\" as the current model sees it does not have",
    );
}

#[test]
fn a_step_whose_newer_side_was_removed_under_an_exception_is_refused() {
    refused(
        "version-lost-newer-side",
        "shop.cru:14:12",
        "`rename GetItem.output.name -> title`: release \"v2\" as the current model sees it no longer has example.lost#GetItemOutput$title, removed under the exception for `compatibility.member` (Items have no title any more), `compatibility.sdk` (Items have no title any more), but release \"v1\" still has example.lost#GetItemOutput$name",
    );
}

/// Excepting the older side too leaves the step inert, with a warning.
#[test]
fn excepting_the_older_side_too_leaves_the_step_inert() {
    let (guarded, found) = check("version-inert-step");
    assert_eq!(guarded, Vec::<String>::new());
    let [only] = &found[..] else {
        panic!("one warning");
    };
    assert!(!only.error);
    assert_eq!(only.at, "shop.cru:14:12");
    assert!(
        only.message
            .starts_with("`rename GetItem.output.name -> title` is inert"),
        "{}",
        only.message
    );
    // The step is left out of what the server converts.
    let dir = model("version-inert-step");
    let service = crucible_lang::load_versioned_service(&dir).unwrap();
    let v1 = &service.versions[0];
    assert_eq!(v1.name, "v1");
    assert!(v1.steps.is_empty(), "{:?}", v1.steps);
}

/// The check runs both families at error whatever `[compatibility]` says:
/// the guard only warns about the member v2 adds, which reaches v1.
#[test]
fn a_projection_is_checked_at_error_whatever_the_settings() {
    let (_, found) = check("version-incompatible-projection");
    let [only] = &found[..] else {
        panic!("one problem");
    };
    assert!(only.error);
    // At the migration that should have declared it.
    assert_eq!(only.at, "shop.cru:17:21");
    assert_eq!(
        only.message,
        "Release \"v1\" as the current model sees it is not its frozen release: [compatibility.sdk] example.incompatible#GetItemOutput$count: new public member can change construction or exhaustive matching APIs; declare the change in the migration to \"v2\", or except `compatibility.sdk` for example.incompatible#GetItemOutput$count in the manifest"
    );
}

/// Every problem is collected, each at its place, in one run.
#[test]
fn every_problem_is_reported() {
    let (guarded, found) = check("version-many-problems");
    assert_eq!(guarded, Vec::<String>::new());
    let mut seen: Vec<(String, String)> = found
        .iter()
        .map(|f| {
            assert!(f.error, "{}", f.message);
            (f.at.clone(), f.message.chars().take(40).collect())
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            (
                "shop.cru:14:9".to_owned(),
                "`GetItemOutput.name` is the older name o".to_owned()
            ),
            (
                "shop.cru:21:20".to_owned(),
                "CountItems names an operation release \"v".to_owned()
            ),
            (
                "shop.cru:29:29".to_owned(),
                "Kind had no value LARGE in release \"v1\" ".to_owned()
            ),
        ]
    );
}

/// A package with nothing to refuse reports nothing beyond its warnings:
/// every versioned package the other tests use.
#[test]
fn accepted_packages_report_no_error() {
    for name in [
        "version-steps",
        "version-history",
        "version-renamed-enum-value",
        "version-retired-operation",
        "version-projection",
        "version-projection-auth",
        "version-projection-retype",
        "version-projection-derived",
        "version-projection-conditions",
        "version-projection-payload",
    ] {
        let (guarded, found) = check(name);
        assert_eq!(guarded, Vec::<String>::new(), "{name}");
        for f in &found {
            assert!(!f.error, "{name}: {}: {}", f.at, f.message);
        }
    }
}

/// An example a release leaves out is a warning at the example.
#[test]
fn an_example_left_out_of_a_release_is_a_warning_at_the_example() {
    let (_, found) = check("version-projection");
    let [only] = &found[..] else {
        panic!(
            "one warning, not {:?}",
            found.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    };
    assert!(!only.error);
    assert_eq!(only.at, "shop.cru:82:13");
    assert!(
        only.message
            .starts_with("example \"Archive a huge item\" does not hold at release \"v2\""),
        "{}",
        only.message
    );
}
