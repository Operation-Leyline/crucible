//! Typed permissions in `.cru` (docs/permissions.md): the syntax, its
//! lowering to the IR, and the checks of §5, against a Pigeon-shaped model
//! and one small model per construct.

use std::path::Path;

use crucible_lang::ast::{BindingValue, ConditionKind, Decl, OpEntry, RequirementKind};
use crucible_lang::{Compiled, Package, Severity, SourceFile, compile, parse};
use crucible_smithy::ShapeId;
use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{Condition, Mode, Requirements, Service, Tier};
use serde_json::json;

const PIGEON: &[&str] = &["service.cru", "tenancy.cru", "mail.cru", "grants.cru"];

fn pigeon_files() -> Vec<SourceFile> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pigeon");
    PIGEON
        .iter()
        .map(|name| SourceFile {
            path: format!("pigeon/{name}"),
            text: std::fs::read_to_string(dir.join(name)).expect("fixture"),
        })
        .collect()
}

fn package(files: Vec<SourceFile>) -> Package {
    let mut package = Package::new("example.pigeon", files);
    package.imports = vec!["crucible.errors".to_owned()];
    package
}

fn pigeon() -> (Service, Compiled) {
    let compiled = compile(&package(pigeon_files()), None);
    assert!(!compiled.has_errors(), "\n{}", compiled.render());
    (compiled.service.clone().expect("service"), compiled)
}

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.pigeon", name)
}

fn member(path: &str) -> Binding {
    Binding::Member(path.split('.').map(str::to_owned).collect())
}

fn requirements<'s>(service: &'s Service, operation: &str) -> &'s Requirements {
    &service
        .operations
        .iter()
        .find(|op| op.id.name() == operation)
        .unwrap_or_else(|| panic!("no operation {operation}"))
        .requirements
}

// ---- the Pigeon model -------------------------------------------------------

#[test]
fn the_pigeon_model_compiles_with_its_one_pending_operation_and_one_advice() {
    let (_, compiled) = pigeon();
    let found: Vec<(Severity, &str)> = compiled
        .diagnostics
        .iter()
        .map(|d| (d.diagnostic.severity, d.diagnostic.message.as_str()))
        .collect();
    assert_eq!(found.len(), 2, "\n{}", compiled.render());
    assert!(found.iter().any(|(s, m)| *s == Severity::Warning
        && m.starts_with("permissions.pending: `GetUsage` has no declared requirements yet: guard::allow in get_usage.rs")));
    assert!(found.iter().any(|(s, m)| *s == Severity::Advice
        && m.starts_with(
            "permissions.unchecked-reference: the input binds `domainId` of `Domain`"
        )
        && m.contains("`CreateInbox`")));
}

#[test]
fn resources_declare_their_permissions_with_names_identifiers_and_tiers() {
    let (service, _) = pigeon();
    let resources: Vec<&str> = service.permissions.keys().map(ShapeId::name).collect();
    // ApiKey, Message and Grant declare none, so they are not listed.
    assert_eq!(resources, ["Domain", "Inbox", "Template", "Workspace"]);
    let inbox = &service.permissions[&id("Inbox")];
    assert_eq!(inbox.name, "inbox");
    assert!(!inbox.name_declared);
    assert_eq!(inbox.identifiers, ["workspaceId", "inboxId"]);
    let symbols: Vec<(&str, &str)> = inbox
        .permissions
        .iter()
        .map(|p| (p.symbol.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(
        symbols,
        [
            ("read", "inbox:read"),
            ("manage", "inbox:manage"),
            ("mailRead", "mail:read"),
            ("mailSend", "mail:send"),
            ("mailManage", "mail:manage"),
            ("emailSend", "email:send"),
        ]
    );
    let alias = inbox.permission("emailSend").unwrap();
    assert_eq!(alias.alias_of.as_deref(), Some("mailSend"));
    assert_eq!(
        alias
            .notes
            .deprecated
            .as_ref()
            .and_then(|d| d.message.as_deref()),
        Some("Renamed to mail:send.")
    );
    assert_eq!(
        inbox.permission("mailSend").unwrap().docs.as_deref(),
        Some("Send, reply and forward from this address. Does not imply `mailRead`.")
    );
    let workspace = &service.permissions[&id("Workspace")];
    assert_eq!(workspace.identifiers, ["workspaceId"]);
    assert_eq!(
        workspace.permission("admin").unwrap().tier,
        Tier::NotGrantable
    );
    assert_eq!(
        workspace.permission("usageRead").unwrap().tier,
        Tier::PermissionOnly
    );
    assert_eq!(workspace.permission("read").unwrap().tier, Tier::Public);
    assert_eq!(
        workspace.permission("apiKeyManage").unwrap().value,
        "api-key:manage"
    );
}

#[test]
fn the_registry_is_sorted_by_resource_then_value() {
    let (service, _) = pigeon();
    let registry: Vec<String> = service
        .all_permissions()
        .into_iter()
        .map(|(r, p)| format!("{}/{}", r.name, p.value))
        .collect();
    let mut sorted = registry.clone();
    sorted.sort();
    assert_eq!(registry, sorted);
    assert_eq!(registry.len(), 6 + 2 + 2 + 9);
    assert_eq!(registry[0], "domain/domain:manage");
}

#[test]
fn instance_parent_and_collection_requirements_bind_every_identifier() {
    let (service, _) = pigeon();
    let Requirements::List(get_inbox) = requirements(&service, "GetInbox") else {
        panic!()
    };
    assert_eq!(get_inbox.len(), 1);
    assert_eq!(get_inbox[0].mode, Mode::Check);
    assert_eq!(get_inbox[0].resource, id("Inbox"));
    assert_eq!(get_inbox[0].permission, "read");
    assert_eq!(
        get_inbox[0].bindings,
        [member("workspaceId"), member("inboxId")]
    );
    // A message operation checks its parent inbox.
    let Requirements::List(get_message) = requirements(&service, "GetMessage") else {
        panic!()
    };
    assert_eq!(get_message[0].resource, id("Inbox"));
    assert_eq!(get_message[0].permission, "mailRead");
    // The workspace itself: one identifier.
    let Requirements::List(delete_key) = requirements(&service, "DeleteApiKey") else {
        panic!()
    };
    assert_eq!(delete_key[0].resource, id("Workspace"));
    assert_eq!(delete_key[0].bindings, [member("workspaceId")]);
    // A creation: the parent by name, every instance.
    let Requirements::List(create) = requirements(&service, "CreateInbox") else {
        panic!()
    };
    assert_eq!(create[0].bindings, [member("workspaceId"), Binding::All]);
}

#[test]
fn send_message_requires_the_template_only_when_one_is_sent() {
    let (service, _) = pigeon();
    let Requirements::List(clauses) = requirements(&service, "SendMessage") else {
        panic!()
    };
    assert_eq!(clauses.len(), 2);
    assert_eq!(clauses[0].permission, "mailSend");
    assert_eq!(clauses[0].when, None);
    let template = &clauses[1];
    assert_eq!(template.resource, id("Template"));
    assert_eq!(template.permission, "read");
    assert_eq!(
        template.bindings,
        [member("workspaceId"), member("content.template.templateId")]
    );
    assert_eq!(
        template.when,
        Some(Condition::Has(vec!["content".into(), "template".into()]))
    );
    assert_eq!(
        template.docs.as_deref(),
        Some("The rendered wording is what the recipient reads.")
    );
}

#[test]
fn a_listing_filters_and_handler_obligations_bind_nothing() {
    let (service, _) = pigeon();
    let Requirements::List(list) = requirements(&service, "ListInboxes") else {
        panic!()
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].mode, Mode::Filter);
    assert_eq!(list[0].bindings, [member("workspaceId"), Binding::All]);
    for operation in ["PutGrant", "DeleteGrant"] {
        let Requirements::List(clauses) = requirements(&service, operation) else {
            panic!()
        };
        assert_eq!(clauses[0].mode, Mode::Check);
        assert_eq!(clauses[0].permission, "grantManage");
        assert_eq!(clauses[1].mode, Mode::Handler);
        assert_eq!(clauses[1].permission, "admin");
        assert!(clauses[1].bindings.is_empty());
        assert!(clauses[1].when.is_none());
        assert!(clauses[1].docs.as_deref().unwrap().starts_with("Only when"));
    }
}

#[test]
fn none_and_pending_lower_to_their_own_variants() {
    let (service, _) = pigeon();
    assert_eq!(requirements(&service, "GetMe"), &Requirements::None);
    assert_eq!(
        requirements(&service, "ListMyWorkspaces"),
        &Requirements::None
    );
    assert_eq!(
        requirements(&service, "GetUsage"),
        &Requirements::Pending {
            reason: "guard::allow in get_usage.rs until the workspace folder moves".into()
        }
    );
}

#[test]
fn resource_bound_operations_get_the_framework_not_found() {
    let (service, _) = pigeon();
    let errors = |name: &str| -> Vec<String> {
        service
            .operations
            .iter()
            .find(|op| op.id.name() == name)
            .unwrap()
            .errors
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    // After the operation's own errors, before the service's.
    assert_eq!(
        errors("GetInbox"),
        [
            "crucible.errors#NotFoundException",
            "crucible.errors#ValidationException",
            "crucible.errors#InternalServerException"
        ]
    );
    // Nothing to hide: no requirement.
    assert!(!errors("GetMe").iter().any(|e| e.contains("NotFound")));
    assert!(!errors("GetUsage").iter().any(|e| e.contains("NotFound")));
    let not_found = service
        .shape(&ShapeId::new("crucible.errors", "NotFoundException"))
        .expect("the shape is in the IR");
    let crucible_smithy::ir::Shape::Structure(s) = not_found else {
        panic!()
    };
    assert_eq!(s.error.as_ref().unwrap().status, 404);
    let members: Vec<&str> = s.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(members, ["message", "resourceType"]);
}

#[test]
fn the_permission_types_enum_is_recorded() {
    let (service, _) = pigeon();
    let types = service.permission_types.as_ref().expect("permission types");
    assert_eq!(types.shape, id("ResourceType"));
    assert!(types.excluded.is_empty());
}

#[test]
fn a_snapshot_round_trips_and_an_older_one_still_reads() {
    let (service, _) = pigeon();
    let text = crucible_smithy::snapshot::write(&service, "test");
    let read = crucible_smithy::snapshot::read(&text).expect("reads back");
    assert_eq!(read.permissions, service.permissions);
    assert_eq!(read.permission_types, service.permission_types);
    for (a, b) in read.operations.iter().zip(&service.operations) {
        assert_eq!(a.requirements, b.requirements);
    }
    // A release frozen before permissions existed has neither field.
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let object = value["service"].as_object_mut().unwrap();
    object.remove("permissions");
    object.remove("permission_types");
    for op in object["operations"].as_array_mut().unwrap() {
        op.as_object_mut().unwrap().remove("requirements");
    }
    let old = crucible_smithy::snapshot::read(&value.to_string()).expect("an older release reads");
    assert!(old.permissions.is_empty());
    assert!(old.permission_types.is_none());
    assert!(
        old.operations
            .iter()
            .all(|op| op.requirements.is_undeclared())
    );
    // And a service without permissions writes no new keys.
    let mut plain = old.clone();
    plain.versions.clear();
    let written: serde_json::Value =
        serde_json::from_str(&crucible_smithy::snapshot::write(&plain, "test")).unwrap();
    assert!(written["service"].get("permissions").is_none());
    assert!(written["service"].get("permission_types").is_none());
    for op in written["service"]["operations"].as_array().unwrap() {
        assert!(op.get("requirements").is_none());
    }
}

#[test]
fn the_ir_serializes_with_the_contract_names() {
    let (service, _) = pigeon();
    let send = service
        .operations
        .iter()
        .find(|op| op.id.name() == "SendMessage")
        .unwrap();
    let value = serde_json::to_value(&send.requirements).unwrap();
    assert_eq!(
        value["List"][1],
        json!({
            "mode": "Check",
            "resource": "example.pigeon#Template",
            "permission": "read",
            "bindings": [{"Member": ["workspaceId"]}, {"Member": ["content", "template", "templateId"]}],
            "when": {"Has": ["content", "template"]},
            "docs": "The rendered wording is what the recipient reads."
        })
    );
    let get_me = service
        .operations
        .iter()
        .find(|op| op.id.name() == "GetMe")
        .unwrap();
    assert_eq!(
        serde_json::to_value(&get_me.requirements).unwrap(),
        json!("None")
    );
    let inbox = serde_json::to_value(&service.permissions[&id("Inbox")]).unwrap();
    assert_eq!(inbox["name"], "inbox");
    assert_eq!(inbox["name_declared"], false);
    assert_eq!(inbox["permissions"][5]["alias_of"], "mailSend");
    assert_eq!(inbox["permissions"][5]["tier"], "Public");
}

#[test]
fn the_client_audience_drops_internal_permissions_and_their_requirements() {
    let source = BASE.replace(
        "permission manage = \"store:manage\"",
        "internal permission manage = \"store:manage\"\n    @deprecated @aliasOf(manage)\n    internal permission oldManage = \"store:admin\"",
    );
    let compiled = compile(&small(&source), None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    let store = ShapeId::new("example.shop", "Store");
    assert_eq!(service.permissions[&store].permissions.len(), 3);
    let external = crucible_smithy::project(&service, crucible_smithy::Audience::External).unwrap();
    let symbols: Vec<&str> = external.permissions[&store]
        .permissions
        .iter()
        .map(|p| p.symbol.as_str())
        .collect();
    assert_eq!(symbols, ["read"]);
    let manage = external
        .operations
        .iter()
        .find(|op| op.id.name() == "ManageStore")
        .unwrap();
    // `requires manage` is dropped; `requires read` stays.
    let Requirements::List(clauses) = &manage.requirements else {
        panic!()
    };
    let kept: Vec<&str> = clauses.iter().map(|c| c.permission.as_str()).collect();
    assert_eq!(kept, ["read"]);
    let internal = crucible_smithy::project(&service, crucible_smithy::Audience::Internal).unwrap();
    assert_eq!(internal.permissions[&store].permissions.len(), 3);
}

#[test]
fn the_source_names_the_resource_with_authorization_name() {
    let source = BASE.replace(
        "resource Store {",
        "@authorizationName(\"shop\")\nresource Store {",
    );
    let compiled = compile(&small(&source), None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    let service = compiled.service.unwrap();
    let store = &service.permissions[&ShapeId::new("example.shop", "Store")];
    assert_eq!(store.name, "shop");
    assert!(store.name_declared);
}

// ---- syntax -----------------------------------------------------------------

#[test]
fn the_parser_reads_every_clause_form() {
    let parsed = parse(
        r#"
resource Inbox in Workspace {
    id inboxId: InboxId
    /// Doc.
    @permissionOnly
    internal permission x = "a:b"
}
operation Op on Inbox {
    requires none
    requires pending because "later"
    requires mailSend
    requires read on Template(content.template.templateId) when has content.template
    requires read on Template(templateId: $x, workspaceId: *) when not (a == "b" or c in ["d", 1]) and has e
    requires write on Graph by handler
    filters read on Inbox
}
"#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Resource(resource) = &parsed.file.items[0].decl else {
        panic!()
    };
    assert_eq!(resource.entries.len(), 1);
    let permission = &resource.permissions[0];
    assert_eq!(permission.name.name, "x");
    assert_eq!(permission.value.value, "a:b");
    assert_eq!(permission.modifiers[0].name, "internal");
    assert_eq!(permission.annotations[0].name.name, "permissionOnly");
    assert_eq!(permission.doc.as_ref().unwrap().text, "Doc.");
    let Decl::Operation(op) = &parsed.file.items[1].decl else {
        panic!()
    };
    let kinds: Vec<&RequirementKind> = op
        .entries
        .iter()
        .map(|e| match e {
            OpEntry::Requirement(r) => &r.kind,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(matches!(kinds[0], RequirementKind::None(_)));
    assert!(matches!(kinds[1], RequirementKind::Pending(reason) if reason.value == "later"));
    assert!(matches!(
        kinds[2],
        RequirementKind::Requires {
            target: None,
            when: None,
            handler: None,
            ..
        }
    ));
    let RequirementKind::Requires {
        target: Some(target),
        when: Some(when),
        ..
    } = kinds[3]
    else {
        panic!()
    };
    assert_eq!(target.resource.name, "Template");
    let bindings = target.bindings.as_ref().unwrap();
    assert!(bindings[0].name.is_none());
    assert!(matches!(&bindings[0].value, BindingValue::Path(p) if p.parts.len() == 3));
    assert!(matches!(&when.kind, ConditionKind::Has(p) if p.parts.len() == 2));
    let RequirementKind::Requires {
        target: Some(target),
        when: Some(when),
        ..
    } = kinds[4]
    else {
        panic!()
    };
    let bindings = target.bindings.as_ref().unwrap();
    assert_eq!(bindings[0].name.as_ref().unwrap().name, "templateId");
    assert!(matches!(&bindings[0].value, BindingValue::Label(l) if l.name == "x"));
    assert!(matches!(bindings[1].value, BindingValue::All(_)));
    let ConditionKind::And(parts) = &when.kind else {
        panic!("{when:?}")
    };
    assert!(
        matches!(&parts[0].kind, ConditionKind::Not(inner) if matches!(inner.kind, ConditionKind::Or(_)))
    );
    assert!(matches!(
        kinds[5],
        RequirementKind::Requires {
            handler: Some(_),
            ..
        }
    ));
    assert!(matches!(kinds[6], RequirementKind::Filters { .. }));
}

#[test]
fn a_broken_clause_is_reported_and_parsing_goes_on() {
    let parsed = parse(
        r#"
operation A {
    requires pending because
}
operation B {
    filters read
}
operation C {
    requires read when x
}
operation D {
    requires read on Template("x")
}
resource R {
    permission p "a:b"
}
structure After { x: string }
"#,
    );
    let messages: Vec<&str> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for needle in [
        "expected the reason, as a string",
        "expected `on`",
        "expected `==` or `in` after the member path",
        "expected `*`, `$identifier` or an input member path",
        "expected `=`",
    ] {
        assert!(
            messages.iter().any(|m| m.contains(needle)),
            "{needle}: {messages:#?}"
        );
    }
    // The declaration after the broken ones is still read.
    assert!(
        parsed
            .file
            .items
            .iter()
            .any(|item| matches!(&item.decl, Decl::Structure(s) if s.name.name == "After"))
    );
}

#[test]
fn a_requirement_keeps_its_documentation() {
    let parsed = parse("operation Op {\n    /// Why.\n    requires none\n    http GET \"/\"\n}\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Operation(op) = &parsed.file.items[0].decl else {
        panic!()
    };
    let OpEntry::Requirement(r) = &op.entries[0] else {
        panic!()
    };
    assert_eq!(r.doc.as_ref().unwrap().text, "Why.");
}

// ---- refusals, one small model each ------------------------------------------

const BASE: &str = r#"
service Shop {
    version "1"
    protocol json
    auth bearer
    operations [Ping]
    resources [Store]
}

readonly operation Ping {
    http GET "/ping"
    requires none
}

string StoreId { pattern "^s_" }
string OrderId { pattern "^o_" }
string OrderRef { pattern "^o_" }
list OrderIds: OrderId

resource Store {
    id storeId: StoreId
    permission read = "store:read"
    permission manage = "store:manage"
}

resource Order in Store {
    id orderId: OrderId
    readonly property note?: string
    permission read = "order:read"
}

readonly operation GetOrder reads Order {
    http GET "/stores/{storeId}/orders/{orderId}"
    requires read
    input {
        $storeId
        $orderId
    }
    output { $note? }
}

operation ManageStore on Store {
    http POST "/stores/{storeId}"
    requires manage
    requires read
    input { $storeId }
}
"#;

fn small(text: &str) -> Package {
    let mut package = Package::new(
        "example.shop",
        vec![SourceFile {
            path: "shop.cru".into(),
            text: text.to_owned(),
        }],
    );
    package.imports = vec!["crucible.errors".to_owned()];
    package
}

/// Every diagnostic of `BASE` plus `extra`, as `severity: message`. The
/// service lists the operations `extra` declares unattached.
fn check(extra: &str) -> Vec<String> {
    let unattached: Vec<&str> = extra
        .lines()
        .filter_map(|line| {
            let rest = line
                .trim_start_matches("public ")
                .trim_start_matches("readonly ");
            let name = rest.strip_prefix("operation ")?.strip_suffix(" {")?;
            (!name.contains(' ')).then_some(name)
        })
        .collect();
    let operations: Vec<&str> = ["Ping"].into_iter().chain(unattached).collect();
    let base = BASE.replace(
        "operations [Ping]",
        &format!("operations [{}]", operations.join(", ")),
    );
    check_source(&format!("{base}\n{extra}"))
}

fn check_source(source: &str) -> Vec<String> {
    let compiled = compile(&small(source), None);
    assert_eq!(compiled.service.is_none(), compiled.has_errors());
    compiled
        .diagnostics
        .iter()
        .map(|d| {
            let severity = match d.diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Advice => "advice",
            };
            format!("{severity}: {}", d.diagnostic.message)
        })
        .collect()
}

#[track_caller]
fn expect(found: &[String], needle: &str) {
    assert!(
        found.iter().any(|m| m.contains(needle)),
        "no diagnostic contains {needle:?}:\n{found:#?}"
    );
}

#[test]
fn the_base_model_is_clean() {
    assert_eq!(check(""), Vec::<String>::new());
}

#[test]
fn once_a_permission_exists_every_authenticated_operation_says_something() {
    let found = check("readonly operation Other {\n    http GET \"/other\"\n}");
    expect(
        &found,
        "error: permissions.undeclared: `Other` says nothing about authorization",
    );
    // A public operation is exempt, and may not declare anything.
    assert!(check("public readonly operation Open {\n    http GET \"/open\"\n}").is_empty());
    expect(
        &check("public readonly operation Open {\n    http GET \"/open\"\n    requires none\n}"),
        "error: permissions.public: `Open` is public",
    );
}

#[test]
fn a_service_without_permissions_needs_no_declarations() {
    let source = r#"
service Plain {
    version "1"
    protocol json
    auth bearer
    operations [Ping]
}
readonly operation Ping {
    http GET "/ping"
}
"#;
    assert!(check_source(source).is_empty());
}

#[test]
fn none_and_pending_stand_alone_and_pending_needs_a_reason() {
    let found = check(
        "readonly operation A on Store {\n    http GET \"/a/{storeId}\"\n    requires none\n    requires read\n    input { $storeId }\n}",
    );
    expect(
        &found,
        "error: permissions.exclusive: `requires none` stands alone",
    );
    let found = check(
        "readonly operation B {\n    http GET \"/b\"\n    requires pending because \"  \"\n}",
    );
    expect(
        &found,
        "error: permissions.reason: `requires pending` needs a nonempty reason",
    );
    let found = check(
        "readonly operation C {\n    http GET \"/c\"\n    requires pending because \"the handler checks\"\n}",
    );
    assert_eq!(
        found,
        ["warning: permissions.pending: `C` has no declared requirements yet: the handler checks"]
    );
}

#[test]
fn a_requirement_names_a_permission_of_its_resource() {
    let op = |clause: &str| {
        format!(
            "readonly operation X on Order {{\n    http GET \"/x/{{storeId}}/{{orderId}}\"\n    {clause}\n    input {{\n        $storeId\n        $orderId\n    }}\n}}"
        )
    };
    let found = check(&op("requires raed"));
    expect(
        &found,
        "permissions.unknown: `Order` declares no permission `raed`; did you mean `read`?",
    );
    let found = check(&op("requires manage"));
    expect(
        &found,
        "`Order` declares no permission `manage`; it is a permission of `Store`",
    );
    let found = check(&op("requires read on Nothing"));
    expect(
        &found,
        "permissions.unknown: `Nothing` is not a resource of this service",
    );
}

#[test]
fn a_resource_without_permissions_cannot_be_required_on() {
    let source = BASE.replace(
        "resource Order in Store {\n    id orderId: OrderId\n    readonly property note?: string\n    permission read = \"order:read\"\n}",
        "resource Order in Store {\n    id orderId: OrderId\n    readonly property note?: string\n}",
    );
    let found = check_source(&source);
    expect(
        &found,
        "permissions.unknown: `Order` declares no permissions; `read` is a permission of `Store`",
    );
}

#[test]
fn values_follow_the_grammar_and_are_unique_case_sensitively() {
    for value in [
        "Store:Write",
        "*",
        "store write",
        "store_write",
        "store:",
        "store--x:y",
    ] {
        let source = BASE.replace(
            "permission manage = \"store:manage\"",
            &format!("permission manage = \"store:manage\"\n    @permissionOnly\n    permission other = {value:?}"),
        );
        expect(&check_source(&source), "permissions.value");
    }
    // Hyphens inside a segment are fine, and lowercase-led camelCase too.
    let source = BASE.replace(
        "permission manage = \"store:manage\"",
        "permission manage = \"store:manage\"\n    @permissionOnly\n    permission keys = \"api-key:manage\"\n    @permissionOnly\n    permission leyline = \"apiKey:manage\"",
    );
    assert!(
        check_source(&source).is_empty(),
        "{:#?}",
        check_source(&source)
    );
    let duplicated = |extra: &str| {
        check_source(&BASE.replace(
            "permission manage = \"store:manage\"",
            &format!("permission manage = \"store:manage\"\n    {extra}"),
        ))
    };
    expect(
        &duplicated("@permissionOnly permission manage = \"store:other\""),
        "declares the permission `manage` twice",
    );
    expect(
        &duplicated("@permissionOnly permission again = \"store:manage\""),
        "declares the value \"store:manage\" twice",
    );
    expect(
        &duplicated("@permissionOnly permission shout = \"store:Manage\""),
        "differs from \"store:manage\" only in case",
    );
}

#[test]
fn a_permission_is_in_one_tier() {
    let found = check_source(&BASE.replace(
        "permission manage = \"store:manage\"",
        "@permissionOnly @notGrantable\n    internal permission manage = \"store:manage\"",
    ));
    expect(
        &found,
        "permissions.tier: a permission is in one tier; `internal` and `@permissionOnly` and `@notGrantable` exclude each other",
    );
    // A permission-only permission is required by nothing.
    let found = check_source(&BASE.replace(
        "permission manage = \"store:manage\"",
        "@permissionOnly\n    permission manage = \"store:manage\"",
    ));
    expect(
        &found,
        "error: permissions.tier: `manage` is `@permissionOnly`",
    );
    // A public permission nothing requires is a warning.
    let found = check_source(&BASE.replace(
        "permission manage = \"store:manage\"",
        "permission manage = \"store:manage\"\n    permission audit = \"store:audit\"",
    ));
    assert_eq!(
        found,
        [
            "warning: permissions.unrequired: no operation requires `audit`; declare it `@permissionOnly` if the service's policy checks it, or require it"
        ]
    );
}

#[test]
fn aliases_are_deprecated_same_tier_and_never_required() {
    let with = |entries: &str| {
        check_source(&BASE.replace(
            "permission manage = \"store:manage\"",
            &format!("permission manage = \"store:manage\"\n    {entries}"),
        ))
    };
    assert!(with("@deprecated @aliasOf(manage) permission admin = \"store:admin\"").is_empty());
    expect(
        &with("@aliasOf(manage) permission admin = \"store:admin\""),
        "permissions.alias: the alias `admin` must be `@deprecated`",
    );
    expect(
        &with("@deprecated @aliasOf(nothing) permission admin = \"store:admin\""),
        "is an alias of `nothing`, which `Store` does not declare",
    );
    expect(
        &with(
            "@deprecated @aliasOf(manage) permission a = \"store:a\"\n    @deprecated @aliasOf(a) permission b = \"store:b\"",
        ),
        "`b` is an alias of `a`, itself an alias; name the primary `manage`",
    );
    expect(
        &with("@deprecated @aliasOf(manage) internal permission a = \"store:a\""),
        "but in another tier",
    );
    expect(
        &with("@aliasOf(\"manage\") permission a = \"store:a\""),
        "`@aliasOf` names the primary permission's symbol",
    );
    let found = check_source(&BASE.replace(
        "permission manage = \"store:manage\"",
        "permission manage = \"store:manage\"\n    @deprecated @aliasOf(manage) permission admin = \"store:admin\"",
    ).replace("    requires manage\n", "    requires admin\n"));
    expect(
        &found,
        "permissions.alias: `admin` is a deprecated alias; requirements name primaries: did you mean `manage`?",
    );
}

#[test]
fn a_permission_takes_only_its_own_annotations() {
    let found = check_source(&BASE.replace(
        "permission manage = \"store:manage\"",
        "@query\n    permission manage = \"store:manage\"",
    ));
    expect(&found, "`@query` does not apply to a permission");
}

#[test]
fn authorization_names_are_lower_camel_and_unique() {
    let found = check_source(&BASE.replace(
        "resource Store {",
        "@authorizationName(\"Shop\")\nresource Store {",
    ));
    expect(
        &found,
        "permissions.name: the authorization name `Shop` is not lowerCamelCase",
    );
    let found = check_source(&BASE.replace(
        "resource Order in Store {",
        "@authorizationName(\"store\")\nresource Order in Store {",
    ));
    expect(&found, "both have the authorization name `store`");
    let found = check_source(&BASE.replace(
        "resource Store {",
        "@authorizationName(shop)\nresource Store {",
    ));
    expect(&found, "`@authorizationName` takes the name as a string");
    let found = check_source(&BASE.replace("resource Store {", "@deprecated\nresource Store {"));
    expect(
        &found,
        "annotations on a resource are not supported by the compiler yet, apart from `@authorizationName`",
    );
}

#[test]
fn bindings_bind_each_identifier_once_from_the_input() {
    let op = |target: &str, input: &str| {
        format!(
            "readonly operation X on Store {{\n    http GET \"/x/{{storeId}}\"\n    requires read on {target}\n    input {{\n        $storeId\n        {input}\n    }}\n}}"
        )
    };
    // An unnamed binding binds the resource's own identifier.
    assert!(check(&op("Order(ref)", "@query ref: OrderId")).is_empty());
    expect(
        &check(&op("Order(nothing: $storeId)", "")),
        "permissions.binding: `Order` has no identifier `nothing`",
    );
    expect(
        &check(&op("Order(a, b)", "@query a: OrderId\n @query b: OrderId")),
        "one binding may go unnamed",
    );
    expect(
        &check(&op(
            "Order(orderId: a, orderId: b)",
            "@query a: OrderId\n @query b: OrderId",
        )),
        "`orderId` is bound twice",
    );
    // Implicitly by name: the input has no `orderId`.
    expect(
        &check(&op("Order", "")),
        "permissions.binding: `orderId` of `Order` is bound to `orderId`, but the input has no member `orderId`",
    );
    // `*` binds only the resource's own identifier.
    expect(
        &check(&op("Order(storeId: *, orderId: *)", "")),
        "`*` binds only `Order`'s own identifier, never a parent",
    );
    // A member of another type.
    expect(
        &check(&op("Order(count)", "@query count: integer")),
        "permissions.binding: `orderId` of `Order` is bound to `count`, a `Integer`; the identifier is a `OrderId`; bind a member of that type",
    );
    // A different string type with the same constraints is a warning.
    let found = check(&op("Order(ref)", "@query ref: OrderRef"));
    assert_eq!(found.len(), 1, "{found:#?}");
    expect(&found, "warning: permissions.binding-type");
}

#[test]
fn a_binding_never_passes_through_a_list_or_map() {
    let found = check(
        "structure Line { orderId: OrderId }\nlist Lines: Line\noperation Batch on Store {\n    http POST \"/batch/{storeId}\"\n    requires read on Order(orderIds)\n    requires read on Order(lines.orderId)\n    input {\n        $storeId\n        orderIds: OrderIds\n        lines: Lines\n    }\n}",
    );
    expect(
        &found,
        "permissions.list-binding: `orderId` of `Order` is bound to `orderIds`, which passes through the list `orderIds`",
    );
    expect(&found, "passes through the list `lines`");
    expect(
        &found,
        "authorize on a shared parent every element belongs to, or check each element `by handler`",
    );
}

#[test]
fn an_optional_path_is_guarded_by_when() {
    let op = |clause: &str| {
        format!(
            "union Pick {{\n    order: OrderPick\n    nothing: string\n}}\nstructure OrderPick {{ orderId: OrderId }}\noperation X on Store {{\n    http POST \"/x/{{storeId}}\"\n    {clause}\n    input {{\n        $storeId\n        maybe?: OrderId\n        pick: Pick\n    }}\n}}"
        )
    };
    expect(
        &check(&op("requires read on Order(maybe)")),
        "permissions.unguarded: `maybe` passes through `maybe`, which a request may leave out: add `when has maybe`",
    );
    assert!(check(&op("requires read on Order(maybe) when has maybe")).is_empty());
    expect(
        &check(&op("requires read on Order(pick.order.orderId)")),
        "add `when has pick.order`",
    );
    assert!(
        check(&op(
            "requires read on Order(pick.order.orderId) when has pick.order"
        ))
        .is_empty()
    );
    // `or` guarantees only what both sides do; `not` guarantees nothing.
    expect(
        &check(&op(
            "requires read on Order(maybe) when has maybe or has pick.order",
        )),
        "permissions.unguarded",
    );
    expect(
        &check(&op("requires read on Order(maybe) when not has maybe")),
        "permissions.unguarded",
    );
    assert!(
        check(&op(
            "requires read on Order(maybe) when has maybe and has pick.order"
        ))
        .is_empty()
    );
}

#[test]
fn conditions_read_input_members_and_compare_like_with_like() {
    let op = |when: &str| {
        format!(
            "enum Kind {{\n    a = \"a\"\n    b = \"b\"\n}}\noperation X on Store {{\n    http POST \"/x/{{storeId}}\"\n    requires manage when {when}\n    input {{\n        $storeId\n        kind?: Kind\n        count?: integer\n        note?: string\n    }}\n}}"
        )
    };
    assert!(
        check(&op(
            "kind == \"a\" or kind in [\"b\"] or count == 3 or has note"
        ))
        .is_empty()
    );
    expect(
        &check(&op("has storeId")),
        "warning: permissions.always-true: `has storeId` is always true",
    );
    expect(
        &check(&op("has nothing")),
        "permissions.condition: `when` reads `nothing`, but the input has no member `nothing`; `when` reads input members only",
    );
    expect(
        &check(&op("kind == \"c\"")),
        "\"c\" is not a value of `Kind`",
    );
    expect(
        &check(&op("count == \"3\"")),
        "`count` is a Integer value and cannot be compared with \"3\"",
    );
    expect(
        &check(&op("note == { a: 1 }")),
        "a condition compares with a string, a number, `true` or `false`",
    );
    expect(&check(&op("kind in []")), "`kind in []` is never true");
}

#[test]
fn filters_belongs_on_a_listing_once() {
    let found = check(
        "operation Touch on Store {\n    http POST \"/touch/{storeId}\"\n    filters read on Order\n    input { $storeId }\n}",
    );
    expect(&found, "permissions.filters: `filters` answers a listing");
    let list = |clauses: &str| {
        format!(
            "readonly operation ListOrders lists Order {{\n    http GET \"/stores/{{storeId}}/orders\"\n    {clauses}\n    input {{ $storeId }}\n}}"
        )
    };
    assert!(check(&list("filters read on Order")).is_empty());
    expect(
        &check(&list("filters read on Order\n    filters read on Order")),
        "permissions.duplicate-clause",
    );
    expect(
        &check(&list(
            "requires read on Order(*)\n    filters read on Order",
        )),
        "warning: permissions.filters: `ListOrders` both filters and requires a permission on `Order`",
    );
}

#[test]
fn a_collection_operation_names_its_target() {
    let create = |clause: &str| {
        format!(
            "operation CreateOrder creates Order {{\n    http POST \"/stores/{{storeId}}/orders\"\n    {clause}\n    input {{ $storeId }}\n}}"
        )
    };
    expect(
        &check(&create("requires read")),
        "permissions.target: `CreateOrder` is a collection operation, so it has no `Order` instance: write `on Order(*)`",
    );
    expect(
        &check(&create("requires read on Order")),
        "a collection operation has no instance: write `Order(*)`",
    );
    assert!(check(&create("requires read on Order(*)")).is_empty());
    assert!(check(&create("requires manage on Store")).is_empty());
    expect(
        &check("readonly operation Loose {\n    http GET \"/loose\"\n    requires read\n}"),
        "permissions.target: `Loose` is attached to no resource",
    );
}

#[test]
fn a_handler_requirement_takes_no_bindings_or_condition() {
    let op = |clause: &str| {
        format!(
            "operation X on Store {{\n    http POST \"/x/{{storeId}}\"\n    requires read\n    /// Only sometimes.\n    {clause}\n    input {{ $storeId }}\n}}"
        )
    };
    assert!(check(&op("requires manage on Store by handler")).is_empty());
    expect(
        &check(&op(
            "requires manage on Store(storeId: $storeId) by handler",
        )),
        "permissions.handler: `by handler` takes no bindings and no `when`",
    );
    expect(
        &check(&op("requires manage on Store when has storeId by handler")),
        "permissions.handler",
    );
}

#[test]
fn a_second_404_is_a_warning_and_the_errors_import_is_needed() {
    let found = check(
        "error Missing {\n    status 404\n    message: string\n}\nreadonly operation Peek on Store {\n    http GET \"/peek/{storeId}\"\n    requires read\n    input { $storeId }\n    errors [Missing]\n}",
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    expect(
        &found,
        "warning: permissions.second-not-found: `Peek` also declares `Missing`, another 404",
    );
    let mut package = small(BASE);
    package.imports.clear();
    let compiled = compile(&package, None);
    let messages: Vec<&str> = compiled
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains(
            "permissions.not-found: a requirement attaches `crucible.errors#NotFoundException`"
        )),
        "{messages:#?}"
    );
}

#[test]
fn a_root_wildcard_has_nothing_to_hide() {
    let found = check_source(&BASE.replace(
        "operation ManageStore on Store {",
        "operation CreateStore on Store collection {\n    http POST \"/stores\"\n    requires manage on Store(*)\n}\n\noperation ManageStore on Store {",
    ));
    assert!(found.is_empty(), "{found:#?}");
    let compiled = compile(
        &small(&BASE.replace(
            "operation ManageStore on Store {",
            "operation CreateStore on Store collection {\n    http POST \"/stores\"\n    requires manage on Store(*)\n}\n\noperation ManageStore on Store {",
        )),
        None,
    );
    let service = compiled.service.unwrap();
    let create = service
        .operations
        .iter()
        .find(|op| op.id.name() == "CreateStore")
        .unwrap();
    assert!(create.errors.is_empty(), "{:?}", create.errors);
}

#[test]
fn permission_types_are_checked_both_ways() {
    let types = |body: &str| {
        check(&format!(
            "@permissionTypes{body}\nstructure Uses {{ kind: Kinds }}"
        ))
    };
    assert!(types(" enum Kinds {\n    store = \"store\"\n    order = \"order\"\n}").is_empty());
    expect(
        &types(" enum Kinds {\n    store = \"store\"\n}"),
        "permissions.types: `Order` has grantable permissions but \"order\" is not a value",
    );
    assert!(types("(exclude: [Order]) enum Kinds {\n    store = \"store\"\n}").is_empty());
    expect(
        &types(
            " enum Kinds {\n    store = \"store\"\n    order = \"order\"\n    shop = \"shop\"\n}",
        ),
        "\"shop\" is not the authorization name of a resource that declares permissions",
    );
    expect(
        &types("(exclude: [Order]) enum Kinds {\n    store = \"store\"\n    order = \"order\"\n}"),
        "`Order` is excluded but \"order\" is one of the values",
    );
    expect(
        &check("@permissionTypes\nstructure NotAnEnum { a: string }"),
        "`@permissionTypes` applies to an enum",
    );
    expect(
        &check(
            "@permissionTypes\nenum A {\n    store = \"store\"\n    order = \"order\"\n}\n@permissionTypes\nenum B {\n    store = \"store\"\n    order = \"order\"\n}",
        ),
        "a service has one `@permissionTypes` enum",
    );
}

#[test]
fn an_identifier_no_requirement_covers_is_advice() {
    let found = check(
        "operation Copy on Store {\n    http POST \"/copy/{storeId}\"\n    requires manage\n    input {\n        $storeId\n        orderId: OrderId\n    }\n}",
    );
    assert_eq!(
        found,
        [
            "advice: permissions.unchecked-reference: the input binds `orderId` of `Order`, and no requirement covers it; decide whether `Copy` needs a permission on it"
        ]
    );
}

#[test]
fn the_same_value_on_two_resources_is_allowed() {
    let source = BASE.replace(
        "permission read = \"order:read\"",
        "permission read = \"store:read\"",
    );
    assert!(
        check_source(&source).is_empty(),
        "{:#?}",
        check_source(&source)
    );
}

#[test]
fn diagnostics_point_at_the_clause_and_the_entry() {
    let source = format!(
        "{BASE}\nreadonly operation X reads Order {{\n    http GET \"/x/{{storeId}}/{{orderId}}\"\n    requires read\n    requires raed\n    input {{\n        $storeId\n        $orderId\n    }}\n}}\n"
    );
    let source = source.replace(
        "permission manage = \"store:manage\"",
        "permission manage = \"Store:Manage\"",
    );
    let compiled = compile(&small(&source), None);
    let at = |code: &str| {
        let d = compiled
            .diagnostics
            .iter()
            .find(|d| d.diagnostic.message.starts_with(code))
            .unwrap_or_else(|| panic!("{code}: {}", compiled.render()));
        d.diagnostic.span.text(&source).to_owned()
    };
    assert_eq!(at("permissions.unknown"), "requires raed");
    assert_eq!(
        at("permissions.value"),
        "permission manage = \"Store:Manage\""
    );
}

// ---- the release guard ------------------------------------------------------

/// The guard of `current` against `BASE` frozen as release "1".
fn guarded(name: &str, current: &str) -> crucible_lang::Guard {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("permissions-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let base = compile(&small(BASE), None).service.expect("base");
    crucible_lang::release::freeze(&dir, &base, "test").unwrap();
    let path = dir.join("1.json");
    let mut package = small(current);
    package.baseline = Some(path);
    let compiled = compile(&package, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    crucible_lang::guard(&package, &compiled, 1, &crucible_policy::Config::default())
        .expect("a baseline")
        .expect("it loads")
}

#[test]
fn the_guard_refuses_a_changed_value_and_reports_a_new_requirement() {
    let changed = guarded(
        "value",
        &BASE.replace("\"store:manage\"", "\"store:admin\""),
    );
    assert!(changed.has_errors());
    assert!(changed.undeclared.iter().any(|f| {
        f.rule == "compatibility.permissions"
            && f.shape == "example.shop#Store$manage"
            && f.message
                .contains("`manage` changed its value from \"store:manage\" to \"store:admin\"")
    }));
    let tightened = guarded(
        "tightened",
        &BASE.replace(
            "    requires read\n    input {\n        $storeId\n        $orderId",
            "    requires read\n    requires manage on Store\n    input {\n        $storeId\n        $orderId",
        ),
    );
    assert!(!tightened.has_errors(), "{:#?}", tightened.undeclared);
    assert!(tightened.undeclared.iter().any(|f| {
        f.rule == "compatibility.authorization"
            && f.severity == crucible_policy::Severity::Warning
            && f.message
                .contains("`GetOrder` now requires \"store:manage\" on `Store`")
    }));
}

#[test]
fn renames_that_keep_stored_grants_pass_the_guard() {
    // The shape is renamed and keeps its authorization name; the permission
    // is renamed and keeps its old value as an alias the requirement moved off.
    let renamed = BASE
        .replace("resource Order in Store {", "@authorizationName(\"order\")\nresource Purchase in Store {")
        .replace("reads Order", "reads Purchase")
        .replace(
            "    permission read = \"order:read\"\n}",
            "    permission view = \"order:view\"\n    @deprecated @aliasOf(view)\n    permission read = \"order:read\"\n}",
        )
        .replace(
            "    requires read\n    input {\n        $storeId\n        $orderId",
            "    requires view\n    input {\n        $storeId\n        $orderId",
        );
    let guard = guarded("renamed", &renamed);
    let permission_findings: Vec<_> = guard
        .undeclared
        .iter()
        .filter(|f| {
            f.rule == "compatibility.permissions" || f.rule == "compatibility.authorization"
        })
        .collect();
    assert!(permission_findings.is_empty(), "{permission_findings:#?}");
    // Renaming the shape without the override breaks them.
    let bare = renamed.replace("@authorizationName(\"order\")\n", "");
    let guard = guarded("renamed-bare", &bare);
    assert!(guard.undeclared.iter().any(|f| {
        f.rule == "compatibility.permissions"
            && f.message
                .contains("a renamed resource keeps them with `@authorizationName(\"order\")`")
    }));
}
