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
                _ => "other",
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

/// A literal is a value of the member's type, so every generator can state
/// the comparison; arbitrary precision is never compared.
#[test]
fn compared_numbers_fit_the_member_type() {
    let op = |when: &str| {
        format!(
            "operation X on Store {{
    http POST \"/x/{{storeId}}\"
    requires manage when {when}
    input {{
        $storeId
        small?: byte
        ratio?: float
        exact?: double
        big?: bigInteger
        precise?: bigDecimal
    }}
}}"
        )
    };
    assert!(check(&op("small == 127 or ratio == 0.1 or exact == 1e300")).is_empty());
    expect(
        &check(&op("small == 128")),
        "`small` is a Byte value and cannot be compared with 128",
    );
    expect(
        &check(&op("ratio == 1e300")),
        "`ratio` is a Float value and cannot be compared with 1e+300",
    );
    expect(
        &check(&op("big == 1")),
        "`big` is a BigInteger value, which a condition cannot compare; test it with `has`",
    );
    expect(
        &check(&op("precise == 1.5")),
        "`precise` is a BigDecimal value, which a condition cannot compare",
    );
    assert!(check(&op("has big and has precise")).is_empty());
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
    // Documentation does not make a clause another one.
    expect(
        &check(&list(
            "/// Seen by anyone.\n    filters read on Order\n    /// Seen twice.\n    filters read on Order",
        )),
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

#[test]
fn a_binding_never_passes_through_a_map_or_a_map_of_structures() {
    let found = check(
        "structure Line { orderId: OrderId }\nmap ByName: string -> Line\nmap Refs: string -> OrderId\noperation Batch on Store {\n    http POST \"/batch/{storeId}\"\n    requires read on Order(refs)\n    requires read on Order(byName.orderId)\n    input {\n        $storeId\n        refs: Refs\n        byName: ByName\n    }\n}",
    );
    expect(
        &found,
        "permissions.list-binding: `orderId` of `Order` is bound to `refs`, which passes through the map `refs`",
    );
    expect(
        &found,
        "permissions.list-binding: `orderId` of `Order` is bound to `byName.orderId`, which passes through the map `byName`",
    );
}

#[test]
fn a_label_binding_names_an_identifier_the_input_binds() {
    let op = |target: &str| {
        format!(
            "readonly operation X on Store {{\n    http GET \"/x/{{storeId}}\"\n    requires read on {target}\n    input {{\n        $storeId\n        @query ref: OrderId\n    }}\n}}"
        )
    };
    // `$storeId` binds an identifier of the operation's resource.
    assert!(check(&op("Order(storeId: $storeId, orderId: ref)")).is_empty());
    // `ref` is a member, not an identifier the input binds: write its path.
    expect(
        &check(&op("Order(storeId: $storeId, orderId: $ref)")),
        "error: permissions.binding: `$ref` names an identifier the input binds, and the input binds `$storeId`; bind any other member by its path: `ref`",
    );
    // An operation bound to no resource binds no identifier.
    expect(
        &check(
            "readonly operation Loose {\n    http GET \"/loose/{storeId}\"\n    requires read on Store(storeId: $storeId)\n    input { storeId: StoreId }\n}",
        ),
        "`$storeId` names an identifier the input binds, and the input binds no identifier",
    );
}

#[test]
fn checking_another_instance_than_the_handler_loads_is_a_warning() {
    let op = |clauses: &str| {
        format!(
            "readonly operation Peek on Store {{\n    http GET \"/peek/{{storeId}}\"\n    {clauses}\n    input {{\n        $storeId\n        @query other: StoreId\n    }}\n}}"
        )
    };
    let found = check(&op("requires read on Store(other)"));
    assert_eq!(
        found,
        [
            "warning: permissions.other-instance: `storeId` of `Store` is bound to `other`, but `Peek` is bound to `Store` through `storeId`: the check authorizes another instance than the handler loads; bind `storeId`, or also require a permission on the instance the operation is bound to"
        ]
    );
    // Checking the loaded instance too makes the second one deliberate.
    assert!(check(&op("requires read\n    requires manage on Store(other)")).is_empty());
    // A child's parent identifier counts the same.
    let found = check(
        "readonly operation Look reads Order {\n    http GET \"/look/{storeId}/{orderId}\"\n    requires read on Order(storeId: other, orderId: $orderId)\n    input {\n        $storeId\n        $orderId\n        @query other: StoreId\n    }\n}",
    );
    expect(
        &found,
        "warning: permissions.other-instance: `storeId` of `Order` is bound to `other`, but `Look` is bound to `Order` through `storeId`",
    );
}

#[test]
fn authorization_name_and_permission_annotations_need_permissions() {
    let found = check_source(&BASE.replace(
        "readonly operation Ping {",
        "@authorizationName(\"thing\")\nresource Thing {\n    id thingId: StoreId\n}\n\nreadonly operation Ping {",
    ));
    expect(
        &found,
        "error: permissions.name: `@authorizationName` names `Thing` in grants, but it declares no permissions",
    );
    let found = check(
        "enum Color {\n    @permissionOnly\n    red = \"red\"\n}\nstructure Paint { color: Color }",
    );
    assert!(
        found
            .iter()
            .any(|m| m.starts_with("error: ") && m.contains("permissionOnly")),
        "{found:#?}"
    );
}

#[test]
fn a_public_operation_is_marked_public_in_the_ir() {
    let compiled = compile(
        &small(&BASE.replace(
            "readonly operation Ping {\n    http GET \"/ping\"\n    requires none\n}",
            "public readonly operation Ping {\n    http GET \"/ping\"\n}",
        )),
        None,
    );
    let service = compiled.service.expect("compiles");
    let public: Vec<(&str, bool)> = service
        .operations
        .iter()
        .map(|op| (op.id.name(), op.public))
        .collect();
    assert!(public.contains(&("Ping", true)), "{public:?}");
    assert!(public.contains(&("GetOrder", false)), "{public:?}");
}

/// The guard of `current` against `base` frozen as release "1".
fn guarded_from(name: &str, base: &str, current: &str) -> crucible_lang::Guard {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("permissions-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let compiled = compile(&small(base), None);
    let frozen = compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    crucible_lang::release::freeze(&dir, &frozen, "test").unwrap();
    let mut package = small(current);
    package.baseline = Some(dir.join("1.json"));
    let compiled = compile(&package, None);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    crucible_lang::guard(&package, &compiled, 1, &crucible_policy::Config::default())
        .expect("a baseline")
        .expect("it loads")
}

#[test]
fn declaring_requirements_on_a_released_operation_is_reported_not_refused() {
    let audit = |clause: &str| {
        format!(
            "{BASE}\noperation Audit on Store {{\n    http POST \"/audit/{{storeId}}\"\n    {clause}\n    input {{ $storeId }}\n}}\n"
        )
    };
    // Pending, then a requirement: NotFoundException comes with it.
    let guard = guarded_from(
        "pending",
        &audit("requires pending because \"the handler checks\""),
        &audit("requires manage"),
    );
    assert!(!guard.has_errors(), "{:#?}", guard.undeclared);
    assert!(guard.undeclared.iter().any(|f| {
        f.rule == "compatibility.authorization"
            && f.message.contains(
                "`Audit` now requires \"store:manage\" on `Store` (it was `requires pending`)",
            )
    }));
    // A release from before the service declared any permission.
    let bare: String = BASE
        .lines()
        .filter(|line| !line.contains("permission ") && !line.contains("requires "))
        .collect::<Vec<_>>()
        .join("\n");
    let guard = guarded_from("undeclared", &bare, BASE);
    assert!(!guard.has_errors(), "{:#?}", guard.undeclared);
    assert!(guard.undeclared.iter().any(|f| {
        f.rule == "compatibility.authorization"
            && f.message
                .contains("`GetOrder` now requires \"order:read\" on `Order` (it declared none)")
    }));
    assert!(
        !guard
            .undeclared
            .iter()
            .any(|f| f.rule == "compatibility.error" || f.rule == "compatibility.sdk"),
        "{:#?}",
        guard.undeclared
    );
}

#[test]
fn swapping_two_values_is_refused_and_moves_the_requirement() {
    let base = BASE.replace(
        "    requires manage\n    requires read\n",
        "    requires manage\n",
    );
    let swapped = base
        .replace("\"store:read\"", "\"store:tmp\"")
        .replace("\"store:manage\"", "\"store:read\"")
        .replace("\"store:tmp\"", "\"store:manage\"");
    let guard = guarded_from("swap", &base, &swapped);
    assert!(guard.has_errors());
    assert!(guard.undeclared.iter().any(|f| {
        f.rule == "compatibility.permissions"
            && f.message.contains(
                "grants stored under \"store:manage\" satisfied `manage` and now satisfy `read`",
            )
    }));
    // `requires manage` names the same symbol, which now checks "store:read".
    assert!(
        guard.undeclared.iter().any(|f| {
            f.rule == "compatibility.authorization"
                && f.message
                    .contains("`ManageStore` now requires \"store:read\" on `Store`")
        }),
        "{:#?}",
        guard.undeclared
    );
}

#[test]
fn advice_without_a_source_location_renders_as_advice() {
    let mut compiled = compile(&small(BASE), None);
    compiled.diagnostics.push(crucible_lang::FileDiagnostic {
        file: None,
        diagnostic: crucible_lang::Diagnostic::advice(Default::default(), "decide"),
    });
    compiled.diagnostics.push(crucible_lang::FileDiagnostic {
        file: None,
        diagnostic: crucible_lang::Diagnostic::warning(Default::default(), "look"),
    });
    assert_eq!(
        compiled.render(),
        "example.shop: advice: decide\nexample.shop: warning: look\n"
    );
}

// ---- a service's own not-found error ----------------------------------------

/// A service with errors of its own, named like the bundled ones, so it
/// cannot import `crucible.errors`: Pigeon's shape. `notFound` designates its
/// 404, which check 17 attaches instead of the framework's.
const OWN_ERRORS: &str = r#"
service Shop {
    version "1"
    protocol json
    auth bearer
    notFound ResourceNotFoundException
    resources [Store]
    errors [ValidationException, InternalServerException]
}

string StoreId { pattern "^s_" }
string OrderId { pattern "^o_" }

enum ErrorResourceType {
    STORE = "store"
    ORDER = "order"
}

error ValidationException {
    status 400
    message: string
}

error InternalServerException {
    status 500
    message: string
}

/// The request names something that does not exist, or not for this caller.
error ResourceNotFoundException {
    status 404
    message: string
    resourceType: ErrorResourceType
    resourceId: string
    requestId?: string
}

resource Store {
    id storeId: StoreId
    permission read = "store:read"
    permission manage = "store:manage"
}

resource Order in Store {
    id orderId: OrderId
    permission read = "order:read"
}

readonly operation GetOrder on Order {
    http GET "/stores/{storeId}/orders/{orderId}"
    requires read
    input {
        $storeId
        $orderId
    }
    output { note?: string }
}

readonly operation ListOrders on Order collection {
    http GET "/stores/{storeId}/orders"
    filters read on Order
    input { $storeId }
    output { ids: OrderIds }
}

list OrderIds: OrderId

operation CreateStore on Store collection {
    http POST "/stores"
    requires manage on Store(*)
}

readonly operation GetStore on Store {
    http GET "/stores/{storeId}"
    requires read
    input { $storeId }
}
"#;

/// `text` alone: no `crucible.errors` import.
fn own(text: &str) -> Compiled {
    compile(
        &Package::new(
            "example.shop",
            vec![SourceFile {
                path: "shop.cru".into(),
                text: text.to_owned(),
            }],
        ),
        None,
    )
}

fn messages(compiled: &Compiled) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .map(|d| d.diagnostic.message.clone())
        .collect()
}

#[test]
fn a_service_with_its_own_errors_designates_its_404_and_declares_requirements() {
    let compiled = own(OWN_ERRORS);
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let service = compiled.service.expect("service");
    let not_found = ShapeId::new("example.shop", "ResourceNotFoundException");
    assert_eq!(service.not_found.as_ref(), Some(&not_found));
    assert_eq!(service.not_found_error(), not_found);
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
    // Check 17 attaches the service's own 404, after the operation's own
    // errors and before the service's; nothing of `crucible.errors`.
    for operation in ["GetOrder", "ListOrders"] {
        assert_eq!(
            errors(operation),
            [
                "example.shop#ResourceNotFoundException",
                "example.shop#ValidationException",
                "example.shop#InternalServerException",
            ],
            "{operation}"
        );
    }
    // A root resource with `*` has nothing to hide.
    assert!(!errors("CreateStore").iter().any(|e| e.contains("NotFound")));
    assert!(
        service
            .shapes
            .keys()
            .all(|id| id.namespace() != "crucible.errors")
    );
    // The designation survives a snapshot, and the IR alone checks clean.
    let json = serde_json::to_value(&service).unwrap();
    assert_eq!(
        json["not_found"],
        json!("example.shop#ResourceNotFoundException")
    );
    let read: Service = serde_json::from_value(json).unwrap();
    assert_eq!(read.not_found, service.not_found);
    assert!(crucible_smithy::validate::permissions_from_ir(&read).is_empty());
}

#[test]
fn an_operation_that_declares_the_designated_404_is_not_warned() {
    let compiled = own(&OWN_ERRORS.replace(
        "    output { note?: string }\n",
        "    output { note?: string }\n    errors [ResourceNotFoundException]\n",
    ));
    assert!(compiled.diagnostics.is_empty(), "\n{}", compiled.render());
    let service = compiled.service.unwrap();
    let get = service
        .operations
        .iter()
        .find(|op| op.id.name() == "GetOrder")
        .unwrap();
    let not_found = get
        .errors
        .iter()
        .filter(|e| e.name() == "ResourceNotFoundException")
        .count();
    assert_eq!(not_found, 1, "{:?}", get.errors);
    // Another 404 beside it is warned about, naming the designated one.
    let compiled = own(&OWN_ERRORS.replace(
        "    output { note?: string }\n",
        "    output { note?: string }\n    errors [Gone]\n}\n\nerror Gone {\n    status 404\n    message: string\n",
    ));
    let found = messages(&compiled);
    assert!(
        found.iter().any(|m| m.starts_with(
            "permissions.second-not-found: `GetOrder` also declares `Gone`, another 404; report genuine absence with the service's not-found error `ResourceNotFoundException`"
        )),
        "{found:#?}"
    );
}

#[test]
fn without_a_designation_a_service_that_imports_nothing_cannot_require() {
    let compiled = own(&OWN_ERRORS.replace("    notFound ResourceNotFoundException\n", ""));
    let found = messages(&compiled);
    assert!(
        found.iter().any(|m| m.starts_with(
            "permissions.not-found: a requirement attaches `crucible.errors#NotFoundException`, which the model does not contain: import the `crucible.errors` package, or designate the service's own 404 with `notFound` in the service"
        )),
        "{found:#?}"
    );
}

#[test]
fn the_designated_error_is_a_404_client_error_of_the_package() {
    let refused = |source: String, needle: &str| {
        let compiled = own(&source);
        assert!(compiled.service.is_none(), "{needle}");
        let found = messages(&compiled);
        assert!(
            found.iter().any(|m| m.contains(needle)),
            "no diagnostic contains {needle:?}:\n{found:#?}"
        );
    };
    refused(
        OWN_ERRORS.replace(
            "    status 404\n    message: string\n    resourceType",
            "    status 410\n    message: string\n    resourceType",
        ),
        "permissions.not-found-error: `ResourceNotFoundException` answers 410; the service's not-found error is a client error with status 404",
    );
    refused(
        OWN_ERRORS.replace(
            "notFound ResourceNotFoundException",
            "notFound ErrorResourceType",
        ),
        "permissions.not-found-error: `ErrorResourceType` is not an error",
    );
    refused(
        OWN_ERRORS.replace("notFound ResourceNotFoundException", "notFound Missing"),
        "permissions.not-found-error: `Missing` is not an error",
    );
    refused(
        OWN_ERRORS.replace(
            "notFound ResourceNotFoundException",
            "notFound ResourceNotFoundException\n    notFound ValidationException",
        ),
        "a service has one `notFound` clause",
    );
    // One of an imported package's errors is not the service's own.
    let package = small(&BASE.replace(
        "    auth bearer\n",
        "    auth bearer\n    notFound NotFoundException\n",
    ));
    let compiled = compile(&package, None);
    let found = messages(&compiled);
    assert!(
        found.iter().any(|m| m.contains(
            "permissions.not-found-error: `NotFoundException` is declared in `crucible.errors`; the service's not-found error is an error of its own package"
        )),
        "{found:#?}"
    );
}

#[test]
fn an_ir_designation_is_checked_without_lowering() {
    let mut service = own(OWN_ERRORS).service.unwrap();
    service.not_found = Some(ShapeId::new("example.shop", "ValidationException"));
    let found: Vec<String> = crucible_smithy::validate::permissions_from_ir(&service)
        .iter()
        .map(|f| f.text())
        .collect();
    assert!(
        found.iter().any(|m| m
            == "permissions.not-found-error: `ValidationException` answers 400; the service's not-found error is a client error with status 404"),
        "{found:#?}"
    );
    // The operations carry the 404 lowering attached; it is no longer the
    // one the service names, and both say so.
    service.not_found = Some(ShapeId::new("example.shop", "Elsewhere"));
    let found: Vec<String> = crucible_smithy::validate::permissions_from_ir(&service)
        .iter()
        .map(|f| f.text())
        .collect();
    assert_eq!(
        found,
        [
            "permissions.not-found-error: `example.shop#Elsewhere`, the service's not-found error, is not in the model"
        ]
    );
}

/// `.cru` cannot write an empty requirement list, and the client projection
/// turns one emptied of internal clauses into `Internal`, so only an IR
/// built some other way (a hand-edited release snapshot) carries one. The
/// IR checks still refuse it rather than read it as `requires none`.
#[test]
fn an_empty_requirement_list_in_the_ir_is_refused() {
    let (mut service, _) = pigeon();
    let operation = service
        .operations
        .iter_mut()
        .find(|op| matches!(op.requirements, Requirements::List(_)))
        .expect("an operation with requirements");
    let name = operation.id.name().to_owned();
    operation.requirements = Requirements::List(Vec::new());
    let found: Vec<String> = crucible_smithy::validate::permissions_from_ir(&service)
        .iter()
        .map(|f| f.text())
        .filter(|text| text.starts_with("permissions.requires:"))
        .collect();
    assert_eq!(
        found,
        [format!(
            "permissions.requires: `{name}` lists no requirement; an operation that needs no permission says `requires none`"
        )]
    );
}

// ---- review follow-ups (#257) -------------------------------------------------

#[test]
fn a_resource_with_permissions_declares_its_own_identifier() {
    // Without one, `Settings(*)` would bind `storeId`: every store.
    let found = check(
        "resource Settings in Store {\n    readonly property theme?: string\n    permission read = \"settings:read\"\n}\nreadonly operation Look on Store {\n    http GET \"/look/{storeId}\"\n    requires read on Settings(*)\n    input { $storeId }\n}",
    );
    expect(
        &found,
        "error: permissions.identifier: `Settings` declares permissions but no identifier of its own, so a check on it could name only its parent `Store`, and `Settings(*)` every one of them",
    );
    // A root resource with no identifier at all, reached or not.
    let found = check(
        "resource Everything {\n    readonly property theme?: string\n    permission read = \"everything:read\"\n}",
    );
    expect(
        &found,
        "error: permissions.identifier: `Everything` declares permissions but no identifier;",
    );
    // An IR alone, such as a release snapshot, is checked too.
    let (mut service, _) = pigeon();
    service
        .permissions
        .get_mut(&id("Workspace"))
        .unwrap()
        .identifiers
        .clear();
    let found: Vec<String> = crucible_smithy::validate::permissions_from_ir(&service)
        .iter()
        .map(|f| f.text())
        .collect();
    assert!(
        found.iter().any(|m| m
            == "permissions.identifier: `Workspace` declares permissions but has no identifier; a permission is held on an instance, named by its identifiers"),
        "{found:#?}"
    );
}

#[test]
fn an_alias_and_an_authorization_name_are_given_once() {
    let found = check_source(&BASE.replace(
        "    permission manage = \"store:manage\"\n",
        "    permission manage = \"store:manage\"\n    @deprecated @aliasOf(read) @aliasOf(manage)\n    permission old = \"store:old\"\n",
    ));
    expect(
        &found,
        "error: `@aliasOf` is given twice; an alias has one primary",
    );
    let found = check_source(&BASE.replace(
        "resource Store {",
        "@authorizationName(\"shop\") @authorizationName(\"store\")\nresource Store {",
    ));
    expect(
        &found,
        "error: `@authorizationName` is given twice; a resource has one authorization name",
    );
}

#[test]
fn an_operation_the_service_does_not_reach_is_checked_too() {
    // `Stray` is listed nowhere: the shared checks still read its clauses.
    let found = check_source(&format!(
        "{BASE}\nreadonly operation Stray {{\n    http GET \"/stray\"\n    requires nope on Store\n}}\n"
    ));
    expect(
        &found,
        "error: permissions.unknown: `Store` declares no permission `nope`",
    );
    expect(
        &check_source(&format!(
            "{BASE}\nreadonly operation Quiet {{\n    http GET \"/quiet\"\n}}\n"
        )),
        "error: permissions.undeclared: `Quiet` says nothing about authorization",
    );
    // Its clause may name a resource the service does not reach either.
    let draft = |clause: &str| {
        format!(
            "{BASE}\nstring DraftId {{ pattern \"^d_\" }}\nresource Draft {{\n    id draftId: DraftId\n    permission read = \"draft:read\"\n}}\nreadonly operation ReadDraft reads Draft {{\n    http GET \"/drafts/{{draftId}}\"\n    {clause}\n    input {{ $draftId }}\n}}\n"
        )
    };
    let found = check_source(&draft("requires read"));
    assert!(found.iter().all(|m| !m.starts_with("error")), "{found:#?}");
    expect(
        &check_source(&draft("requires write")),
        "error: permissions.unknown: `Draft` declares no permission `write`",
    );
    // A service without permissions does not ask a draft to say anything.
    let quiet = "service Quiet {\n    version \"1\"\n    protocol json\n    auth bearer\n    operations [Ping]\n}\nreadonly operation Ping {\n    http GET \"/ping\"\n}\nstring DraftId { pattern \"^d_\" }\nresource Draft {\n    id draftId: DraftId\n    permission read = \"draft:read\"\n}\nreadonly operation ReadDraft reads Draft {\n    http GET \"/drafts/{draftId}\"\n    input { $draftId }\n}\n";
    let found = check_source(quiet);
    assert!(
        found.iter().all(|m| !m.contains("permissions.undeclared")),
        "{found:#?}"
    );
}

#[test]
fn a_condition_tests_a_list_or_map_for_presence() {
    let op = |when: &str| {
        format!(
            "map Notes: string -> string\noperation Tag on Store {{\n    http POST \"/tag/{{storeId}}\"\n    requires manage when {when}\n    input {{\n        $storeId\n        tags?: OrderIds\n        notes?: Notes\n    }}\n}}"
        )
    };
    assert_eq!(check(&op("has tags or has notes")), Vec::<String>::new());
    // It still reads no element, and compares no list.
    expect(
        &check(&op("tags.x == \"a\"")),
        "permissions.list-binding: `when` reads `tags.x`, which passes through the list `tags`",
    );
    expect(
        &check(&op("tags == \"a\"")),
        "`tags` is not a scalar or enum",
    );
}

#[test]
fn a_binding_type_is_checked_when_nothing_else_uses_the_identifier_type() {
    // `ShelfId` reaches the IR through no member, and `ShelfRef` repeats
    // its constraints: a warning, as with any other identifier type.
    let source = |member: &str| {
        format!(
            "{}\nstring ShelfId {{ pattern \"^sh_\" }}\nstring ShelfRef {{ pattern \"^sh_\" }}\nresource Shelf in Store {{\n    id shelfId: ShelfId\n    permission read = \"shelf:read\"\n}}\nreadonly operation Peek {{\n    http GET \"/peek/{{storeId}}\"\n    requires read on Shelf(storeId: storeId, shelfId: ref)\n    input {{\n        storeId: StoreId\n        @query ref: {member}\n    }}\n}}\n",
            BASE.replace("operations [Ping]", "operations [Ping, Peek]")
        )
    };
    let found = check_source(&source("ShelfRef"));
    assert_eq!(found.len(), 1, "{found:#?}");
    expect(
        &found,
        "warning: permissions.binding-type: `shelfId` of `Shelf` is bound to `ref`, a `ShelfRef`; the identifier is a `ShelfId` with the same constraints",
    );
    // The identifier type stays out of the IR.
    let compiled = compile(&small(&source("ShelfRef")), None);
    let service = compiled.service.expect("service");
    assert!(
        service
            .shape(&ShapeId::new("example.shop", "ShelfId"))
            .is_none()
    );
}

#[test]
fn a_constrained_string_does_not_bind_a_plain_string_identifier() {
    let source = |member: &str| {
        format!(
            "{}\nresource Tag in Store {{\n    id tagId: string\n    permission read = \"tag:read\"\n}}\nreadonly operation Peek {{\n    http GET \"/peek/{{storeId}}\"\n    requires read on Tag(storeId: storeId, tagId: ref)\n    input {{\n        storeId: StoreId\n        @query ref: {member}\n    }}\n}}\n",
            BASE.replace("operations [Ping]", "operations [Ping, Peek]")
        )
    };
    assert_eq!(check_source(&source("string")), Vec::<String>::new());
    expect(
        &check_source(&source("OrderRef")),
        "error: permissions.binding: `tagId` of `Tag` is bound to `ref`, a `OrderRef`; the identifier is a `String`; bind a member of that type",
    );
    expect(
        &check_source(&format!("{}\nstring Loose\n", source("Loose"))),
        "warning: permissions.binding-type",
    );
}

#[test]
fn a_listing_filters_the_resource_it_lists() {
    let found = check(
        "readonly operation ListOrders lists Order {\n    http GET \"/stores/{storeId}/orders\"\n    filters read on Store\n    input { $storeId }\n}",
    );
    expect(
        &found,
        "error: permissions.filters: `ListOrders` is bound to `Order`, so `filters` names a permission of `Order`, the resource the listing returns; a permission on `Store` is a `requires`",
    );
}

#[test]
fn none_and_pending_are_not_permission_symbols() {
    let found = check_source(&BASE.replace(
        "    permission manage = \"store:manage\"\n",
        "    permission manage = \"store:manage\"\n    permission none = \"store:none\"\n    permission pending = \"store:pending\"\n",
    ));
    expect(
        &found,
        "error: permissions.symbol: `none` cannot name a permission: `requires none` is the clause that says an operation needs no permission",
    );
    expect(
        &found,
        "error: permissions.symbol: `pending` cannot name a permission",
    );
    // `requires none` takes no target, condition or handler.
    for clause in [
        "requires none on Store",
        "requires none when has storeId",
        "requires none by handler",
    ] {
        let parsed = parse(&format!(
            "operation Op on Store {{\n    {clause}\n    input {{ $storeId }}\n}}\n"
        ));
        assert!(
            parsed.diagnostics.iter().any(|d| d.message
                == "`requires none` stands alone: it says the operation needs no permission, on any resource"),
            "{clause}: {:?}",
            parsed.diagnostics
        );
        let Decl::Operation(op) = &parsed.file.items[0].decl else {
            panic!()
        };
        assert!(matches!(
            &op.entries[0],
            OpEntry::Requirement(r) if matches!(r.kind, RequirementKind::None(_))
        ));
        assert!(
            matches!(op.entries[1], OpEntry::Body(_)),
            "{:?}",
            op.entries
        );
    }
    // `requires pending` needs its `because`.
    let parsed = parse("operation Op {\n    requires pending\n}\n");
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.message.contains("expected `because`")),
        "{:?}",
        parsed.diagnostics
    );
}

#[test]
fn a_permission_annotation_elsewhere_says_where_it_applies() {
    let found = check("@aliasOf(read)\nstructure Loose { a: string }");
    expect(
        &found,
        "error: `@aliasOf` applies to a `permission` entry of a resource, not to a structure",
    );
    let found = check("@authorizationName(\"x\")\nstructure Loose { a: string }");
    expect(
        &found,
        "error: `@authorizationName` applies to a resource that declares permissions, not to a structure",
    );
    assert!(
        found
            .iter()
            .all(|m| !m.contains("declare it with `metadata")),
        "{found:#?}"
    );
}

#[test]
fn a_condition_that_does_not_lower_leaves_its_clause_out() {
    let compiled = compile(
        &small(&BASE.replace(
            "    requires manage\n    requires read\n",
            "    requires manage when storeId == [1]\n    requires read\n",
        )),
        None,
    );
    assert!(compiled.has_errors());
    let service = compiled.best_effort.expect("best effort");
    let Requirements::List(clauses) = requirements(&service, "ManageStore") else {
        panic!()
    };
    // Not an unconditional `manage`.
    assert_eq!(clauses.len(), 1, "{clauses:?}");
    assert_eq!(clauses[0].permission, "read");
}

#[test]
fn members_named_has_and_not_are_read_in_a_condition() {
    let op = |when: &str| {
        format!(
            "operation X on Store {{\n    http POST \"/x/{{storeId}}\"\n    requires manage when {when}\n    input {{\n        $storeId\n        has?: boolean\n        not?: string\n    }}\n}}"
        )
    };
    for when in [
        "has == true",
        "not == \"a\"",
        "not in [\"a\", \"b\"]",
        "has has",
        "not has not",
        "has has and not not == \"a\"",
    ] {
        assert_eq!(check(&op(when)), Vec::<String>::new(), "{when}");
    }
    let parsed = parse("operation X {\n    requires r when not has == true\n}\n");
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Operation(op) = &parsed.file.items[0].decl else {
        panic!()
    };
    let OpEntry::Requirement(r) = &op.entries[0] else {
        panic!()
    };
    let RequirementKind::Requires {
        when: Some(when), ..
    } = &r.kind
    else {
        panic!()
    };
    let ConditionKind::Not(inner) = &when.kind else {
        panic!("{when:?}")
    };
    assert!(matches!(&inner.kind, ConditionKind::Equals(p, _) if p.parts[0].name == "has"));
}
