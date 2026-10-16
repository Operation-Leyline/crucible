//! Each older release as the current model sees it
//! (`crucible_ir::versions::projections`, docs/api-versions-clients.md 5b,
//! decisions 68-75), from the chains `crucible_lang::versions` builds.
//! crucible-ir cannot dev-depend on the compiler, so its projection is tested
//! here, from `.cru` packages.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crucible_ir::ir::{
    Condition, Member, Operation, Requirements, Service, Shape, SimpleKind, TimestampFormat,
    VersionStep,
};
use crucible_ir::versions::{Projection, projections, undo};
use crucible_ir::{Diagnostics, ShapeId};
use crucible_policy::Config;

fn dir(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn load(path: &str) -> (Service, Config) {
    let dir = dir(path);
    let service =
        crucible_lang::load_versioned_service(&dir).unwrap_or_else(|error| panic!("{error}"));
    let manifest = dir.join("crucible.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let config = if Config::declares_policy(&text) {
        Config::load(&manifest).unwrap()
    } else {
        Config::default()
    };
    (service, config)
}

fn projected(service: &Service) -> Vec<Projection> {
    projections(service).unwrap_or_else(|errors| panic!("{errors}"))
}

/// The projection of release `name`.
fn at<'p>(projections: &'p [Projection], name: &str) -> &'p Service {
    &projections
        .iter()
        .find(|projection| projection.name == name)
        .unwrap_or_else(|| panic!("no projection of {name}"))
        .service
}

fn id(namespace: &str, text: &str) -> ShapeId {
    format!("{namespace}#{text}").parse().unwrap()
}

fn operation<'s>(service: &'s Service, name: &str) -> &'s Operation {
    service
        .operations
        .iter()
        .find(|op| op.name() == name)
        .unwrap_or_else(|| panic!("{} has no operation {name}", service.version))
}

fn has_operation(service: &Service, name: &str) -> bool {
    service.operations.iter().any(|op| op.name() == name)
}

/// A structure's members as `name`, `name?` (optional) or `name=default`,
/// each with its target's name.
fn members(service: &Service, shape: &ShapeId) -> Vec<String> {
    let Some(Shape::Structure(s) | Shape::Union(s)) = service.shapes.get(shape) else {
        panic!("{} has no structure {shape}", service.version);
    };
    s.members
        .iter()
        .map(|m| {
            let mark = match (&m.default, m.required) {
                (Some(default), _) => format!("={default}"),
                (None, true) => String::new(),
                (None, false) => "?".to_owned(),
            };
            format!("{}{mark}: {}", m.name, m.target.name())
        })
        .collect()
}

fn member<'s>(service: &'s Service, shape: &ShapeId, name: &str) -> &'s Member {
    let Some(Shape::Structure(s)) = service.shapes.get(shape) else {
        panic!("no structure {shape}");
    };
    s.member(name)
        .unwrap_or_else(|| panic!("{shape} has no {name}"))
}

fn values(service: &Service, shape: &ShapeId) -> Vec<String> {
    let Some(Shape::Enum(e)) = service.shapes.get(shape) else {
        panic!("no enum {shape}");
    };
    e.values
        .iter()
        .map(|v| format!("{}={}", v.name, v.value))
        .collect()
}

fn errors(operation: &Operation) -> Vec<&str> {
    operation.errors.iter().map(ShapeId::name).collect()
}

fn examples(operation: &Operation) -> Vec<&str> {
    operation
        .examples
        .iter()
        .map(|e| e.title.as_str())
        .collect()
}

fn json(value: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap()
}

/// What `service`'s operations and errors reach.
fn reached_in(service: &Service) -> BTreeSet<ShapeId> {
    let mut reached = BTreeSet::new();
    let mut pending: Vec<ShapeId> = service
        .operations
        .iter()
        .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
        .chain(&service.errors)
        .cloned()
        .collect();
    while let Some(id) = pending.pop() {
        if let Some(shape) = service.shapes.get(&id)
            && reached.insert(id)
        {
            pending.extend(shape.members().iter().map(|m| m.target.clone()));
        }
    }
    reached
}

/// What the guard's comparison finds between each frozen release and its
/// projection, both families at error with the manifest's exceptions
/// (decision 68): nothing, when undo has no gap.
fn findings(service: &Service, config: &Config) -> Vec<String> {
    let config = Config {
        exceptions: config.exceptions.clone(),
        ..Config::default()
    };
    let mut out = Vec::new();
    for (version, projection) in service.versions.iter().zip(projected(service)) {
        let report = crucible_policy::diff(&version.service, &projection.service, &config);
        for finding in report.findings {
            if finding.exception.is_none() {
                out.push(format!(
                    "{}: {} {}: {}",
                    version.name, finding.rule, finding.shape, finding.message
                ));
            }
        }
    }
    out
}

const PACKAGES: &[&str] = &[
    "tests/models/version-steps",
    "tests/models/version-history",
    "tests/models/version-renamed-enum-value",
    "tests/models/version-retired-operation",
    "tests/models/version-projection",
    "tests/models/version-projection-auth",
    "tests/models/version-projection-retype",
    "tests/models/version-projection-derived",
    "tests/models/version-projection-conditions",
    "tests/models/version-projection-payload",
    "e2e/weather-service/versioned",
    "e2e/weather-service/versioned_pages",
    "e2e/weather-service/versioned_permissions",
];

/// The packages whose projections are their frozen releases whole, apart
/// from documentation: nothing was added since that reaches them.
const UNCHANGED: &[&str] = &[
    "tests/models/version-steps",
    "tests/models/version-renamed-enum-value",
    "tests/models/version-projection-retype",
    "tests/models/version-projection-conditions",
    "tests/models/version-projection-payload",
];

/// The IR as JSON without its documentation and doc links: a projection
/// keeps the current model's documentation for what it did not restore.
fn without_docs(service: &Service) -> serde_json::Value {
    fn strip(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("docs");
                map.remove("doc_links");
                map.remove("doc_link_operations");
                map.values_mut().for_each(strip);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut value = json(service);
    strip(&mut value);
    value
}

/// Every versioned package's projections pass the IR's own checks
/// (`projections` refuses one that does not) and compare clean with their
/// frozen releases: no undo gap, whatever was added since.
#[test]
fn every_projection_compares_clean_with_its_frozen_release() {
    for package in PACKAGES {
        let (service, config) = load(package);
        assert!(!service.versions.is_empty(), "{package} has no versions");
        assert_eq!(
            findings(&service, &config),
            Vec::<String>::new(),
            "{package}"
        );
    }
}

/// The comparison is not vacuous: the frozen release against the current
/// model, unprojected, is the break the migration declares.
#[test]
fn the_comparison_finds_what_undo_removes() {
    let (service, config) = load("tests/models/version-steps");
    let config = Config {
        exceptions: config.exceptions,
        ..Config::default()
    };
    let report = crucible_policy::diff(&service.versions[0].service, &service, &config);
    let rules: BTreeSet<&str> = report.findings.iter().map(|f| f.rule.as_str()).collect();
    for rule in [
        "compatibility.operation",
        "compatibility.http",
        "compatibility.error",
        "compatibility.enum",
        "compatibility.required",
        "compatibility.member",
    ] {
        assert!(rules.contains(rule), "{rule} in {rules:?}");
    }
}

/// What lowering derived is recomputed: the shapes are what the operations
/// and errors reach, every target resolving, and what the current model
/// holds that nothing reaches; the recursion groups are those of the shapes,
/// every feature use names something there, the operations are sorted, and
/// a projection carries no versions of its own.
#[test]
fn derived_ir_is_recomputed_for_every_projection() {
    for package in PACKAGES {
        let (service, _) = load(package);
        for projection in projected(&service) {
            let model = &projection.service;
            let what = format!("{package} at {}", projection.name);
            assert_eq!(model.version, projection.name, "{what}");
            assert_eq!(model.api_version.as_deref(), Some(projection.name.as_str()));
            assert!(model.versions.is_empty(), "{what}");
            let mut reached = BTreeSet::new();
            let mut pending: Vec<ShapeId> = model
                .operations
                .iter()
                .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
                .chain(&model.errors)
                .cloned()
                .collect();
            while let Some(id) = pending.pop() {
                let shape = model
                    .shapes
                    .get(&id)
                    .unwrap_or_else(|| panic!("{what}: {id} does not resolve"));
                if reached.insert(id) {
                    pending.extend(shape.members().iter().map(|m| m.target.clone()));
                }
            }
            let shapes: BTreeSet<ShapeId> = model.shapes.keys().cloned().collect();
            let unreached: BTreeSet<ShapeId> = shapes.difference(&reached).cloned().collect();
            let current = reached_in(&service);
            for id in &unreached {
                assert!(
                    service.shapes.contains_key(id) && !current.contains(id),
                    "{what}: {id} is reached by nothing, here or in the current model"
                );
            }
            assert_eq!(
                model.recursive_groups,
                crucible_ir::ir::recursive_groups(&model.shapes),
                "{what}"
            );
            for usage in &model.requirements {
                let root = usage.shape.root();
                assert!(
                    model.shapes.contains_key(&root)
                        || model.operations.iter().any(|op| op.id == root),
                    "{what}: {usage:?}"
                );
            }
            let names: Vec<&str> = model.operations.iter().map(Operation::name).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            assert_eq!(names, sorted, "{what}");
        }
    }
}

/// `tests/models/version-steps`: every kind of step, undone, read member by
/// member; `an_unchanged_release_projects_to_its_frozen_self` compares the
/// whole projection with the frozen release.
#[test]
fn every_step_kind_is_undone() {
    let (service, _) = load("tests/models/version-steps");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let shape = |text: &str| id("example.steps", text);
    // rename, require (by default and by hook), add enum value, retype (by
    // hook and by conversion, the first renamed too), reshape.
    assert_eq!(
        members(v1, &shape("GetItemOutput")),
        ["name: String", "kind: Kind", "size: Size"]
    );
    assert_eq!(
        members(v1, &shape("ListItemsInput")),
        ["max?: Integer", "sort?: String"]
    );
    assert_eq!(values(v1, &shape("Kind")), ["SMALL=SMALL", "LARGE=LARGE"]);
    assert_eq!(
        members(v1, &shape("PutItemInput")),
        ["id: String", "count?: Integer"]
    );
    assert_eq!(members(v1, &shape("PutItemOutput")), ["total: Integer"]);
    assert_eq!(
        members(v1, &shape("Size")),
        ["width: Integer", "height: Integer"]
    );
    // add error, move, remove operation.
    let get = operation(v1, "GetItem");
    assert_eq!(errors(get), ["ItemNotFound"]);
    assert_eq!(
        (get.method.as_str(), get.uri.to_string()),
        ("GET", "/items/{id}".to_owned())
    );
    assert!(!v1.shapes.contains_key(&shape("ItemGone")));
    let count = operation(v1, "CountItems");
    assert_eq!(count.uri.to_string(), "/count");
    assert_eq!(members(v1, &shape("CountItemsOutput")), ["count: Integer"]);
    // The renamed query member binds as the release bound it.
    let list = operation(v1, "ListItems");
    let query: Vec<(&str, &str)> = list
        .request
        .query
        .iter()
        .map(|b| (b.member.as_str(), b.name.as_str()))
        .collect();
    assert_eq!(query, [("max", "max"), ("sort", "sort")]);
}

/// Where nothing added since reaches an older release, its projection is
/// the frozen release, whole: every derived field, header and feature use,
/// documentation and doc links aside.
#[test]
fn an_unchanged_release_projects_to_its_frozen_self() {
    for package in UNCHANGED {
        let (service, _) = load(package);
        for (version, projection) in service.versions.iter().zip(projected(&service)) {
            assert_eq!(
                without_docs(&projection.service),
                without_docs(&version.service),
                "{package} at {}",
                version.name
            );
        }
    }
}

/// Every step undo applies makes a difference the guard's comparison sees:
/// undoing the migration without it leaves a finding against the frozen
/// release. `behavior` changes no shape, so it has nothing to undo.
#[test]
fn every_step_is_needed() {
    for package in PACKAGES {
        let (service, config) = load(package);
        let config = Config {
            exceptions: config.exceptions,
            ..Config::default()
        };
        let projections = projected(&service);
        for (index, version) in service.versions.iter().enumerate() {
            let newer = projections
                .get(index + 1)
                .map_or(&service, |projection| &projection.service);
            for (at, step) in version.steps.iter().enumerate() {
                if matches!(step, VersionStep::Behavior { .. }) {
                    continue;
                }
                let mut without = version.clone();
                without.steps.remove(at);
                let mut errors = Diagnostics::default();
                let mut warnings = Diagnostics::default();
                let projected = undo(newer, &without, &mut errors, &mut warnings);
                let report = crucible_policy::diff(&version.service, &projected, &config);
                let found = report.findings.iter().any(|f| f.exception.is_none());
                assert!(
                    found,
                    "{package} at {}: undo without {step:?} still compares clean",
                    version.name
                );
            }
        }
    }
}

/// A shape the current model holds that nothing reaches, as lowering may
/// leave one, stays in every projection; one only undo copied, which
/// nothing reaches, does not.
#[test]
fn an_unreached_shape_of_the_current_model_stays() {
    let (mut service, _) = load("tests/models/version-projection");
    let orphan = id(SHOP, "Orphan");
    let tag = service.shapes[&id(SHOP, "Tag")].clone();
    service.shapes.insert(orphan.clone(), tag);
    for projection in projected(&service) {
        assert!(
            projection.service.shapes.contains_key(&orphan),
            "{}",
            projection.name
        );
    }
}

/// An enum value renamed in a migration is its older self again, wire value
/// included.
#[test]
fn a_renamed_enum_value_is_its_older_self() {
    let (service, _) = load("tests/models/version-renamed-enum-value");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let frozen = &service.versions[0].service;
    let kind = v1
        .shapes
        .keys()
        .find(|id| id.name() == "Kind")
        .cloned()
        .unwrap();
    assert_eq!(values(v1, &kind), values(frozen, &kind));
    assert_ne!(values(&service, &kind), values(frozen, &kind));
}

const SHOP: &str = "example.projection";

/// `require S.notes` written before `rename S.description -> notes`: the
/// steps compose, so the member is the release's optional `description`.
#[test]
fn steps_compose_whatever_their_order() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    let input = id(SHOP, "PutItemInput");
    assert_eq!(
        members(at(&projections, "v1"), &input),
        ["id: String", "description?: String"]
    );
    assert_eq!(
        operation(at(&projections, "v1"), "PutItem").request.body,
        ["description"]
    );
    assert_eq!(
        members(at(&projections, "v2"), &input),
        ["id: String", "notes: String"]
    );
}

/// A renamed pagination token or page size is the release's member again,
/// and so are the pagination paths and query bindings through it.
#[test]
fn renamed_pagination_members_page_as_the_release_did() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let list = operation(v1, "ListItems");
    let frozen = operation(&service.versions[0].service, "ListItems");
    assert_eq!(list.paginated, frozen.paginated);
    let paginated = list.paginated.as_ref().unwrap();
    assert_eq!(
        (
            paginated.input_token.as_str(),
            paginated.output_token.as_str(),
            paginated.page_size.as_deref()
        ),
        ("token", "next", Some("max"))
    );
    assert_eq!(json(&list.request), json(&frozen.request));
    assert_eq!(json(&list.response), json(&frozen.response));
    assert_eq!(
        members(v1, &id(SHOP, "ListItemsInput")),
        ["token?: String", "max?: Integer"]
    );
    let current = operation(&service, "ListItems").paginated.as_ref().unwrap();
    assert_eq!(current.input_token, "cursor");
}

/// Additions reach every older release: an operation, members appended to
/// a structure, a member added to a structure a restored operation reaches.
/// A renamed member keeps the current documentation.
#[test]
fn additions_reach_older_releases() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    let output = id(SHOP, "GetItemOutput");
    let v1 = at(&projections, "v1");
    let v2 = at(&projections, "v2");
    assert_eq!(
        members(v1, &output),
        [
            "name: String",
            "kind: Kind",
            "size: Size",
            "tag?: Tag",
            "note?: String"
        ]
    );
    assert_eq!(
        member(v1, &output, "name").docs.as_deref(),
        Some("The item's title.")
    );
    assert_eq!(
        members(v2, &output),
        [
            "title: String",
            "kind: Kind",
            "size: Size",
            "tag?: Tag",
            "note?: String"
        ]
    );
    for release in [v1, v2] {
        assert!(has_operation(release, "ArchiveItem"), "{}", release.version);
        assert_eq!(
            members(release, &id(SHOP, "Tag")),
            ["name: String", "color?: String"]
        );
    }
    // Undone at v2, so at v1 too: the route, the error and the value.
    for release in [v1, v2] {
        let get = operation(release, "GetItem");
        assert_eq!(get.uri.to_string(), "/items/{id}");
        assert_eq!(errors(get), ["ItemNotFound"]);
        assert!(!release.shapes.contains_key(&id(SHOP, "ItemGone")));
        assert_eq!(
            values(release, &id(SHOP, "Kind")),
            ["SMALL=SMALL", "LARGE=LARGE"]
        );
    }
}

/// Decision 74: a member appended to a structure after a `reshape` of it is
/// not in the releases before the reshape, whose definition is the frozen
/// one; it is in the release the reshape serves.
#[test]
fn additions_do_not_reach_inside_a_reshape() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    let size = id(SHOP, "Size");
    assert_eq!(
        members(at(&projections, "v1"), &size),
        ["width: Integer", "height: Integer"]
    );
    assert_eq!(
        member(at(&projections, "v1"), &size, "width")
            .docs
            .as_deref(),
        Some("How wide it is.")
    );
    assert_eq!(
        members(at(&projections, "v2"), &size),
        ["area: Integer", "unit?: String"]
    );
}

/// `remove operation` restores the release's operation, with its own input
/// and output, the error only it used, its route and documentation, and
/// its targets as the projection has them: `Tag` with the member added
/// since. Its feature use is the release's; the current model's stay.
#[test]
fn a_removed_operation_is_restored() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let count = operation(v1, "CountItems");
    assert_eq!(
        (count.method.as_str(), count.uri.to_string()),
        ("GET", "/count".to_owned())
    );
    assert_eq!(errors(count), ["CountUnavailable"]);
    assert_eq!(
        members(v1, &id(SHOP, "CountItemsOutput")),
        ["count: BigInteger", "tag: Tag"]
    );
    assert_eq!(
        members(v1, &id(SHOP, "CountUnavailable")),
        ["message: String"]
    );
    let frozen = &service.versions[0].service;
    assert_eq!(count.docs, operation(frozen, "CountItems").docs);
    let features: Vec<String> = v1
        .requirements
        .iter()
        .map(|usage| format!("{} {}", usage.feature, usage.shape))
        .collect();
    assert_eq!(
        features,
        [
            "big-integer crucible.api#BigInteger",
            "big-decimal crucible.api#BigDecimal"
        ]
    );
    let v2 = at(&projections, "v2");
    assert!(!has_operation(v2, "CountItems"));
    assert!(!v2.shapes.contains_key(&id(SHOP, "CountUnavailable")));
    assert!(
        v2.requirements
            .iter()
            .all(|usage| usage.shape.name() != "BigInteger")
    );
}

/// An operation whose shapes undo changed takes its release's examples; one
/// added since keeps those that hold at the release, and each one left out
/// is a warning, once: at the newest release it does not hold at, whose
/// projection the older ones start from.
#[test]
fn examples_are_the_releases_or_hold_at_it() {
    let (service, _) = load("tests/models/version-projection");
    let projections = projected(&service);
    for (index, name) in [(0, "v1"), (1, "v2")] {
        let release = at(&projections, name);
        let get = operation(release, "GetItem");
        let frozen = operation(&service.versions[index].service, "GetItem");
        assert_eq!(get.examples, frozen.examples, "{name}");
        assert_ne!(get.examples, operation(&service, "GetItem").examples);
        assert_eq!(
            examples(operation(release, "ArchiveItem")),
            ["Archive an item"]
        );
    }
    let warnings = |name: &str| -> Vec<String> {
        let projection = projections.iter().find(|p| p.name == name).unwrap();
        projection
            .warnings
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    assert_eq!(
        warnings("v2"),
        [
            "example.projection#ArchiveItem: example \"Archive a huge item\" does not hold at release \"v2\" and is left out of it: input.kind: \"HUGE\" is not a value of example.projection#Kind"
        ]
    );
    assert_eq!(warnings("v1"), Vec::<String>::new());
}

/// Decision 73: an operation the release had authenticates as the release
/// declared it, though the current model made it public; one added since
/// as the current model declares it, bearer or public.
#[test]
fn operations_keep_their_releases_authentication() {
    let (service, _) = load("tests/models/version-projection-auth");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let auth = |name: &str| {
        let op = operation(v1, name);
        (
            op.auth
                .iter()
                .map(|id| id.name().to_owned())
                .collect::<Vec<_>>(),
            op.public,
        )
    };
    let bearer = (vec!["httpBearerAuth".to_owned()], false);
    assert_eq!(auth("GetTicket"), bearer);
    assert_eq!(auth("ListTickets"), bearer);
    assert_eq!(auth("Ping"), bearer);
    assert_eq!(auth("Health"), (Vec::new(), true));
    let current = operation(&service, "GetTicket");
    assert!(current.public && current.auth.is_empty());
    let schemes: Vec<&str> = v1.auth_schemes.keys().map(ShapeId::name).collect();
    assert_eq!(schemes, ["httpBearerAuth"]);
}

/// A `retype` restores the release's member whole: a constrained string
/// retyped to a boolean is the string again, with its length, pattern and
/// documentation, and a timestamp retyped to a string keeps its format.
#[test]
fn a_retype_restores_the_whole_member() {
    let (service, _) = load("tests/models/version-projection-retype");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let code = member(v1, &id("example.retype", "GetItemOutput"), "code");
    assert!(matches!(
        v1.shapes.get(&code.target),
        Some(Shape::Simple(simple)) if simple.kind == SimpleKind::String
    ));
    let length = code.constraints.length.as_ref().unwrap();
    assert_eq!((length.min, length.max), (Some(1), Some(8)));
    assert_eq!(code.constraints.pattern.as_deref(), Some("^[a-z]+$"));
    assert_eq!(code.docs.as_deref(), Some("The item's code."));
    let since = member(v1, &id("example.retype", "GetItemInput"), "since");
    assert!(matches!(
        v1.shapes.get(&since.target),
        Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Timestamp
    ));
    assert!(!since.required);
    assert_eq!(since.timestamp_format, Some(TimestampFormat::EpochSeconds));
    assert_eq!(since.wire_timestamp_format, TimestampFormat::EpochSeconds);
    // The current model's members are what the current model says.
    let now = member(&service, &id("example.retype", "GetItemOutput"), "code");
    assert_eq!(now.target.name(), "Boolean");
    assert_eq!(now.constraints, Default::default());
}

/// A restored operation keeps its release's requirements, naming today's
/// permissions; a converted one has the current model's.
#[test]
fn a_restored_operation_requires_todays_permissions() {
    let (service, _) = load("e2e/weather-service/versioned_permissions");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let count = operation(v1, "CountNotes");
    let Requirements::List(clauses) = &count.requirements else {
        panic!("{:?}", count.requirements);
    };
    let shelf = service
        .permissions
        .keys()
        .find(|id| id.name() == "Shelf")
        .unwrap();
    assert_eq!(clauses.len(), 1);
    assert_eq!(&clauses[0].resource, shelf);
    assert!(
        service.permissions[shelf]
            .permission(&clauses[0].permission)
            .is_some()
    );
    assert_eq!(
        operation(v1, "GetNote").requirements,
        operation(&service, "GetNote").requirements
    );
    assert_eq!(v1.permissions, service.permissions);
}

/// A step whose newer side the newer model no longer has, which only an
/// edited IR can hold once the compiler refuses it: every problem of every
/// release is reported, each naming its release.
#[test]
fn a_step_whose_subject_is_gone_is_an_error() {
    let (mut service, _) = load("tests/models/version-steps");
    let output = id("example.steps", "GetItemOutput");
    if let Some(Shape::Structure(s)) = service.shapes.get_mut(&output) {
        s.members.retain(|m| m.name != "title");
    }
    let input = id("example.steps", "PutItemInput");
    if let Some(Shape::Structure(s)) = service.shapes.get_mut(&input) {
        s.members.retain(|m| m.name != "quantity");
    }
    let errors = projections(&service).unwrap_err().to_string();
    for expected in [
        "example.steps#GetItemOutput$title: release \"v1\" cannot be projected: `rename GetItemOutput.name -> title` names a member the newer model no longer has",
        "example.steps#PutItemInput$quantity: release \"v1\" cannot be projected: `retype PutItemInput.quantity` names a member the newer model no longer has",
    ] {
        assert!(errors.contains(expected), "{expected}\nin\n{errors}");
    }
}

const DERIVED: &str = "example.derived";

/// A moved operation's route is its release's, and so is the greedy-label
/// feature use lowering derived from it.
#[test]
fn a_restored_route_brings_its_feature_use() {
    let (service, _) = load("tests/models/version-projection-derived");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    assert_eq!(operation(v1, "GetFile").uri.to_string(), "/files/{key+}");
    let greedy = |model: &Service| {
        model
            .requirements
            .iter()
            .filter(|usage| usage.feature == crucible_ir::Feature::GreedyLabels)
            .map(|usage| usage.shape.name().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(greedy(v1), ["GetFile"]);
    assert_eq!(greedy(&service), Vec::<String>::new());
}

/// The protocol's fixed `Content-Type` follows the bindings undo restored:
/// a request or response the release sent with a body has it, one it sent
/// without has none, whatever the current model does.
#[test]
fn fixed_headers_follow_the_restored_bindings() {
    let (service, _) = load("tests/models/version-projection-derived");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let json_type = || {
        std::collections::BTreeMap::from([(
            "Content-Type".to_owned(),
            "application/json".to_owned(),
        )])
    };
    let none = std::collections::BTreeMap::new;
    let request = |model: &Service, name: &str| operation(model, name).protocol_headers.clone();
    let response =
        |model: &Service, name: &str| operation(model, name).response_protocol_headers.clone();
    // A query then, a body now; a body then, a query now.
    assert_eq!(request(v1, "PutQuery"), none());
    assert_eq!(request(&service, "PutQuery"), json_type());
    assert_eq!(request(v1, "PutBody"), json_type());
    assert_eq!(request(&service, "PutBody"), none());
    // A header then, a body now; a body then, a header now.
    assert_eq!(response(v1, "ReadHeader"), none());
    assert_eq!(response(&service, "ReadHeader"), json_type());
    assert_eq!(response(v1, "ReadBody"), json_type());
    assert_eq!(response(&service, "ReadBody"), none());
    let frozen = &service.versions[0].service;
    for name in ["PutQuery", "PutBody", "ReadHeader", "ReadBody"] {
        assert_eq!(request(v1, name), request(frozen, name), "{name}");
        assert_eq!(response(v1, name), response(frozen, name), "{name}");
    }
}

/// The release's example of an operation it had is left out, with a
/// warning, when a member required since makes it fail at the release.
#[test]
fn a_frozen_example_that_no_longer_holds_is_left_out() {
    let (service, _) = load("tests/models/version-projection-derived");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    assert_eq!(examples(operation(v1, "GetItem")), Vec::<&str>::new());
    assert_eq!(
        examples(operation(&service.versions[0].service, "GetItem")),
        ["Read an item"]
    );
    let warnings: Vec<String> = projections[0]
        .warnings
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].starts_with(
            "example.derived#GetItem: example \"Read an item\" does not hold at release \"v1\""
        ),
        "{warnings:?}"
    );
}

/// A member added since that defaults to a value undo removes defaults to
/// the value's downgrade, as the release's clients receive it.
#[test]
fn a_default_on_a_removed_value_is_its_downgrade() {
    let (service, _) = load("tests/models/version-projection-derived");
    let projections = projected(&service);
    let grade = |model: &Service| {
        let member = member(model, &id(DERIVED, "RateItemInput"), "grade");
        (member.default.clone(), member.wire_default.clone())
    };
    let large = Some(serde_json::Value::String("LARGE".into()));
    assert_eq!(grade(at(&projections, "v1")), (large.clone(), large));
    let huge = Some(serde_json::Value::String("HUGE".into()));
    assert_eq!(grade(&service), (huge.clone(), huge));
}

/// A permission's `when` condition reads the renamed member by the name the
/// release's clients send.
#[test]
fn a_condition_reads_a_renamed_member_by_its_older_name() {
    let (service, _) = load("tests/models/version-projection-conditions");
    let projections = projected(&service);
    let condition = |model: &Service| {
        let Requirements::List(clauses) = &operation(model, "PaintShelf").requirements else {
            panic!("no requirements");
        };
        clauses[0].when.clone()
    };
    let equals = |name: &str| {
        Some(Condition::Equals(
            vec![name.to_owned()],
            serde_json::Value::String("red".into()),
        ))
    };
    assert_eq!(condition(at(&projections, "v1")), equals("colour"));
    assert_eq!(condition(&service), equals("color"));
}

/// A doc link whose newer target undo removed (a definition restored under
/// another id) links to what the release linked it to.
#[test]
fn a_doc_link_to_what_undo_removed_is_the_releases() {
    let (mut service, _) = load("tests/models/version-projection");
    let label = "Size.area".to_owned();
    service
        .doc_links
        .insert(label.clone(), id(SHOP, "Size$area"));
    let frozen = &mut service.versions[0].service;
    frozen
        .doc_links
        .insert(label.clone(), id(SHOP, "Size$width"));
    let projections = projected(&service);
    assert_eq!(
        at(&projections, "v1").doc_links.get(&label),
        Some(&id(SHOP, "Size$width"))
    );
    assert_eq!(
        at(&projections, "v2").doc_links.get(&label),
        Some(&id(SHOP, "Size$area"))
    );
}

/// A rename whose older name the release does not have, which only an
/// edited IR can hold, is a problem, not a silent rename.
#[test]
fn a_rename_the_release_cannot_have_made_is_an_error() {
    let (mut service, _) = load("tests/models/version-steps");
    let output = id("example.steps", "GetItemOutput");
    if let Some(Shape::Structure(s)) = service.versions[0].service.shapes.get_mut(&output) {
        s.members.retain(|m| m.name != "name");
    }
    let errors = projections(&service).unwrap_err().to_string();
    assert!(
        errors.contains(
            "example.steps#GetItemOutput$name: release \"v1\" cannot be projected: `rename GetItemOutput.name -> title` names a member the release does not have"
        ),
        "{errors}"
    );
}

/// A restored operation rejoins its resource where the release listed it.
#[test]
fn a_restored_operation_rejoins_its_resource_in_place() {
    let (mut service, _) = load("e2e/weather-service/versioned_permissions");
    let shelf = service
        .resources
        .keys()
        .find(|id| id.name() == "Shelf")
        .cloned()
        .unwrap();
    let get = operation(&service, "GetNote").id.clone();
    service.resources.get_mut(&shelf).unwrap().operations = vec![get.clone()];
    let mut older = service.resources[&shelf].clone();
    let count = service.versions[0]
        .service
        .operations
        .iter()
        .find(|op| op.name() == "CountNotes")
        .unwrap()
        .id
        .clone();
    older.operations = vec![count.clone(), get.clone()];
    service.versions[0]
        .service
        .resources
        .insert(shelf.clone(), older);
    let projections = projected(&service);
    assert_eq!(
        at(&projections, "v1").resources[&shelf].operations,
        [count, get]
    );
}

/// A retyped payload member binds as the release bound it: its kind and
/// media type, and the `Content-Type` that follows, in a request and a
/// response.
#[test]
fn a_retyped_payload_binds_as_the_release_did() {
    let (service, _) = load("tests/models/version-projection-payload");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let frozen = &service.versions[0].service;
    let payload = |model: &Service, name: &str, request: bool| {
        let op = operation(model, name);
        let bindings = if request { &op.request } else { &op.response };
        let payload = bindings.payload.as_ref().unwrap();
        (
            format!("{:?}", payload.kind),
            payload.member.clone(),
            payload.media_type.clone(),
        )
    };
    assert_eq!(
        payload(v1, "PutFile", true),
        ("Blob".to_owned(), "body".to_owned(), "image/png".to_owned())
    );
    assert_eq!(payload(&service, "PutFile", true).0, "String");
    assert_eq!(
        payload(v1, "GetFile", false),
        (
            "String".to_owned(),
            "data".to_owned(),
            "text/plain".to_owned()
        )
    );
    assert_eq!(payload(&service, "GetFile", false).2, "application/pdf");
    for (name, request) in [("PutFile", true), ("GetFile", false)] {
        assert_eq!(payload(v1, name, request), payload(frozen, name, request));
    }
    assert_eq!(
        operation(v1, "PutFile")
            .protocol_headers
            .get("Content-Type"),
        Some(&"image/png".to_owned())
    );
    assert_eq!(
        operation(v1, "GetFile")
            .response_protocol_headers
            .get("Content-Type"),
        Some(&"text/plain".to_owned())
    );
    assert_eq!(without_docs(v1), without_docs(frozen));
}

/// A step on a structure a reshape restored whole is part of that
/// definition, whichever id the newer model gives the structure: here the
/// newer model calls version-steps' `Size` `Area`, and a requirement names
/// a member only the newer definition has.
#[test]
fn a_step_inside_a_reshape_under_a_new_id_is_skipped() {
    let (mut service, _) = load("tests/models/version-steps");
    let size = id("example.steps", "Size");
    let area = id("example.steps", "Area");
    // Only the current model renames it: the release keeps `Size`.
    let versions = std::mem::take(&mut service.versions);
    service.rename_shape(&size, "Area");
    service.versions = versions;
    let steps = &mut service.versions[0].steps;
    for step in steps.iter_mut() {
        if let VersionStep::Hooked {
            hook: crucible_ir::ir::Hook::Reshape { new, .. },
            ..
        } = step
        {
            *new = area.clone();
        }
    }
    steps.push(VersionStep::Require {
        old: size.clone(),
        new: area.clone(),
        member: "area".into(),
        from: String::new(),
        default: serde_json::Value::from(0),
        docs: None,
    });
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    assert_eq!(members(v1, &size), ["width: Integer", "height: Integer"]);
    assert!(!v1.shapes.contains_key(&area));
}

/// A member the release sent in the JSON body, which the newer model makes
/// the `@payload` while retyping it, is a body member at the release
/// again: no payload binding, and the release's `Content-Type`, whatever
/// media type either target declares.
#[test]
fn a_member_retyped_into_a_payload_is_the_releases_body_member() {
    let (service, _) = load("tests/models/version-projection-payload");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let report = operation(v1, "PutReport");
    assert_eq!(report.request.payload, None);
    assert_eq!(report.request.body, ["report"]);
    assert_eq!(
        report.protocol_headers.get("Content-Type"),
        Some(&"application/json".to_owned())
    );
    let now = operation(&service, "PutReport");
    assert_eq!(
        now.request.payload.as_ref().map(|p| p.media_type.as_str()),
        Some("application/vnd.ms-excel")
    );
    let frozen = operation(&service.versions[0].service, "PutReport");
    assert_eq!(json(&report.request), json(&frozen.request));
}

/// Decision 74: a member the release sent as the payload, which the newer
/// model sends in the JSON body beside a member added since, is the payload
/// again, and the added member does not reach the release: a request whose
/// body is the payload has nowhere to carry it. The projection is the
/// frozen release and passes the IR's checks.
#[test]
fn additions_do_not_reach_a_payload_bound_message() {
    let (service, _) = load("tests/models/version-projection-payload");
    let projections = projected(&service);
    let v1 = at(&projections, "v1");
    let note = operation(v1, "PutNote");
    assert_eq!(
        note.request.payload.as_ref().map(|p| p.member.as_str()),
        Some("body")
    );
    assert!(note.request.body.is_empty());
    let input = id("example.payload", "PutNoteInput");
    assert_eq!(members(v1, &input), ["id: String", "body: Image"]);
    assert_eq!(
        members(&service, &input),
        ["id: String", "body: String", "title?: String"]
    );
    let frozen = operation(&service.versions[0].service, "PutNote");
    assert_eq!(json(&note.request), json(&frozen.request));
    let mut problems = Diagnostics::default();
    crucible_ir::validate::service(v1, &mut problems);
    assert!(problems.is_empty(), "{problems}");
}
