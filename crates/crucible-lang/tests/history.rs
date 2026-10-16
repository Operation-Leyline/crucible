//! Each element's release history (`crucible_ir::versions::history`,
//! decision 77), from the chain `crucible_lang::versions` builds, in both
//! audiences: `tests/models/version-history` has three releases and every
//! kind of step, two of them on internal API.

use std::path::Path;

use crucible_ir::ShapeId;
use crucible_ir::ir::{Hook, Service, VersionStep};
use crucible_ir::versions::{History, history, history_at, subjects};
use crucible_ir::{Audience, project};

fn service() -> Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/version-history");
    crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"))
}

fn id(text: &str) -> ShapeId {
    format!("example.history#{text}").parse().unwrap()
}

fn dated(added: Option<&str>, changed: &[&str], missing: &[&str]) -> History {
    History {
        added: added.map(str::to_owned),
        changed: changed.iter().map(|s| (*s).to_owned()).collect(),
        missing: missing.iter().map(|s| (*s).to_owned()).collect(),
    }
}

#[test]
fn every_element_is_dated_from_the_chain_following_renames() {
    let service = service();
    let names: Vec<&str> = service.versions.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["v1", "v2"]);
    let histories = history(&service);
    let at = |text: &str| histories.get(&id(text)).clone();
    // Renamed in v2, so the same member as v1's `name`.
    assert_eq!(at("GetItemOutput$title"), dated(None, &["v2"], &[]));
    // Moved and given an error in v3.
    assert_eq!(at("GetItem"), dated(None, &["v3"], &[]));
    assert_eq!(at("ListItems"), dated(None, &["v2"], &[]));
    // Renamed and required in one migration: one entry.
    assert_eq!(at("ListItemsInput$limit"), dated(None, &["v2"], &[]));
    assert_eq!(at("ListItemsInput$sort"), dated(None, &["v3"], &[]));
    assert_eq!(at("Kind"), dated(None, &["v2"], &[]));
    assert_eq!(at("Kind$HUGE"), dated(Some("v2"), &[], &[]));
    assert_eq!(at("Kind$SMALL"), dated(None, &[], &[]));
    assert_eq!(at("PutItemInput$count"), dated(None, &["v2"], &[]));
    assert_eq!(at("PutItemOutput$total"), dated(None, &["v3"], &[]));
    // Reshaped in v2: v1 sees its own definition, without what came since.
    assert_eq!(at("Size"), dated(None, &["v2"], &[]));
    assert_eq!(at("Size$area"), dated(Some("v2"), &[], &["v1"]));
    assert_eq!(at("Size$unit"), dated(Some("v3"), &[], &["v1"]));
    // Not frozen yet: added in the release being made.
    assert_eq!(at("DeleteItem"), dated(Some("v3"), &[], &[]));
    assert_eq!(at("ItemGone"), dated(Some("v3"), &[], &[]));
    // Internal API is dated in the internal view.
    assert_eq!(at("GetItemOutput$secret"), dated(None, &["v2"], &[]));
    assert_eq!(at("Audit"), dated(None, &["v3"], &[]));
}

#[test]
fn an_older_release_is_dated_within_the_chain_that_ends_there() {
    let service = service();
    let v2 = history_at(&service, 1);
    assert_eq!(v2.get(&id("CountItems")), &dated(None, &[], &[]));
    assert_eq!(v2.get(&id("Size$area")), &dated(Some("v2"), &[], &["v1"]));
    assert_eq!(
        v2.get(&id("GetItemOutput$title")),
        &dated(None, &["v2"], &[])
    );
    let v1 = history_at(&service, 0);
    assert_eq!(v1.get(&id("PingItems")), &dated(None, &[], &[]));
    assert_eq!(v1.get(&id("Size$width")), &dated(None, &[], &[]));
}

#[test]
fn a_service_without_versions_dates_nothing() {
    let mut service = service();
    service.versions.clear();
    let histories = history(&service);
    assert!(
        histories
            .iter()
            .all(|(_, history)| history == &History::default()),
        "{:?}",
        histories.iter().find(|(_, h)| **h != History::default())
    );
}

#[test]
fn the_external_view_leaves_out_internal_steps_and_history() {
    let service = service();
    let internal = project(&service, Audience::Internal).unwrap();
    let external = project(&service, Audience::External).unwrap();
    let count = |s: &Service| s.versions.iter().map(|v| v.steps.len()).collect::<Vec<_>>();
    assert_eq!(count(&internal), [8, 6]);
    assert_eq!(count(&external), [7, 5]);
    // The internal steps are the ones left out.
    let audit = id("Audit");
    let secret = id("GetItemOutput$secret");
    for version in &external.versions {
        for step in &version.steps {
            let named = subjects(step);
            assert!(
                !named.contains(&audit) && !named.contains(&secret),
                "{step:?}"
            );
        }
        // Each release's model is projected as the current one is.
        assert!(!version.service.operations.iter().any(|op| op.id == audit));
    }
    assert!(
        internal.versions[0]
            .service
            .operations
            .iter()
            .any(|op| op.id == audit)
    );
    // The retired operation is public, so both views keep it.
    assert_eq!(external.versions[0].retired, [id("PingItems")]);
    assert_eq!(internal.versions[0].retired, [id("PingItems")]);
    // Documentation stays as written.
    assert_eq!(
        external.versions[0].sunset_docs.as_deref(),
        Some("v1 is served until the end of 2026.")
    );
    let histories = history(&external);
    assert!(
        !histories
            .iter()
            .any(|(id, _)| *id == secret || *id == audit)
    );
    assert_eq!(
        histories.get(&id("GetItemOutput$title")),
        &dated(None, &["v2"], &[])
    );
    assert_eq!(
        histories.get(&id("Size$unit")),
        &dated(Some("v3"), &[], &["v1"])
    );
}

#[test]
fn every_step_kind_names_its_subjects() {
    let service = service();
    let mut kinds = Vec::new();
    for version in &service.versions {
        for step in &version.steps {
            let kind = match step {
                VersionStep::Rename { .. } => "rename",
                VersionStep::Require { .. } => "require",
                VersionStep::AddEnumValue { .. } => "addEnumValue",
                VersionStep::AddError { .. } => "addError",
                VersionStep::Move { .. } => "move",
                VersionStep::Behavior { .. } => "behavior",
                VersionStep::Hooked { hook, .. } => match hook {
                    Hook::Reshape { .. } => "reshape",
                    Hook::Retype { .. } => "retype",
                    Hook::Require { .. } => "require by hook",
                    Hook::RemoveOperation { .. } => "removeOperation",
                },
            };
            kinds.push(kind);
            assert_eq!(subjects(step).len(), 1, "{step:?}");
        }
    }
    for kind in [
        "rename",
        "require",
        "require by hook",
        "addEnumValue",
        "addError",
        "move",
        "behavior",
        "reshape",
        "retype",
        "removeOperation",
    ] {
        assert!(kinds.contains(&kind), "{kind} in {kinds:?}");
    }
}
