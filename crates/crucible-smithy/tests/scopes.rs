use crucible_smithy::{Capabilities, Feature, Model};

fn lower(json: &str) -> Result<crucible_smithy::ir::Service, crucible_smithy::Error> {
    Model::from_json(json).unwrap().lower_with_capabilities(
        None,
        &Capabilities::new([Feature::OptionalAuth, Feature::ApiKeyAuth]),
    )
}

fn diagnostics(json: &str) -> Vec<String> {
    match lower(json) {
        Err(crucible_smithy::Error::Unsupported(diagnostics)) => {
            diagnostics.iter().map(ToString::to_string).collect()
        }
        other => panic!("expected diagnostics, lowered {other:?}"),
    }
}

fn expect(diagnostics: &[String], shape: &str, fragment: &str) {
    assert!(
        diagnostics
            .iter()
            .any(|d| d.starts_with(&format!("example.scopes#{shape}:")) && d.contains(fragment)),
        "no diagnostic on {shape} containing {fragment:?} in {diagnostics:#?}"
    );
}

#[test]
fn operations_inherit_override_or_opt_out_of_the_service_default() {
    let service = lower(include_str!("fixtures/scopes.json")).unwrap();
    let scopes = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap()
            .scopes
            .clone()
    };
    assert_eq!(service.scopes, ["weather/read"]);
    assert_eq!(scopes("Inherits"), ["weather/read"]);
    // Model order is kept: the list is data the model owns, not a set.
    assert_eq!(scopes("Overrides"), ["weather/write", "weather/admin"]);
    assert!(scopes("OptsOut").is_empty());
    // A public operation has no caller to hold a scope, so it inherits none.
    assert!(scopes("Public").is_empty());
    assert_eq!(scopes("OptionalWithScopes"), ["weather/read"]);
}

#[test]
fn the_service_scope_list_is_sorted_and_unique() {
    let service = lower(include_str!("fixtures/scopes.json")).unwrap();
    assert_eq!(
        service.all_scopes(),
        ["weather/admin", "weather/read", "weather/write"]
    );
}

#[test]
fn a_service_without_scopes_lowers_empty_lists() {
    let service = lower(include_str!("fixtures/auth-chain.json")).unwrap();
    assert!(service.scopes.is_empty());
    assert!(service.operations.iter().all(|op| op.scopes.is_empty()));
    assert!(service.all_scopes().is_empty());
}

#[test]
fn misuse_is_collected_against_the_shape_that_declares_it() {
    let diagnostics = diagnostics(include_str!("fixtures/scopes-refused.json"));
    expect(&diagnostics, "NotAList", "must be a list of scope strings");
    expect(&diagnostics, "NotAString", "entries must be strings");
    expect(&diagnostics, "Empty", "is not a scope token");
    expect(&diagnostics, "HasSpace", "is not a scope token");
    // Both the quote and the backslash are refused; one run reports both.
    assert_eq!(
        diagnostics
            .iter()
            .filter(
                |d| d.starts_with("example.scopes#HasQuote:") && d.contains("not a scope token")
            )
            .count(),
        2
    );
    expect(&diagnostics, "NonAscii", "is not a scope token");
    expect(&diagnostics, "Duplicate", "more than once");
    expect(
        &diagnostics,
        "PublicWithScopes",
        "requires an authenticated operation",
    );
    assert_eq!(diagnostics.len(), 9, "{diagnostics:#?}");
}

#[test]
fn a_service_default_needs_an_authentication_scheme() {
    let diagnostics = diagnostics(include_str!("fixtures/scopes-unauthenticated-service.json"));
    expect(
        &diagnostics,
        "NoAuthWeather",
        "requires the service to declare an authentication scheme",
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
}
