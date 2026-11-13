//! Roles in the server bindings (docs/roles.md §7.1): the generated
//! catalogue and the closed `Role` and `Scope` enums over it, checked in
//! under `tests/golden/`. `CRUCIBLE_BLESS=1 cargo test` rewrites them.
//! The runtime's tests compile `roles_leyline.rs` and run Leyline's policy
//! tests against it.

use std::fs;
use std::path::Path;

use crucible_codegen_rust::{Options, generate};
mod common;

fn matches_golden(code: &str, file: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(file);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, code).unwrap();
    }
    let golden = fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        golden == code,
        "generated code differs from tests/golden/{file}; review it and rerun with CRUCIBLE_BLESS=1"
    );
}

/// Leyline's catalogue (docs/roles.md §8): five scopes over a resource
/// tree, inclusion, `*`, holders, assigners and a `by service` grant.
#[test]
fn leyline_roles_match_golden() {
    let service = common::load("roles-leyline");
    let code = generate(&service, &Options::default()).unwrap();
    // Generated twice, byte-identical: nothing depends on hash order.
    assert_eq!(code, generate(&service, &Options::default()).unwrap());
    matches_golden(&code, "roles_leyline.rs");
}

/// A service-wide scope and `custom` roles: `PredefinedRole` closed, `Role`
/// open.
#[test]
fn custom_roles_and_a_service_scope_match_golden() {
    let service = common::load("roles-service-scope");
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("pub enum PredefinedRole"), "{code}");
    assert!(code.contains("Custom(::std::string::String)"), "{code}");
    matches_golden(&code, "roles_service_scope.rs");
}

/// Every role custom (`roles [] custom`): the module is still generated,
/// with an empty role table, the scopes, an open `Role` that is only
/// custom, and an authorizer that requires `CustomRoles`.
#[test]
fn custom_only_roles_match_golden() {
    let service = common::load("roles-custom-only");
    assert!(service.roles.is_empty() && service.role_settings.custom);
    let code = generate(&service, &Options::default()).unwrap();
    assert!(code.contains("pub mod roles"), "{code}");
    assert!(code.contains("roles: &[\n"), "{code}");
    assert!(!code.contains("pub enum PredefinedRole"), "{code}");
    assert!(
        code.contains("authorizer!(CATALOGUE, CustomHatch, CustomRoles)"),
        "{code}"
    );
    matches_golden(&code, "roles_custom_only.rs");
}

/// A model without roles generates no `roles` module, so its bindings are
/// what they were before roles existed (the other goldens hold them).
#[test]
fn a_model_without_roles_has_no_roles_module() {
    let code = generate(&common::load("weather"), &Options::default()).unwrap();
    assert!(!code.contains("pub mod roles"));
}

/// Two roles whose symbols spell one variant are a diagnostic, not code that
/// does not compile.
#[test]
fn roles_spelled_alike_are_a_diagnostic() {
    let mut service = common::load("roles-service-scope");
    // `viewer` and `Viewer` are both `Viewer` in Rust.
    service.roles[1].symbol = "Viewer".into();
    let errors = generate(&service, &Options::default()).unwrap_err();
    assert!(
        format!("{errors:?}").contains("role variant `Viewer`"),
        "{errors:?}"
    );
}
