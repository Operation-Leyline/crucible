//! The `crucible.auth` permission traits (docs/permissions.md §4.11),
//! lowered to the IR the `.cru` front end produces, and checked by the same
//! `validate::permissions`.

use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{Condition, Mode, Requirements, Service, Tier};
use crucible_smithy::validate::{FindingSeverity, PermissionFacts};
use crucible_smithy::{Audience, Model, ShapeId};

const ACCEPTED: &str = include_str!("fixtures/permissions.json");
const REFUSED: &str = include_str!("fixtures/permissions-refused.json");

fn lower(json: &str) -> Result<Service, crucible_smithy::Error> {
    Model::from_json(json).unwrap().lower(None)
}

fn service() -> Service {
    lower(ACCEPTED).unwrap_or_else(|e| panic!("{e}"))
}

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.leyline", name)
}

fn member(path: &str) -> Binding {
    Binding::Member(path.split('.').map(str::to_owned).collect())
}

fn requirements<'s>(service: &'s Service, name: &str) -> &'s Requirements {
    &service
        .operations
        .iter()
        .find(|op| op.id.name() == name)
        .unwrap()
        .requirements
}

#[test]
fn permissions_of_enums_lower_to_their_resources() {
    let service = service();
    let names: Vec<&str> = service.permissions.keys().map(ShapeId::name).collect();
    assert_eq!(names, ["Graph", "History", "Organization"]);
    let org = &service.permissions[&id("Organization")];
    assert_eq!(org.name, "org");
    assert!(org.name_declared);
    assert_eq!(org.identifiers, ["organizationId"]);
    assert_eq!(org.permission("READ").unwrap().tier, Tier::Internal);
    assert_eq!(org.permission("ADMIN").unwrap().tier, Tier::NotGrantable);
    assert_eq!(
        org.permission("ADMIN").unwrap().docs.as_deref(),
        Some("Everything in the organization.")
    );
    let graph = &service.permissions[&id("Graph")];
    assert_eq!(graph.name, "graph");
    assert!(!graph.name_declared);
    assert_eq!(graph.identifiers, ["organizationId", "graphId"]);
    let values: Vec<(&str, &str)> = graph
        .permissions
        .iter()
        .map(|p| (p.symbol.as_str(), p.value.as_str()))
        .collect();
    // Source order, the alias included.
    assert_eq!(
        values,
        [
            ("READ", "graph:read"),
            ("WRITE", "graph:write"),
            ("AUDIT", "graph:audit"),
            ("VIEW", "graph:view"),
        ]
    );
    let view = graph.permission("VIEW").unwrap();
    assert_eq!(view.alias_of.as_deref(), Some("READ"));
    assert!(view.notes.deprecated.is_some());
    assert_eq!(
        graph.permission("AUDIT").unwrap().tier,
        Tier::PermissionOnly
    );
    let history = &service.permissions[&id("History")];
    assert_eq!(
        history.identifiers,
        ["organizationId", "graphId", "historyId"]
    );
    let types = service.permission_types.as_ref().unwrap();
    assert_eq!(types.shape, id("ResourceKind"));
    assert_eq!(types.excluded, [id("History")]);
    assert!(service.shape(&id("ResourceKind")).is_some());
}

#[test]
fn requirements_name_the_symbol_and_bind_every_identifier() {
    let service = service();
    let Requirements::List(get) = requirements(&service, "GetOrganization") else {
        panic!()
    };
    assert_eq!(get[0].permission, "READ");
    assert_eq!(get[0].bindings, [member("organizationId")]);
    // `$label` binds the member of that URI label.
    let Requirements::List(get) = requirements(&service, "GetGraph") else {
        panic!()
    };
    assert_eq!(get[0].bindings, [member("organizationId"), member("graph")]);
    let Requirements::List(create) = requirements(&service, "CreateGraph") else {
        panic!()
    };
    assert_eq!(create[0].permission, "WRITE");
    assert_eq!(create[0].bindings, [member("organizationId"), Binding::All]);
    // A filter binds the parent (through `@resourceIdentifier`) and every instance.
    let Requirements::List(list) = requirements(&service, "ListGraphs") else {
        panic!()
    };
    assert_eq!(list[0].mode, Mode::Filter);
    assert_eq!(list[0].bindings, [member("org"), Binding::All]);
    assert_eq!(
        list[0].docs.as_deref(),
        Some("Returns only the graphs the caller may read.")
    );
    let Requirements::List(query) = requirements(&service, "ExecuteQuery") else {
        panic!()
    };
    assert_eq!(query.len(), 3);
    assert_eq!(query[1].mode, Mode::Handler);
    assert!(query[1].bindings.is_empty());
    assert_eq!(
        query[1].docs.as_deref(),
        Some("Only when the parsed query writes.")
    );
    assert_eq!(query[2].resource, id("History"));
    assert_eq!(
        query[2].bindings,
        [
            member("organizationId"),
            member("graphId"),
            member("target.history.historyId")
        ]
    );
    assert_eq!(
        query[2].when,
        Some(Condition::Has(vec!["target".into(), "history".into()]))
    );
    assert_eq!(requirements(&service, "GetUser"), &Requirements::None);
    assert_eq!(
        requirements(&service, "RestoreBackup"),
        &Requirements::Pending {
            reason: "guard::allow in the handler until the history module moves.".into()
        }
    );
}

#[test]
fn not_found_is_attached_where_something_can_be_hidden() {
    let service = service();
    let errors = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap()
            .errors
            .clone()
    };
    let not_found = ShapeId::new("crucible.errors", "NotFoundException");
    for name in [
        "GetOrganization",
        "GetGraph",
        "CreateGraph",
        "ListGraphs",
        "ExecuteQuery",
    ] {
        assert_eq!(errors(name), std::slice::from_ref(&not_found), "{name}");
    }
    assert!(errors("GetUser").is_empty());
    assert!(errors("RestoreBackup").is_empty());
}

#[test]
fn the_shared_check_warns_about_pending_operations_only() {
    let service = service();
    let facts = PermissionFacts {
        resources: [
            (
                id("Organization"),
                vec![("organizationId".into(), id("OrganizationId"))],
            ),
            (
                id("Graph"),
                vec![
                    ("organizationId".into(), id("OrganizationId")),
                    ("graphId".into(), id("GraphId")),
                ],
            ),
            (
                id("History"),
                vec![
                    ("organizationId".into(), id("OrganizationId")),
                    ("graphId".into(), id("GraphId")),
                    ("historyId".into(), id("HistoryId")),
                ],
            ),
        ]
        .into(),
        collection_operations: [id("CreateGraph"), id("ListGraphs")].into(),
        declared_at: Default::default(),
    };
    let findings = crucible_smithy::validate::permissions(&service, &facts);
    let rendered: Vec<String> = findings.iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        [
            "example.leyline#RestoreBackup: permissions.pending: `RestoreBackup` has no declared requirements yet: guard::allow in the handler until the history module moves."
        ]
    );
    assert_eq!(findings[0].severity, FindingSeverity::Warning);
}

#[test]
fn the_client_audience_keeps_no_internal_permission() {
    let service = service();
    let external = crucible_smithy::project(&service, Audience::External).unwrap();
    let org = &external.permissions[&id("Organization")];
    let symbols: Vec<&str> = org.permissions.iter().map(|p| p.symbol.as_str()).collect();
    assert_eq!(symbols, ["ADMIN"]);
    // GetOrganization required only the internal permission.
    assert_eq!(
        requirements(&external, "GetOrganization"),
        &Requirements::List(Vec::new())
    );
    // Its error contract is unchanged.
    let get = external
        .operations
        .iter()
        .find(|op| op.id.name() == "GetOrganization")
        .unwrap();
    assert_eq!(get.errors.len(), 1);
}

#[test]
fn a_model_without_not_found_exception_is_refused() {
    let mut value: serde_json::Value = serde_json::from_str(ACCEPTED).unwrap();
    value["shapes"]
        .as_object_mut()
        .unwrap()
        .remove("crucible.errors#NotFoundException");
    let Err(crucible_smithy::Error::Unsupported(diagnostics)) = lower(&value.to_string()) else {
        panic!("expected diagnostics")
    };
    let messages: Vec<String> = diagnostics.iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 5, "{messages:#?}");
    assert!(messages.iter().all(|m| m.contains(
        "permissions.not-found: a requirement attaches `crucible.errors#NotFoundException`"
    )));
}

fn refused() -> Vec<String> {
    match lower(REFUSED) {
        Err(crucible_smithy::Error::Unsupported(diagnostics)) => {
            diagnostics.iter().map(ToString::to_string).collect()
        }
        other => panic!("expected diagnostics, lowered {other:?}"),
    }
}

#[track_caller]
fn expect(diagnostics: &[String], shape: &str, fragment: &str) {
    assert!(
        diagnostics
            .iter()
            .any(|d| d.starts_with(&format!("example.refused#{shape}:")) && d.contains(fragment)),
        "no diagnostic on {shape} containing {fragment:?} in {diagnostics:#?}"
    );
}

#[test]
fn misuse_is_collected_against_the_shape_that_declares_it() {
    let d = refused();
    expect(
        &d,
        "NotAResourcePermission",
        "permissions.vocabulary: example.refused#Nothing is not a resource",
    );
    expect(
        &d,
        "StructurePermission",
        "permissionsOf applies to a string enum",
    );
    expect(
        &d,
        "ThingPermissionAgain",
        "already declares the permissions of example.refused#Thing; one enum per resource",
    );
    expect(
        &d,
        "ThingPermission$BAD",
        "permissions.value: \"Thing:Bad\" is not a permission value",
    );
    expect(
        &d,
        "ThingPermission$BOTH",
        "permissions.tier: a permission is in one tier; @internal and @permissionOnly exclude each other",
    );
    expect(
        &d,
        "ThingPermission$OLD",
        "permissions.alias: the alias `OLD` must be `@deprecated`",
    );
    expect(
        &d,
        "BothDeclared",
        "permissions.exclusive: @requires, @requiresNone exclude each other",
    );
    expect(
        &d,
        "UnknownValue",
        "permissions.unknown: @requires[0]: Thing declares no permission \"thing:fly\"",
    );
    expect(&d, "BadMode", "mode \"sometimes\" is one of");
    expect(
        &d,
        "UnknownIdentifier",
        "permissions.binding: @requires[0]: `Thing` has no identifier `nope`",
    );
    expect(
        &d,
        "IntoList",
        "permissions.list-binding: @requires[0]: `thingId` of `Thing` is bound to `ids`, which passes through the list `ids`",
    );
    expect(
        &d,
        "BadWhen",
        "permissions.condition: @requires[0]: `when` \"has\"",
    );
    expect(
        &d,
        "Undeclared",
        "permissions.undeclared: `Undeclared` says nothing about authorization",
    );
    expect(
        &d,
        "HandlerBinds",
        "permissions.handler: @requires[0]: `by handler` takes no bindings and no `when`",
    );
    expect(
        &d,
        "FilterOnInstance",
        "permissions.filters: @requires[0]: `filters` answers a listing",
    );
    expect(
        &d,
        "OnPlain",
        "permissions.unknown: @requires[0]: `Plain` declares no permissions",
    );
    expect(
        &d,
        "PendingNotString",
        "permissions.reason: @crucible.auth#requiresPending takes the reason as a string",
    );
    expect(&d, "UnknownKey", "has an unknown key `because`");
    expect(
        &d,
        "NotALabel",
        "permissions.binding: @requires[0]: `$nothing` names no URI label",
    );
    expect(&d, "Public", "permissions.public: `Public` is public");
    expect(
        &d,
        "SecondKinds",
        "permissions.types: a service has one @permissionTypes enum",
    );
    expect(
        &d,
        "Unguarded",
        "permissions.unguarded: @requires[0]: `maybe` passes through `maybe`, which a request may leave out: add `when has maybe`",
    );
    expect(
        &d,
        "AliasRequired",
        "permissions.alias: @requires[0]: `OLD` is a deprecated alias",
    );
    expect(
        &d,
        "Kinds",
        "permissions.types: `Thing` has grantable permissions but \"thing\" is not a value",
    );
    expect(
        &d,
        "Kinds",
        "\"zzz\" is not the authorization name of a resource that declares permissions; those are `thing`",
    );
    assert_eq!(d.len(), 25, "{d:#?}");
}

#[test]
fn malformed_trait_values_are_diagnosed_not_skipped() {
    let d = match lower(include_str!("fixtures/permissions-malformed.json")) {
        Err(crucible_smithy::Error::Unsupported(diagnostics)) => diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        other => panic!("expected diagnostics, lowered {other:?}"),
    };
    let on = |shape: &str, fragment: &str| {
        assert!(
            d.iter()
                .any(|m| m.starts_with(&format!("example.malformed#{shape}:"))
                    && m.contains(fragment)),
            "no diagnostic on {shape} containing {fragment:?} in {d:#?}"
        );
    };
    on(
        "Thing",
        "permissions.name: @crucible.auth#authorizationName is a string",
    );
    on(
        "ThingPermission$ALIAS",
        "@crucible.auth#aliasOf names the primary enum member",
    );
    on(
        "NotAnIdPermission",
        "@crucible.auth#permissionsOf names a resource shape id",
    );
    on(
        "TypesOnStructure",
        "@crucible.auth#permissionTypes applies to a string enum",
    );
    on("BadExclude", "`exclude` lists resource shape ids");
    on(
        "NotAList",
        "@crucible.auth#requires is a list of requirements",
    );
    on(
        "NotAnObject",
        "@requires[0] is an object with `permission` and `resource`",
    );
    on(
        "Missing",
        "@requires[0] names a `permission` value and a `resource`",
    );
    on("NotAnId", "`not an id` is not a shape id");
    on("BindNotAnObject", "`bind` maps identifiers to bindings");
    on("BindNotAString", "`thingId` is bound by a string");
    on("BindSyntax", "`ids[]` is not a binding");
    on("WhenNotAString", "`when` is a string");
    on(
        "NoneNotAString",
        "@crucible.auth#requiresNone takes the reason as a string",
    );
    on(
        "Empty",
        "lists no requirement; an operation that needs no permission says @requiresNone",
    );
    on(
        "FilterWhen",
        "permissions.filters: @requires[0]: `filters` takes no `when`",
    );
    assert_eq!(d.len(), 16, "{d:#?}");
}
