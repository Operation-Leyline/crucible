//! `internal resource` (decision 83): what it may hold, what the compiler
//! refuses so that nothing internal reaches the external contract, what the
//! projection leaves, and how releases and the guard treat the flag.

use std::path::{Path, PathBuf};

use crucible_ir::ir::{Resource, Service, Shape};
use crucible_ir::versions::visible;
use crucible_ir::{Audience, ShapeId, project, snapshot};
use crucible_lang::{Compiled, Package, SourceFile, compile, guard, load_package, load_service};
use crucible_policy::{Config, Severity, diff};

fn models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models")
}

fn full() -> Service {
    load_service(&models().join("internal-resource"))
        .unwrap_or_else(|rendered| panic!("internal-resource:\n{rendered}"))
}

fn external() -> Service {
    project(&full(), Audience::External).unwrap()
}

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.staff", name)
}

fn resource<'s>(service: &'s Service, name: &str) -> &'s Resource {
    service
        .resources
        .get(&id(name))
        .unwrap_or_else(|| panic!("no resource {name}"))
}

fn names(ids: &[ShapeId]) -> Vec<&str> {
    ids.iter().map(ShapeId::name).collect()
}

/// The package's sources with `edit` applied, CRLF checkouts normalized.
fn compiled_with(edit: impl Fn(&str) -> String) -> Compiled {
    let mut package = load_package(&models().join("internal-resource")).unwrap();
    for file in &mut package.files {
        file.text = edit(&file.text.replace("\r\n", "\n"));
    }
    compile(&package, None)
}

/// The package with every `internal` modifier gone: the same contract, all
/// of it public.
fn all_public() -> Service {
    let compiled = compiled_with(|text| text.replace("internal ", ""));
    compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()))
}

/// The internal resource the serialized view still names, if any.
fn names_an_internal_resource(service: &Service) -> Option<&'static str> {
    let text = serde_json::to_string(service).unwrap().to_lowercase();
    ["bulletin", "marginalia"]
        .into_iter()
        .find(|name| text.contains(name))
}

#[test]
fn an_internal_resource_compiles_with_every_role_and_internal_entries() {
    let service = full();
    let bulletin = resource(&service, "Bulletin");
    assert!(bulletin.internal);
    assert!(!resource(&service, "Shelf").internal);
    let lifecycle = &bulletin.lifecycle;
    assert_eq!(lifecycle.create, Some(id("CreateBulletin")));
    assert_eq!(lifecycle.read, Some(id("GetBulletin")));
    assert_eq!(lifecycle.update, Some(id("UpdateBulletin")));
    assert_eq!(lifecycle.delete, Some(id("DeleteBulletin")));
    assert_eq!(lifecycle.list, Some(id("ListBulletins")));
    assert_eq!(names(&bulletin.operations), ["PinBulletin"]);
    assert_eq!(names(&bulletin.collection_operations), ["ArchiveBulletins"]);
    // Entries whose types are internal are the resource's to hold.
    assert_eq!(bulletin.identifiers["bulletinId"], id("BulletinId"));
    assert_eq!(bulletin.properties["tone"], id("BulletinTone"));
    assert!(service.shape(&id("BulletinId")).unwrap().internal());
    assert_eq!(
        project(&service, Audience::Internal)
            .unwrap()
            .resources
            .len(),
        3
    );
}

#[test]
fn the_compiler_refuses_what_would_reach_the_external_contract() {
    let refused = load_service(&models().join("internal-resource-refusals")).unwrap_err();
    for message in [
        "`Hidden` is internal, and `GetHidden` is part of the external contract: an operation bound to an internal resource must be `internal` too",
        "`Hidden` is internal, and `PokeHidden` is part of the external contract: an operation bound to an internal resource must be `internal` too",
        "`Hidden` is internal, and `Exposed` is part of the external contract: a resource in an internal resource must be `internal` too",
        "permissions.internal-resource: `Hidden` is internal, and `CheckedAgainstHidden` is part of the external contract: its requirement would name an internal resource to its callers; make the operation `internal`",
        "permissions.types: \"hidden\" names `Hidden`, which is internal, and is part of the external contract: mark the value `internal`",
        "`sensitive` does not apply to a resource",
    ] {
        assert!(refused.contains(message), "{message}\n{refused}");
    }
}

#[test]
fn every_refusal_lifts_when_the_operation_or_child_is_internal() {
    // The same refusals package, with what each one asked for done.
    let mut package = load_package(&models().join("internal-resource-refusals")).unwrap();
    for file in &mut package.files {
        file.text = file
            .text
            .replace("\r\n", "\n")
            .replace("sensitive resource", "resource")
            .replace(
                "readonly operation GetHidden",
                "internal readonly operation GetHidden",
            )
            .replace("operation PokeHidden", "internal operation PokeHidden")
            .replace("resource Exposed", "internal resource Exposed")
            .replace(
                "readonly operation CheckedAgainstHidden",
                "internal readonly operation CheckedAgainstHidden",
            )
            .replace(
                "    hidden = \"hidden\"",
                "    internal hidden = \"hidden\"",
            )
            .replace(
                "    exposed = \"exposed\"",
                "    internal exposed = \"exposed\"",
            );
    }
    let compiled = compile(&package, None);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|item| item.diagnostic.is_error())
        .map(|item| item.diagnostic.message.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn the_formatter_keeps_the_modifier() {
    let source = "internal resource Notice {\n    id noticeId: string\n}\n";
    assert_eq!(
        crucible_lang::format(source, &crucible_lang::FormatOptions::default()),
        source
    );
}

#[test]
fn the_external_view_has_no_trace_of_it() {
    let external = external();
    assert_eq!(names_an_internal_resource(&external), None);
    assert_eq!(
        external
            .resources
            .keys()
            .map(|r| r.name())
            .collect::<Vec<_>>(),
        ["Shelf"]
    );
    assert_eq!(names(&external.resource_roots), ["Shelf"]);
    assert_eq!(
        external
            .permissions
            .keys()
            .map(|r| r.name())
            .collect::<Vec<_>>(),
        ["Shelf"]
    );
    assert!(external.operations.iter().all(|op| !op.internal));
    assert_eq!(external.operations.len(), 5);
    // The enum of grantable resources keeps its public values and is still
    // the view's permission types, without the resource it excluded.
    let types = external
        .permission_types
        .as_ref()
        .expect("permission types");
    assert!(types.excluded.is_empty());
    let Some(Shape::Enum(target)) = external.shape(&types.shape) else {
        panic!("{:?} is a string enum", types.shape);
    };
    assert_eq!(
        target
            .values
            .iter()
            .map(|v| v.value.as_str())
            .collect::<Vec<_>>(),
        ["shelf"]
    );
}

#[test]
fn an_internal_child_of_a_public_resource_leaves_its_parent_alone() {
    let service = full();
    let child = resource(&service, "Marginalia");
    assert!(child.internal);
    assert_eq!(
        child.identifiers.keys().collect::<Vec<_>>(),
        ["marginaliaId", "shelfId"],
    );
    assert_eq!(
        names(&resource(&service, "Shelf").resources),
        ["Marginalia"]
    );
    let external = project(&service, Audience::External).unwrap();
    let shelf = resource(&external, "Shelf");
    assert!(shelf.resources.is_empty());
    assert_eq!(shelf.lifecycle.read, Some(id("GetShelf")));
    assert!(shelf.bindings.contains_key(&id("GetShelf")));
}

#[test]
fn the_internal_view_keeps_it_whole() {
    let service = full();
    let internal = project(&service, Audience::Internal).unwrap();
    assert_eq!(names(&internal.resource_roots), ["Bulletin", "Shelf"]);
    assert!(internal.permissions.contains_key(&id("Bulletin")));
    assert!(internal.permissions.contains_key(&id("Marginalia")));
    let bulletin = resource(&internal, "Bulletin");
    let get = &bulletin.bindings[&id("GetBulletin")];
    assert_eq!(get.input.identifiers["bulletinId"], ["bulletinId"]);
    assert_eq!(
        get.output.identifiers["bulletinId"],
        ["bulletin", "bulletinId"]
    );
    assert_eq!(get.output.properties["headline"], ["bulletin", "headline"]);
    // A listing binds per element, through the view its list holds.
    let list = &bulletin.bindings[&id("ListBulletins")];
    let elements = &list.output.collections["bulletins"];
    assert_eq!(elements.identifiers["bulletinId"], ["bulletinId"]);
    assert_eq!(elements.properties["headline"], ["headline"]);
    assert_eq!(names(&bulletin.collection_operations), ["ArchiveBulletins"]);
}

#[test]
fn a_doc_link_to_an_internal_operation_is_text_in_the_external_view() {
    let compiled = compiled_with(|text| {
        text.replace(
            "/// A shelf of books.",
            "/// A shelf of books; staff read [GetBulletin].",
        )
    });
    let service = compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    assert!(service.doc_links.contains_key("GetBulletin"));
    let external = project(&service, Audience::External).unwrap();
    assert!(!external.doc_links.contains_key("GetBulletin"));
}

/// `DescribeGrant` with one more output member, the package's other
/// declarations beside it.
fn describe_grant_with(member: &str, declarations: &str) -> Service {
    let compiled = compiled_with(|text| {
        let text = text.replace(
            "        target: GrantTarget\n",
            &format!("        target: GrantTarget\n        {member}\n"),
        );
        format!("{text}\n{declarations}")
    });
    compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()))
}

#[test]
fn an_external_operation_reaching_a_view_of_an_internal_resource() {
    // Its members are refused as any member of an internal type is.
    let service = describe_grant_with("latest?: BulletinView", "");
    let refused = project(&service, Audience::External)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("audience.internal_target"), "{refused}");
    // A view of public types is a shape like any other: the operation
    // publishes the shape, and the `for` it was declared with is not in it.
    let service = describe_grant_with(
        "digest?: Digest",
        "structure Digest for Bulletin {\n    $headline\n}\n",
    );
    let external = project(&service, Audience::External).unwrap();
    assert!(external.shape(&id("Digest")).is_some());
    assert!(!external.resources.contains_key(&id("Bulletin")));
}

#[test]
fn the_flag_survives_the_release_snapshot() {
    let service = full();
    let text = snapshot::write(&service, "test");
    let read = snapshot::read(&text).unwrap();
    assert!(resource(&read, "Bulletin").internal);
    assert!(resource(&read, "Marginalia").internal);
    assert!(!resource(&read, "Shelf").internal);
    assert_eq!(snapshot::write(&read, "test"), text);
    // A package's contract keeps it too: an SDK release plan projects it.
    let contract = snapshot::read(&snapshot::write_contract(&service, "test")).unwrap();
    assert!(resource(&contract, "Bulletin").internal);
    let projected = project(&contract, Audience::External).unwrap();
    assert!(!projected.resources.contains_key(&id("Bulletin")));
}

#[test]
fn a_snapshot_without_the_flag_reads_its_resources_as_public() {
    let text = snapshot::write(&all_public(), "test");
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let resources = value["service"]["resources"].as_object_mut().unwrap();
    assert!(!resources.is_empty());
    for resource in resources.values_mut() {
        assert!(
            resource
                .as_object_mut()
                .unwrap()
                .remove("internal")
                .is_some()
        );
    }
    let read = snapshot::read(&serde_json::to_string(&value).unwrap()).unwrap();
    assert!(read.resources.values().all(|r| !r.internal));
    assert_eq!(read.resources.len(), 3);
}

/// What a comparison found at error, as `rule id`.
fn errors(old: &Service, new: &Service) -> Vec<String> {
    diff(old, new, &Config::default())
        .findings
        .into_iter()
        .filter(|finding| finding.severity == Severity::Error)
        .map(|finding| format!("{} {}", finding.rule, finding.shape))
        .collect()
}

#[test]
fn an_internal_resource_becoming_public_is_additive() {
    let before = full();
    let after = all_public();
    // The guard compares the models themselves, which hold the same
    // operations, shapes and permissions either way.
    assert_eq!(errors(&before, &after), Vec::<String>::new());
    // An SDK release plan compares the views its audience sees, which only
    // gain. A public output enum that gains a value is its own finding, as
    // any added value is: a client may meet one it does not know.
    let external = |service: &Service| project(service, Audience::External).unwrap();
    assert_eq!(
        errors(&external(&before), &external(&after)),
        [
            "compatibility.enum example.staff#GrantTarget",
            "compatibility.sdk example.staff#GrantTarget",
        ]
    );
    // And the other way the plan sees what its clients lose.
    let lost = errors(&external(&after), &external(&before));
    assert!(
        lost.iter()
            .any(|f| f == "compatibility.sdk example.staff#GetBulletin"),
        "{lost:#?}"
    );
    assert!(
        lost.iter()
            .any(|f| f == "compatibility.sdk example.staff#Bulletin$read"),
        "{lost:#?}"
    );
}

/// The package frozen as release "1", with `edit` applied to its next
/// release "2", which migrates from it declaring nothing.
fn released(name: &str, edit: impl Fn(&str) -> String) -> (Package, Compiled) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("internal-resources-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    crucible_lang::release::freeze(&dir.join("releases"), &full(), "test").unwrap();
    let source = std::fs::read_to_string(models().join("internal-resource/library.cru"))
        .unwrap()
        .replace("\r\n", "\n");
    std::fs::write(
        dir.join("library.cru"),
        format!(
            "{}\nmigration \"2\" from \"1\" {{\n}}\n",
            edit(&source).replace("version \"1\"", "version \"2\"")
        ),
    )
    .unwrap();
    let manifest = std::fs::read_to_string(models().join("internal-resource/crucible.toml"))
        .unwrap()
        .replace("\r\n", "\n");
    std::fs::write(
        dir.join("crucible.toml"),
        format!("{manifest}\n[model.baseline]\npath = \"releases/1.json\"\n"),
    )
    .unwrap();
    let package = load_package(&dir).unwrap();
    let compiled = compile(&package, None);
    assert!(compiled.service.is_some(), "{}", compiled.render());
    (package, compiled)
}

#[test]
fn the_guard_finds_nothing_when_an_internal_resource_becomes_public() {
    let (package, compiled) = released("public", |text| text.replace("internal ", ""));
    let own = package.files.len();
    let checked = guard(&package, &compiled, own, &Config::default())
        .expect("a baseline")
        .expect("the baseline loads");
    assert!(!checked.has_errors(), "{}", checked.render(&compiled));
}

#[test]
fn an_older_release_shows_clients_no_internal_resource_either() {
    let (package, compiled) = released("older", str::to_owned);
    let own = package.files.len();
    let config = Config::default();
    let mut current = compiled.service.clone().unwrap();
    current.versions = crucible_lang::versions(&package, &compiled, own, &config).versions;
    assert_eq!(current.versions.len(), 1);
    // The release itself still holds it, for the server.
    assert!(
        current.versions[0]
            .service
            .resources
            .contains_key(&id("Bulletin"))
    );
    let external = project(&current, Audience::External).unwrap();
    let seen = visible(&external);
    assert_eq!(seen.len(), 1);
    for model in [&*seen[0].service, &*seen[0].release] {
        assert!(!model.resources.contains_key(&id("Bulletin")));
        assert!(!model.permissions.contains_key(&id("Marginalia")));
        assert!(model.operations.iter().all(|op| !op.internal));
    }
}

fn policy_package(policy: &str) -> Compiled {
    compile(
        &Package::new(
            "example.rules",
            vec![
                SourceFile {
                    path: "model.cru".to_owned(),
                    text: r#"
service Rules {
    version "1"
    protocol json
    resources [Open, Hidden]
}

/// Documented, as the rule asks of public resources.
resource Open {
    id openId: string
}

internal resource Hidden {
    id hiddenId: string
}
"#
                    .to_owned(),
                },
                SourceFile {
                    path: "policy.cru".to_owned(),
                    text: policy.to_owned(),
                },
            ],
        ),
        None,
    )
}

fn policy_errors(policy: &str) -> Vec<String> {
    policy_package(policy)
        .diagnostics
        .iter()
        .filter(|item| item.diagnostic.is_error())
        .map(|item| item.diagnostic.message.clone())
        .collect()
}

#[test]
fn a_rule_reads_the_flag_on_a_resource() {
    let holds = "rule PublicDocumented { every resource where not internal must documented }";
    assert_eq!(policy_errors(holds), Vec::<String>::new());
    let found = policy_errors("rule Documented { every resource where internal must documented }");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("Hidden"), "{found:#?}");
    let found = policy_errors("rule NoneInternal { no resource where internal }");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("Hidden"), "{found:#?}");
}

#[test]
fn a_rule_cannot_read_sensitive_on_a_resource() {
    let found = policy_errors("rule Quiet { no resource where sensitive }");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].contains("sensitive"), "{found:#?}");
}
