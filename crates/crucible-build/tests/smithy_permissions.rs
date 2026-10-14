//! Typed permissions are `.cru`-only (docs/permissions.md §4.11), and
//! `crucible-traits` does not define their traits, so the Smithy compiler
//! refuses a `.smithy` source that uses one before lowering can explain it.
//! `crucible-build` puts lowering's explanation ahead of the compiler's
//! report.

use std::fs;
use std::io;

/// Every typed-permission trait, each where a model would have put it.
const MODEL: &str = r#"$version: "2"
namespace example.leyline

service Leyline { version: "1", operations: [GetUser, ListOrgs, DeleteOrg], resources: [Org] }

@crucible.auth#authorizationName("org")
resource Org { identifiers: { orgId: String } }

@crucible.auth#requiresNone("the caller's own credential")
operation GetUser {}

@crucible.auth#requiresPending("checked in the handler")
operation ListOrgs {}

@crucible.auth#requires([{ permission: "manage", resource: "example.leyline#Org" }])
operation DeleteOrg {}

@crucible.auth#permissionsOf("example.leyline#Org")
enum OrgPermission {
    @crucible.auth#permissionOnly
    READ = "org:read"
    @crucible.auth#notGrantable
    ADMIN = "org:admin"
    @crucible.auth#aliasOf("READ")
    VIEW = "org:view"
}

@crucible.auth#permissionTypes
enum ResourceType {
    ORG = "org"
}
"#;

/// Two of the Smithy 1.74 compiler's diagnostics for [`MODEL`], verbatim.
const REPORT: &str = "\
──  ERROR  ─────────────────────────────────────────────── Model.UnresolvedTrait
Shape: example.leyline#GetUser
File:  model/api.smithy:9:1

9 | @crucible.auth#requiresNone(\"the caller's own credential\")
  | ^
10| operation GetUser {}

Unable to resolve trait `crucible.auth#requiresNone`. If this is a custom trait,
then it must be defined before it can be used in a model.


──  ERROR  ─────────────────────────────────────────────── Model.UnresolvedTrait
Shape: example.leyline#DeleteOrg
File:  model/api.smithy:15:1

15| @crucible.auth#requires([{ permission: \"manage\", resource: \"example.leyline#Org\" }])
  | ^
16| operation DeleteOrg {}

Unable to resolve trait `crucible.auth#requires`. If this is a custom trait,
then it must be defined before it can be used in a model.

FAILURE: Validated 289 shapes (ERROR: 2)
";

fn sources() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("model")).unwrap();
    fs::write(dir.path().join("model/api.smithy"), MODEL).unwrap();
    dir
}

#[test]
fn an_unresolved_permission_trait_is_explained_before_the_compiler_report() {
    let dir = sources();
    let error = crucible_build::compile_smithy_with(
        dir.path().join("model"),
        dir.path().join("out"),
        |_, _| Err(io::Error::other(REPORT)),
    )
    .unwrap_err();
    let message = error.to_string();
    let refusal = |name: &str| crucible_smithy::permission_trait_refusal(name);
    // One line per trait the report names, in the order of
    // `PERMISSION_TRAITS`, and `requires` is not mistaken for a prefix of
    // `requiresNone` or the other way round.
    assert!(
        message.starts_with(&format!(
            "{}\n{}\n\n",
            refusal("requires"),
            refusal("requiresNone")
        )),
        "{message}"
    );
    assert!(!message.contains(&refusal("requiresPending")), "{message}");
    assert!(
        message.contains("migrate the model to .cru to declare them"),
        "{message}"
    );
    // The compiler's own report follows, unchanged.
    assert!(message.ends_with(REPORT), "{message}");
}

#[test]
fn a_report_that_names_no_permission_trait_is_unchanged() {
    let dir = sources();
    let report = "Unable to resolve trait `crucible.auth#requiresEverything`.";
    let error = crucible_build::compile_smithy_with(
        dir.path().join("model"),
        dir.path().join("out"),
        |_, _| Err(io::Error::new(io::ErrorKind::InvalidData, report)),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), report);
}

/// The pinned compiler itself, so a change to its wording that stops the
/// explanation from matching fails here.
#[cfg(feature = "smithy")]
#[test]
fn the_pinned_compiler_s_refusal_names_every_permission_trait() {
    let dir = sources();
    let message = crucible_build::compile_smithy(dir.path().join("model"), dir.path().join("out"))
        .unwrap_err()
        .to_string();
    assert!(message.contains("Model.UnresolvedTrait"), "{message}");
    for name in crucible_smithy::PERMISSION_TRAITS {
        assert!(
            message.contains(&crucible_smithy::permission_trait_refusal(name)),
            "missing {name}: {message}"
        );
    }
}
