//! The `crucible.auth` permission traits (docs/permissions.md §4.11),
//! lowered to the IR the `.cru` front end produces, and checked by the same
//! `validate::permissions`.

use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{Condition, Mode, Requirements, Service, Tier};
use crucible_smithy::validate::{FindingSeverity, PermissionFinding};
use crucible_smithy::{Audience, Capabilities, Model, ShapeId};
use serde_json::{Value, json};

const ACCEPTED: &str = include_str!("fixtures/permissions.json");
const REFUSED: &str = include_str!("fixtures/permissions-refused.json");

fn lower(json: &str) -> Result<Service, crucible_smithy::Error> {
    Model::from_json(json).unwrap().lower(None)
}

fn with_warnings(json: &str) -> (Service, Vec<PermissionFinding>) {
    Model::from_json(json)
        .unwrap()
        .lower_with_warnings(None, &Capabilities::default(), None)
        .unwrap_or_else(|e| panic!("{e}"))
}

fn accepted() -> Value {
    serde_json::from_str(ACCEPTED).unwrap()
}

/// The diagnostics a modified model is refused with.
fn refusal(model: &Value) -> Vec<String> {
    match lower(&model.to_string()) {
        Err(crucible_smithy::Error::Unsupported(diagnostics)) => {
            diagnostics.iter().map(ToString::to_string).collect()
        }
        other => panic!("expected diagnostics, lowered {other:?}"),
    }
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
    let (service, warnings) = with_warnings(ACCEPTED);
    let rendered: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        [
            "example.leyline#RestoreBackup: warning: permissions.pending: `RestoreBackup` has no declared requirements yet: guard::allow in the handler until the history module moves."
        ]
    );
    assert_eq!(warnings[0].severity, FindingSeverity::Warning);
    // On the IR alone the checks that need lowering's facts are skipped,
    // not reported: `ListGraphs` still filters.
    let from_ir: Vec<String> = crucible_smithy::validate::permissions_from_ir(&service)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(from_ir, rendered);
}

#[test]
fn the_client_audience_keeps_no_internal_permission() {
    let service = service();
    let external = crucible_smithy::project(&service, Audience::External).unwrap();
    let org = &external.permissions[&id("Organization")];
    let symbols: Vec<&str> = org.permissions.iter().map(|p| p.symbol.as_str()).collect();
    assert_eq!(symbols, ["ADMIN"]);
    // GetOrganization required only the internal permission: it still has
    // requirements, which the client may not see.
    assert_eq!(
        requirements(&external, "GetOrganization"),
        &Requirements::Internal
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
        "permissions.requires: `Empty` lists no requirement; an operation that needs no permission says `requires none` (Smithy `@requiresNone(\"why\")`)",
    );
    on(
        "FilterWhen",
        "permissions.filters: @requires[0]: `filters` takes no `when`",
    );
    assert_eq!(d.len(), 16, "{d:#?}");
}

/// `RestoreBackup` with these requirements in place of `@requiresPending`.
fn restore_requiring(model: &mut Value, requires: Value) {
    let traits = model["shapes"]["example.leyline#RestoreBackup"]["traits"]
        .as_object_mut()
        .unwrap();
    traits.remove("crucible.auth#requiresPending");
    traits.insert("crucible.auth#requires".into(), requires);
}

fn add_input_member(model: &mut Value, input: &str, name: &str, member: Value) {
    model["shapes"][format!("example.leyline#{input}")]["members"]
        .as_object_mut()
        .unwrap()
        .insert(name.into(), member);
}

#[test]
fn each_service_of_a_model_has_its_own_permission_types() {
    let mut model = accepted();
    let shapes = model["shapes"].as_object_mut().unwrap();
    for (id, shape) in [
        (
            "example.leyline#Shop",
            json!({"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}, "smithy.api#httpBearerAuth": {}}, "resources": [{"target": "example.leyline#Store"}]}),
        ),
        ("example.leyline#StoreId", json!({"type": "string"})),
        (
            "example.leyline#Store",
            json!({"type": "resource", "identifiers": {"storeId": {"target": "example.leyline#StoreId"}}, "read": {"target": "example.leyline#GetStore"}}),
        ),
        (
            "example.leyline#StorePermission",
            json!({"type": "enum", "traits": {"crucible.auth#permissionsOf": "example.leyline#Store"}, "members": {"READ": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "store:read"}}}}),
        ),
        (
            "example.leyline#StoreKind",
            json!({"type": "enum", "traits": {"crucible.auth#permissionTypes": {}}, "members": {"STORE": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "store"}}}}),
        ),
        (
            "example.leyline#GetStore",
            json!({"type": "operation", "input": {"target": "example.leyline#GetStoreInput"}, "traits": {"smithy.api#http": {"method": "GET", "uri": "/stores/{storeId}", "code": 200}, "smithy.api#readonly": {}, "crucible.auth#requires": [{"permission": "store:read", "resource": "example.leyline#Store"}]}}),
        ),
        (
            "example.leyline#GetStoreInput",
            json!({"type": "structure", "members": {"storeId": {"target": "example.leyline#StoreId", "traits": {"smithy.api#required": {}, "smithy.api#httpLabel": {}}}}}),
        ),
    ] {
        shapes.insert(id.into(), shape);
    }
    let model = Model::from_json(&model.to_string()).unwrap();
    let leyline = model
        .lower(Some(&id("Leyline")))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(leyline.permission_types.unwrap().shape, id("ResourceKind"));
    let shop = model
        .lower(Some(&id("Shop")))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(shop.permission_types.unwrap().shape, id("StoreKind"));
    assert_eq!(shop.permissions.keys().collect::<Vec<_>>(), [&id("Store")]);
}

#[test]
fn warnings_and_advice_reach_a_smithy_build() {
    let mut model = accepted();
    restore_requiring(
        &mut model,
        json!([{"permission": "history:read", "resource": "example.leyline#History", "bind": {"historyId": "otherHistory"}}]),
    );
    add_input_member(
        &mut model,
        "RestoreBackupInput",
        "otherHistory",
        json!({"target": "example.leyline#HistoryId", "traits": {"smithy.api#required": {}}}),
    );
    let (_, warnings) = with_warnings(&model.to_string());
    let rendered: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    assert_eq!(
        rendered,
        [
            "example.leyline#RestoreBackup: warning: permissions.other-instance: @requires[0]: `historyId` of `History` is bound to `otherHistory`, but `RestoreBackup` is bound to `History` through `historyId`: the check authorizes another instance than the handler loads; bind `historyId`, or also require a permission on the instance the operation is bound to"
        ]
    );
    // Requiring a permission on the bound instance as well is deliberate.
    restore_requiring(
        &mut model,
        json!([
            {"permission": "history:read", "resource": "example.leyline#History"},
            {"permission": "history:read", "resource": "example.leyline#History", "bind": {"historyId": "otherHistory"}}
        ]),
    );
    let (_, warnings) = with_warnings(&model.to_string());
    assert!(warnings.is_empty(), "{warnings:#?}");
    // Advice: an identifier of a resource with permissions that no
    // requirement covers.
    let mut model = accepted();
    add_input_member(
        &mut model,
        "GetOrganizationInput",
        "graphId",
        json!({"target": "example.leyline#GraphId"}),
    );
    let (_, warnings) = with_warnings(&model.to_string());
    assert_eq!(warnings.len(), 2, "{warnings:#?}");
    assert!(warnings.iter().any(|w| w.severity == FindingSeverity::Advice
        && w.to_string().starts_with(
            "example.leyline#GetOrganization: advice: permissions.unchecked-reference: the input binds `graphId` of `Graph`"
        )));
}

#[test]
fn an_empty_requirement_list_is_refused_by_the_shared_check() {
    let mut model = accepted();
    restore_requiring(&mut model, json!([]));
    assert_eq!(
        refusal(&model),
        [
            "example.leyline#RestoreBackup: permissions.requires: `RestoreBackup` lists no requirement; an operation that needs no permission says `requires none` (Smithy `@requiresNone(\"why\")`)"
        ]
    );
}

#[test]
fn a_binding_never_passes_through_a_map_or_a_map_of_structures() {
    let mut model = accepted();
    let shapes = model["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "example.leyline#HistoryIds".into(),
        json!({"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.leyline#HistoryId"}}),
    );
    shapes.insert(
        "example.leyline#HistoryRefs".into(),
        json!({"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.leyline#HistoryRef"}}),
    );
    restore_requiring(
        &mut model,
        json!([
            {"permission": "history:read", "resource": "example.leyline#History", "bind": {"historyId": "ids"}},
            {"permission": "history:read", "resource": "example.leyline#History", "bind": {"historyId": "refs.historyId"}}
        ]),
    );
    add_input_member(
        &mut model,
        "RestoreBackupInput",
        "ids",
        json!({"target": "example.leyline#HistoryIds", "traits": {"smithy.api#required": {}}}),
    );
    add_input_member(
        &mut model,
        "RestoreBackupInput",
        "refs",
        json!({"target": "example.leyline#HistoryRefs", "traits": {"smithy.api#required": {}}}),
    );
    let d = refusal(&model);
    assert!(
        d.iter().any(|m| m.contains(
            "permissions.list-binding: @requires[0]: `historyId` of `History` is bound to `ids`, which passes through the map `ids`"
        )),
        "{d:#?}"
    );
    assert!(
        d.iter().any(|m| m.contains(
            "permissions.list-binding: @requires[1]: `historyId` of `History` is bound to `refs.historyId`, which passes through the map `refs`"
        )),
        "{d:#?}"
    );
}

#[test]
fn permission_traits_outside_a_vocabulary_are_refused() {
    let mut model = accepted();
    let shapes = model["shapes"].as_object_mut().unwrap();
    shapes.insert(
        "example.leyline#Color".into(),
        json!({"type": "enum", "members": {
            "RED": {"target": "smithy.api#Unit", "traits": {"crucible.auth#permissionOnly": {}}},
            "BLUE": {"target": "smithy.api#Unit", "traits": {"crucible.auth#notGrantable": {}}},
            "GREEN": {"target": "smithy.api#Unit", "traits": {"crucible.auth#aliasOf": "RED"}}
        }}),
    );
    shapes.insert(
        "example.leyline#SnapshotId".into(),
        json!({"type": "string"}),
    );
    shapes.insert(
        "example.leyline#Snapshot".into(),
        json!({"type": "resource", "identifiers": {"organizationId": {"target": "example.leyline#OrganizationId"}, "graphId": {"target": "example.leyline#GraphId"}, "snapshotId": {"target": "example.leyline#SnapshotId"}}, "traits": {"crucible.auth#authorizationName": "snap"}}),
    );
    model["shapes"]["example.leyline#Graph"]["resources"]
        .as_array_mut()
        .unwrap()
        .push(json!({"target": "example.leyline#Snapshot"}));
    let d = refusal(&model);
    for (site, name) in [
        ("RED", "permissionOnly"),
        ("BLUE", "notGrantable"),
        ("GREEN", "aliasOf"),
    ] {
        assert!(
            d.contains(&format!(
                "example.leyline#Color${site}: permissions.vocabulary: @crucible.auth#{name} applies to a member of a @crucible.auth#permissionsOf enum: a permission"
            )),
            "{d:#?}"
        );
    }
    assert!(
        d.iter().any(|m| m.starts_with(
            "example.leyline#Snapshot: permissions.name: example.leyline#Snapshot has @crucible.auth#authorizationName but declares no permissions"
        )),
        "{d:#?}"
    );
    assert_eq!(d.len(), 4, "{d:#?}");
}

#[test]
fn only_an_explicit_empty_auth_marks_an_operation_public() {
    let mut model = accepted();
    let get_user = model["shapes"]["example.leyline#GetUser"]["traits"]
        .as_object_mut()
        .unwrap();
    get_user.remove("crucible.auth#requiresNone");
    get_user.insert("smithy.api#auth".into(), json!([]));
    let service = lower(&model.to_string()).unwrap_or_else(|e| panic!("{e}"));
    let public: Vec<&str> = service
        .operations
        .iter()
        .filter(|op| op.public)
        .map(|op| op.id.name())
        .collect();
    assert_eq!(public, ["GetUser"]);
    // A service with no scheme leaves its operations without one, which is
    // not a declaration that they are public.
    let bare = json!({"smithy": "2.0", "shapes": {
        "example.bare#Bare": {"type": "service", "version": "1", "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.bare#Ping"}]},
        "crucible.protocols#json": {"type": "structure", "traits": {"smithy.api#trait": {}, "smithy.api#protocolDefinition": {}}},
        "example.bare#Ping": {"type": "operation", "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping", "code": 200}}}
    }});
    let service = lower(&bare.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(service.operations[0].auth.is_empty());
    assert!(!service.operations[0].public);
}
