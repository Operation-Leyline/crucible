//! Each element's release history (`crucible_ir::versions::history`,
//! decision 77), from the chain `crucible_lang::versions` builds, in both
//! audiences: `tests/models/version-history` has three releases and every
//! kind of step, some of them on internal API or naming it on one side.

use std::path::Path;

use crucible_ir::ShapeId;
use crucible_ir::ir::{Hook, Service, VersionStep};
use crucible_ir::versions::{History, history, history_at, line, named, subjects, visible};
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
    let v2 = &mut service.versions[1].release;
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
    assert_eq!(count(&visible(&internal)), [10, 9]);
    let seen = visible(&external);
    assert_eq!(count(&seen), [7, 6]);
    for (index, version) in seen.iter().enumerate() {
        let newer = seen.get(index + 1).map_or(&external, |v| &*v.service);
        for step in &version.steps {
            let named = named(step);
            for id in &named.older {
                assert!(
                    crucible_ir::versions::has(&version.release, id),
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
        "Audit", "secret", "hint", "depth", "SECRET", "Conflict", "priority", "holder",
    ] {
        // `priority` is public since v2; only its v1 retype names v1's
        // internal member.
        let leaked = seen
            .iter()
            .flat_map(|v| &v.steps)
            .any(|step| format!("{step:?}").contains(internal));
        assert!(!leaked, "{internal} in {text}");
    }
    // A public error mapped partly from an internal member is kept: the
    // error and the operation decide, and the mapping keeps what is seen.
    let locked = seen[1]
        .steps
        .iter()
        .find_map(|step| match step {
            VersionStep::AddError { error, members, .. } if *error == id("ItemLocked") => {
                Some(members.clone())
            }
            _ => None,
        })
        .expect("the ItemLocked step is external");
    assert_eq!(locked, [("message".to_owned(), "message".to_owned())]);
    let internal_locked = visible(&internal)[1]
        .steps
        .iter()
        .find_map(|step| match step {
            VersionStep::AddError { error, members, .. } if *error == id("ItemLocked") => {
                Some(members.len())
            }
            _ => None,
        });
    assert_eq!(internal_locked, Some(2));
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
    assert!(!crucible_ir::versions::has(&seen[0].release, &token));
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

fn lines_id(text: &str) -> ShapeId {
    format!("example.lines#{text}").parse().unwrap()
}

/// What undo restored, as each release of `tests/models/version-line-history`
/// records it: v3's steps restore at v2 a removed operation with its own
/// shapes, a reshaped structure and a retyped member; v2's rename restores
/// nothing.
#[test]
fn each_release_records_what_its_undo_restored() {
    let service = package("version-line-history");
    let restored = |index: usize| -> Vec<String> {
        service.versions[index]
            .restored
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    assert_eq!(restored(0), Vec::<String>::new());
    assert_eq!(
        restored(1),
        [
            "example.lines#CountItems",
            "example.lines#CountItemsInput",
            "example.lines#CountItemsOutput",
            "example.lines#GetItemOutput$note",
            "example.lines#Size",
        ]
    );
    // The projections the compiler computed record the same.
    let projections = crucible_ir::versions::projections(&service).unwrap();
    for (projection, version) in projections.iter().zip(&service.versions) {
        assert_eq!(projection.restored, version.restored, "{}", version.name);
    }
}

/// A release's line dates what undo restored where it comes from (decision
/// 79): added and missing within the chain that ends at the release it was
/// restored from, and changed there, in the migration that restored it, and
/// in every later one that changed what it became; everything else as the
/// current model does, through the renames since. An operation added in v2
/// and removed in v3 is added in v2 at v1 too and changed in v3; a reshaped
/// structure and a retyped member are v2's, with v2's members, changed in
/// v3; a member renamed in v2 is the current one under its v1 name.
#[test]
fn a_line_dates_what_undo_restored_within_its_own_chain() {
    let current = package("version-line-history");
    let at = |histories: &crucible_ir::versions::Histories, text: &str| {
        histories.get(&lines_id(text)).clone()
    };
    let dates = history(&current);
    assert_eq!(at(&dates, "Size"), dated(None, &["v3"], &[]));
    assert_eq!(
        at(&dates, "Size$area"),
        dated(Some("v3"), &[], &["v1", "v2"])
    );
    assert_eq!(
        at(&dates, "GetItemOutput$note"),
        dated(Some("v2"), &["v3"], &[])
    );
    assert_eq!(
        at(&dates, "ListItemsInput$limit"),
        dated(None, &["v2"], &[])
    );
    for release in ["v1", "v2"] {
        let line = line(&current, release).unwrap();
        let dates = history(&line);
        // Removed in v3, restored from v2 with its own shapes.
        assert_eq!(
            at(&dates, "CountItems"),
            dated(Some("v2"), &["v3"], &[]),
            "{release}"
        );
        assert_eq!(
            at(&dates, "CountItemsInput$shelf"),
            dated(Some("v2"), &["v3"], &[])
        );
        assert_eq!(
            at(&dates, "CountItemsOutput"),
            dated(Some("v2"), &["v3"], &[])
        );
        // Reshaped in v3: v2's definition, with the member v2 added.
        assert_eq!(at(&dates, "Size"), dated(None, &["v3"], &[]), "{release}");
        assert_eq!(at(&dates, "Size$width"), dated(None, &["v3"], &[]));
        assert_eq!(at(&dates, "Size$depth"), dated(Some("v2"), &["v3"], &[]));
        // Retyped in v3: v2's member, added in v2.
        assert_eq!(
            at(&dates, "GetItemOutput$note"),
            dated(Some("v2"), &["v3"], &[])
        );
        // Nothing restored it: the current model's dates.
        assert_eq!(at(&dates, "GetItem"), dated(None, &[], &[]));
        assert_eq!(at(&dates, "GetItemOutput$name"), dated(None, &[], &[]));
    }
    // Renamed in v2: v1's `max` is the current `limit`.
    let v1 = history(&line(&current, "v1").unwrap());
    assert_eq!(at(&v1, "ListItemsInput$max"), dated(None, &["v2"], &[]));
    let v2 = history(&line(&current, "v2").unwrap());
    assert_eq!(at(&v2, "ListItemsInput$limit"), dated(None, &["v2"], &[]));
    // The current release's line is the current model.
    let v3 = history(&line(&current, "v3").unwrap());
    assert_eq!(at(&v3, "Size"), dated(None, &["v3"], &[]));
}

/// A member renamed in v2 and again in v3, then retyped in v4, keeps every
/// change in each older line: v4's retype restores v3's member, dated by
/// the chain that ends at v3 (both renames), the retype, and the current
/// member's own changes. The enum v2 added a value to is restored too, as
/// v4 retyped the member that held it and the current model has none:
/// changed in v2 and in v4, the value added in v2 where the release has it.
#[test]
fn a_line_keeps_every_change_through_renames_and_retypes() {
    let current = package("version-line-renames");
    let id = |text: &str| -> ShapeId { format!("example.renames#{text}").parse().unwrap() };
    let dates = history(&current);
    assert_eq!(
        dates.get(&id("GetItemOutput$third")),
        &dated(None, &["v2", "v3", "v4"], &[])
    );
    assert!(!current.shapes.contains_key(&id("Kind")));
    for (release, member) in [("v1", "first"), ("v2", "second"), ("v3", "third")] {
        let line = line(&current, release).unwrap();
        let dates = history(&line);
        let at = |text: &str| dates.get(&id(text)).clone();
        assert_eq!(
            at(&format!("GetItemOutput${member}")),
            dated(None, &["v2", "v3", "v4"], &[]),
            "{release}"
        );
        assert_eq!(at("GetItemOutput$kind"), dated(None, &["v4"], &[]));
        assert_eq!(at("Kind"), dated(None, &["v2", "v4"], &[]), "{release}");
        assert_eq!(at("Kind$SMALL"), dated(None, &["v4"], &[]));
        // v1's clients see a huge item as large: v1 has no HUGE.
        assert_eq!(
            crucible_ir::versions::has(&line, &id("Kind$HUGE")),
            release != "v1",
            "{release}"
        );
        if release != "v1" {
            assert_eq!(at("Kind$HUGE"), dated(Some("v2"), &["v4"], &[]));
        }
    }
}

/// The e2e shop's v1 reference: a member retyped since is the release's
/// own, changed by the retype; a member renamed and required since is the
/// current one; and a structure reshaped since is v2's, changed by the
/// reshape, its renamed member by the rename too.
#[test]
fn the_shops_v1_line_dates_each_element_where_it_comes_from() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/weather-service/versioned");
    let current = crucible_lang::load_versioned_service(&dir).unwrap_or_else(|e| panic!("{e}"));
    let shop = |text: &str| -> ShapeId { format!("example.shop#{text}").parse().unwrap() };
    let v1 = history(&line(&current, "v1").unwrap());
    let at = |text: &str| v1.get(&shop(text)).clone();
    assert_eq!(at("ListItemsInput$max"), dated(None, &["v2", "v3"], &[]));
    assert_eq!(at("DeleteItemInput$force"), dated(None, &["v3"], &[]));
    assert_eq!(at("DeleteItemOutput$deleted"), dated(None, &["v3"], &[]));
    assert_eq!(at("GetItemOutput"), dated(None, &["v3"], &[]));
    assert_eq!(at("GetItemOutput$name"), dated(None, &["v2", "v3"], &[]));
    assert_eq!(at("CountItems"), dated(None, &["v3"], &[]));
    assert_eq!(at("Label$size"), dated(Some("v3"), &["v4"], &[]));
    assert_eq!(at("Label$color"), dated(Some("v4"), &[], &[]));
    assert_eq!(at("GetItem"), dated(None, &["v3"], &[]));
}

/// The documentation of a definition undo restored is its release's
/// (decision 79); everything else keeps the current documentation, a
/// renamed member included.
#[test]
fn restored_definitions_keep_their_releases_documentation() {
    let current = package("version-line-history");
    fn docs(service: &Service, id: &ShapeId) -> Option<String> {
        if let Some(op) = service.operations.iter().find(|op| &op.id == id) {
            return op.docs.clone();
        }
        let shape = service.shapes.get(&id.root()).expect("shape");
        match id.member() {
            Some(name) => shape
                .members()
                .into_iter()
                .find(|m| m.name == name)
                .and_then(|m| m.docs.clone()),
            None => {
                let value = serde_json::to_value(shape).unwrap();
                let (_, fields) = value.as_object().unwrap().iter().next().unwrap();
                fields["docs"].as_str().map(str::to_owned)
            }
        }
    }
    let v1 = line(&current, "v1").unwrap();
    let at = |text: &str| docs(&v1, &lines_id(text));
    assert_eq!(at("Size").as_deref(), Some("A size, as v2 measured it."));
    assert_eq!(
        at("Size$depth").as_deref(),
        Some("How deep the item is, measured since v2.")
    );
    assert_eq!(
        at("GetItemOutput$note").as_deref(),
        Some("A note about the item, as text.")
    );
    assert_eq!(
        at("CountItems").as_deref(),
        Some("Counts the items on a shelf, as v2 counts them.")
    );
    // Not restored: the current documentation, under the release's name.
    assert_eq!(
        at("ListItemsInput$max").as_deref(),
        Some("The most names to return.")
    );
    assert_eq!(at("GetItem").as_deref(), Some("Reads one item."));
    assert_eq!(
        docs(&current, &lines_id("Size")).as_deref(),
        Some("A size, as an area.")
    );
}

/// Decision 74's payload case: a member added since to a message an older
/// release sent as a payload, and its projection sends so again, is not in
/// that release, which has no body for it; `missing` says so, for an
/// operation's input and for an error, in either audience.
#[test]
fn a_member_a_payload_bound_message_leaves_out_is_missing_there() {
    let mut service = package("version-projection-payload");
    let payload = |text: &str| -> ShapeId { format!("example.payload#{text}").parse().unwrap() };
    for audience in [Audience::Internal, Audience::External] {
        let projected = project(&service, audience).unwrap();
        let dates = history(&projected);
        assert_eq!(
            dates.get(&payload("PutNoteInput$title")),
            &dated(Some("v2"), &[], &["v1"]),
            "{audience:?}"
        );
        // The release had them, or they travel outside the body.
        assert_eq!(
            dates.get(&payload("PutNoteInput$body")),
            &dated(None, &["v2"], &[])
        );
        assert_eq!(
            dates.get(&payload("PutNoteInput$id")),
            &dated(None, &[], &[])
        );
    }
    // An error whose body is a payload at v1, with a member added since.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/error-payloads");
    let wire = crucible_lang::load_service(&dir).unwrap_or_else(|e| panic!("{e}"));
    let error: ShapeId = "example.wire#TextError".parse().unwrap();
    let released = wire.shapes[&error].clone();
    let mut grown = released.clone();
    let crucible_ir::ir::Shape::Structure(s) = &mut grown else {
        panic!("TextError");
    };
    assert!(s.error.as_ref().is_some_and(|e| e.payload.is_some()));
    let mut extra = s.members[0].clone();
    extra.name = "extra".into();
    extra.required = false;
    s.members.push(extra);
    service.shapes.insert(error.clone(), grown);
    service.versions[0]
        .service
        .shapes
        .insert(error.clone(), released.clone());
    service.versions[0]
        .release
        .shapes
        .insert(error.clone(), released);
    let dates = history(&service);
    assert_eq!(
        dates.get(&error.with_member("extra")),
        &dated(Some("v2"), &[], &["v1"])
    );
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
