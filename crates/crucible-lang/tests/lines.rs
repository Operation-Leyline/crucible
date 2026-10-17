//! SDK lines (`crucible_ir::versions::line`, docs/api-versions-clients.md 5c,
//! decision 76): one supported release as the current model sees it, which
//! every output but a server is generated from. crucible-ir cannot
//! dev-depend on the compiler, so lines are tested here, from `.cru`
//! packages lowered with their versions as a build lowers them.

use std::path::Path;

use crucible_ir::ir::Service;
use crucible_ir::versions::{VERSION_HEADER, is_line, line, line_current, projections, releases};

fn load(path: &str) -> Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"))
}

fn shop() -> Service {
    load("e2e/weather-service/versioned")
}

/// A model as data: `Service` has no `PartialEq`.
fn json(service: &Service) -> serde_json::Value {
    serde_json::to_value(service).unwrap()
}

fn operations(service: &Service) -> Vec<&str> {
    service.operations.iter().map(|op| op.name()).collect()
}

/// A line is the release's projection, the model the server serves the
/// release from, with the release's name as `version` and `api_version`.
#[test]
fn a_line_is_its_releases_projection() {
    let current = shop();
    assert_eq!(releases(&current), ["v1", "v2", "v3", "v4"]);
    let v1 = line(&current, "v1").unwrap();
    assert_eq!(v1.version, "v1");
    assert_eq!(v1.api_version.as_deref(), Some("v1"));
    let projection = projections(&current).unwrap().remove(0);
    assert_eq!(projection.name, "v1");
    // The same model, its chain aside.
    let mut bare = v1.clone();
    bare.versions.clear();
    assert_eq!(json(&bare), json(&projection.service));
    // What v1 had (CountItems, removed in v3) and what was added since
    // (ListLabels, GetLabel, PutLabel) are both in its line.
    let names = operations(&v1);
    for name in [
        "CountItems",
        "GetItem",
        "ListLabels",
        "GetLabel",
        "PutLabel",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
    assert_eq!(
        crucible_ir::versions::current_name(&v1),
        "v1",
        "a client of the line sends {VERSION_HEADER}: v1"
    );
}

/// A line is recognizable without a field of its own: its `version` names
/// one of its own `versions`. The current model's never does.
#[test]
fn a_line_is_recognizable_and_keeps_the_current_release() {
    let current = shop();
    assert!(!is_line(&current));
    let v2 = line(&current, "v2").unwrap();
    assert!(is_line(&v2));
    // The chain is all of it: the older releases, then the current one.
    let names: Vec<&str> = v2.versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["v1", "v2", "v3", "v4"]);
    assert_eq!(releases(&v2), ["v1", "v2", "v3", "v4"]);
    // So the current model it came from is found again, chain and all.
    assert_eq!(json(&line_current(&v2).unwrap()), json(&current));
    assert!(line_current(&current).is_none());
    // A line is not projected again.
    let error = line(&v2, "v1").unwrap_err().to_string();
    assert!(error.contains("already release \"v2\""), "{error}");
}

/// The current release is the current model itself, whose server and
/// clients are generated as before.
#[test]
fn the_current_release_is_the_current_model() {
    let current = shop();
    assert_eq!(json(&line(&current, "v4").unwrap()), json(&current));
    // A service without migrations has one release, its own.
    let weather = load("tests/models/weather");
    assert_eq!(releases(&weather), [weather.version.as_str()]);
    assert_eq!(
        json(&line(&weather, &weather.version).unwrap()),
        json(&weather)
    );
}

/// A release outside the chain is a diagnostic naming the ones served.
#[test]
fn an_unknown_release_names_the_supported_ones() {
    let error = line(&shop(), "v0").unwrap_err().to_string();
    assert!(
        error.contains(r#"release "v0" is not one example.shop#Shop serves: its releases are "v1", "v2", "v3", "v4""#),
        "{error}"
    );
    let weather = load("tests/models/weather");
    assert!(line(&weather, "v1").is_err());
}

/// A sunset release is still in the chain until its migration is removed,
/// so its line is still generated (v1's migration declares a sunset).
#[test]
fn a_sunset_release_can_still_be_generated() {
    let current = shop();
    assert_eq!(current.versions[0].name, "v1");
    assert!(current.versions[0].sunset.is_some());
    assert_eq!(line(&current, "v1").unwrap().version, "v1");
}
