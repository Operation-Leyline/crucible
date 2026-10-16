//! Each element's release history (`crucible_ir::versions::history`,
//! decision 77), from the chain `crucible_lang::versions` builds, in both
//! audiences: `tests/models/version-history` has three releases and every
//! kind of step, some of them on internal API or naming it on one side.

use std::path::Path;

use crucible_ir::ShapeId;
use crucible_ir::ir::{Hook, Service, VersionStep};
use crucible_ir::versions::{History, history, history_at, named, subjects, visible};
use crucible_ir::{Audience, project};

fn package(name: &str) -> Service {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"))
}

fn service() -> Service {
    package("version-history")
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
    // Moved and given errors in v3.
    assert_eq!(at("GetItem"), dated(None, &["v3"], &[]));
    assert_eq!(at("ListItems"), dated(None, &["v2"], &[]));
    // Renamed and required in one migration: one entry.
    assert_eq!(at("ListItemsInput$limit"), dated(None, &["v2"], &[]));
    assert_eq!(at("ListItemsInput$sort"), dated(None, &["v3"], &[]));
    assert_eq!(at("Kind"), dated(None, &["v2", "v3"], &[]));
    assert_eq!(at("Kind$HUGE"), dated(Some("v2"), &[], &[]));
    assert_eq!(at("Kind$SMALL"), dated(None, &[], &[]));
    assert_eq!(at("PutItemInput$count"), dated(None, &["v2"], &[]));
    assert_eq!(at("PutItemInput$priority"), dated(None, &["v2"], &[]));
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
    assert_eq!(at("GetItemOutput$note"), dated(None, &["v2"], &[]));
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

/// Dropping the oldest release moves `added` to `null` for what the new
/// oldest has: "before every supported release" stays true.
#[test]
fn dropping_the_oldest_release_dates_its_elements_before_every_release() {
    let mut service = service();
    service.versions.remove(0);
    let histories = history(&service);
    assert_eq!(histories.get(&id("Kind$HUGE")), &dated(None, &[], &[]));
    assert_eq!(histories.get(&id("Size$area")), &dated(None, &[], &[]));
    assert_eq!(histories.get(&id("Kind")), &dated(None, &["v3"], &[]));
    assert_eq!(
        histories.get(&id("DeleteItem")),
        &dated(Some("v3"), &[], &[])
    );
}

/// `added` is the release since which every release has the element: one
/// a release dropped and a later one added again is dated by the re-add.
#[test]
fn an_element_added_again_is_dated_by_the_latest_addition() {
    let mut service = service();
    let v2 = &mut service.versions[1].service;
    let Some(crucible_ir::ir::Shape::Structure(output)) = v2.shapes.get_mut(&id("GetItemOutput"))
    else {
        panic!("GetItemOutput");
    };
    output.members.retain(|m| m.name != "kind");
    let histories = history(&service);
    assert_eq!(
        histories.get(&id("GetItemOutput$kind")),
        &dated(Some("v3"), &[], &[])
    );
}

#[test]
fn the_external_view_leaves_out_every_step_naming_internal_api() {
    let service = service();
    let internal = project(&service, Audience::Internal).unwrap();
    let external = project(&service, Audience::External).unwrap();
    // The projection leaves the older releases to the reader, marked.
    assert!(external.versions.iter().all(|v| v.external));
    assert!(!internal.versions.iter().any(|v| v.external));
    let count = |versions: &[crucible_ir::ir::ApiVersion]| {
        versions.iter().map(|v| v.steps.len()).collect::<Vec<_>>()
    };
    assert_eq!(count(&visible(&internal)), [10, 8]);
    let seen = visible(&external);
    assert_eq!(count(&seen), [7, 5]);
    for (index, version) in seen.iter().enumerate() {
        let newer = seen.get(index + 1).map_or(&external, |v| &*v.service);
        for step in &version.steps {
            let named = named(step);
            for id in &named.older {
                assert!(
                    crucible_ir::versions::has(&version.service, id),
                    "{id} in {step:?}"
                );
            }
            for id in &named.newer {
                assert!(crucible_ir::versions::has(newer, id), "{id} in {step:?}");
            }
            // A reshape lists only the members this audience saw.
            if let VersionStep::Hooked {
                hook: Hook::Reshape { members, .. },
                ..
            } = step
            {
                assert_eq!(members, &["width", "height"]);
            }
        }
        // Each release's model is projected as the current one is.
        assert!(
            !version
                .service
                .operations
                .iter()
                .any(|op| op.id == id("Audit"))
        );
    }
    let text = format!("{seen:?}");
    for internal in [
        "Audit", "secret", "hint", "depth", "SECRET", "Conflict", "priority",
    ] {
        // `priority` is public since v2; only its v1 retype names v1's
        // internal member.
        let leaked = seen
            .iter()
            .flat_map(|v| &v.steps)
            .any(|step| format!("{step:?}").contains(internal));
        assert!(!leaked, "{internal} in {text}");
    }
    // The retired operation is public, so both views keep it.
    assert_eq!(seen[0].retired, [id("PingItems")]);
    assert_eq!(visible(&internal)[0].retired, [id("PingItems")]);
    // Documentation stays as written.
    assert_eq!(
        seen[0].sunset_docs.as_deref(),
        Some("v1 is served until the end of 2026.")
    );
    // History follows the filtered steps: no change only internal API made.
    let histories = history(&external);
    assert!(
        !histories
            .iter()
            .any(|(id, _)| id.to_string().contains("secret") || id.name() == "Audit")
    );
    let at = |text: &str| histories.get(&id(text)).clone();
    assert_eq!(at("Kind"), dated(None, &["v2"], &[]));
    assert_eq!(at("GetItemOutput$title"), dated(None, &["v2"], &[]));
    // Internal in v1: externally it arrived in v2.
    assert_eq!(at("GetItemOutput$note"), dated(Some("v2"), &[], &[]));
    assert_eq!(at("PutItemInput$priority"), dated(Some("v2"), &[], &[]));
    assert_eq!(at("Size$unit"), dated(Some("v3"), &[], &["v1"]));
}

/// A frozen release the strict external projection refuses (a required
/// internal member, public since) fails no external output: the older
/// release is projected only where it is read, best effort.
#[test]
fn an_older_release_the_projection_would_refuse_fails_no_output() {
    let service = package("version-internal-required");
    let external = project(&service, Audience::External).unwrap();
    let token: ShapeId = "example.required#GetItemOutput$token".parse().unwrap();
    let seen = visible(&external);
    assert!(!crucible_ir::versions::has(&seen[0].service, &token));
    assert!(crucible_ir::versions::has(&external, &token));
    assert_eq!(history(&external).get(&token).added.as_deref(), Some("v2"));
    assert_eq!(history(&service).get(&token).added, None);
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

/// `load_versioned_service` runs the compatibility guard first, as a build
/// does: an undeclared break is refused, not documented.
#[test]
fn the_versioned_loader_refuses_an_undeclared_break() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/version-history");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("history-undeclared");
    let _ = std::fs::remove_dir_all(&dir);
    copy(&source, &dir);
    let file = dir.join("shop.cru");
    let text = std::fs::read_to_string(&file).unwrap();
    // PutItem loses its priority: a break no migration declares.
    let edited = text.replace(
        "        /// How soon to store it.\n        priority?: string\n",
        "",
    );
    assert_ne!(edited, text);
    std::fs::write(&file, edited).unwrap();
    let error = crucible_lang::load_versioned_service(&dir).unwrap_err();
    assert!(error.contains("breaks compatibility"), "{error}");
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
