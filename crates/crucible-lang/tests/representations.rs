//! Resources and their representations (decisions 31, 34a and 34c-34h):
//! declared property mutability and the checks against it, property
//! presence, an identifier's spelling in views, `$*` views, the warnings for
//! a resource modelled beside its representation, and a child resource's
//! properties, which are its own.

use crucible_lang::ast::{Decl, MemberKind, ResourceEntryKind};
use crucible_lang::{Compiled, FormatOptions, Package, SourceFile, compile, format, parse};
use crucible_smithy::ir::{Member, Mutability, Service, Shape};

const SERVICE: &str = r#"
service Mail {
    version "1"
    protocol json
    auth bearer
    resources [Workspace]
}

string Text
string InboxId
string WorkspaceId
string MessageId
"#;

fn compiled(extra: &str) -> Compiled {
    let package = Package::new(
        "example.mail",
        vec![SourceFile {
            path: "mail.cru".to_owned(),
            text: format!("{SERVICE}\n{extra}"),
        }],
    );
    compile(&package, None)
}

/// Errors and warnings, apart.
fn split(extra: &str) -> (Vec<String>, Vec<String>) {
    let compiled = compiled(extra);
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for item in compiled.diagnostics {
        if item.diagnostic.is_error() {
            errors.push(item.diagnostic.message);
        } else {
            warnings.push(item.diagnostic.message);
        }
    }
    (errors, warnings)
}

fn service(extra: &str) -> Service {
    let compiled = compiled(extra);
    assert!(!compiled.has_errors(), "{}", compiled.render());
    compiled.service.expect("service")
}

fn members<'a>(service: &'a Service, shape: &str) -> &'a [Member] {
    match service.shapes.iter().find(|(id, _)| id.name() == shape) {
        Some((_, Shape::Structure(s))) => &s.members,
        _ => panic!("no structure {shape}"),
    }
}

fn member<'a>(service: &'a Service, shape: &str, name: &str) -> &'a Member {
    members(service, shape)
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("{shape} has no {name}"))
}

fn names(service: &Service, shape: &str) -> Vec<(String, bool)> {
    members(service, shape)
        .iter()
        .map(|m| (m.name.clone(), m.required))
        .collect()
}

fn assert_mentions(messages: &[String], needle: &str) {
    assert!(
        messages.iter().any(|m| m.contains(needle)),
        "no diagnostic mentions {needle:?}: {messages:#?}"
    );
}

/// A workspace with an inbox below it. Every property is written by the
/// operation its keyword asks for, so the model compiles without warnings.
const INBOX: &str = r#"
resource Workspace {
    id workspaceId: WorkspaceId as id
    property name: Text
    readonly property createdAt: Text
}

resource Inbox in Workspace {
    id inboxId: InboxId as id
    immutable property address: Text
    property name: Text
    property note?: Text
    readonly property createdAt: Text
    writeonly property secret: Text
    immutable writeonly property seed?: Text
}

structure WorkspaceData for Workspace { $* }

structure InboxData for Inbox {
    $workspaceId
    $*
    $note?
    $name?
}

structure InboxSummary for Inbox {
    $inboxId
    $address
}

operation CreateWorkspace creates Workspace {
    http POST "/workspaces"
    input { $name }
    output { workspace: WorkspaceData }
}

operation UpdateWorkspace updates Workspace {
    http PATCH "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $name?
    }
}

operation CreateInbox creates Inbox {
    http POST "/workspaces/{workspaceId}/inboxes"
    input {
        $workspaceId
        $address
        $name
        $secret
        $seed?
    }
    output { inbox: InboxData }
}

operation UpdateInbox updates Inbox {
    http PATCH "/workspaces/{workspaceId}/inboxes/{inboxId}"
    input {
        $workspaceId
        $inboxId
        $name?
        $note?
        $secret?
    }
    output {
        $inboxId
        $createdAt
    }
}

operation ListInboxes lists Inbox {
    http GET "/workspaces/{workspaceId}/inboxes"
    input {
        $workspaceId
        @query
        $name?
    }
    output { inboxes: InboxSummaries }
}

list InboxSummaries: InboxSummary
"#;

// ---- parsing and formatting --------------------------------------------------

#[test]
fn entries_parse_keywords_presence_and_spelling() {
    let parsed = parse(
        "resource R {\n    id rId: X as id\n    readonly property a?: Y\n    immutable writeonly property b: Z { length 1..2 }\n}\nstructure V for R { $*\n $a? }",
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Decl::Resource(resource) = &parsed.file.items[0].decl else {
        panic!("resource");
    };
    let [id, a, b] = resource.entries.as_slice() else {
        panic!("three entries");
    };
    assert_eq!(id.kind, ResourceEntryKind::Id);
    assert_eq!(id.alias.as_ref().map(|a| a.name.as_str()), Some("id"));
    assert!(!id.optional && id.modifiers.is_empty());
    assert!(a.optional);
    assert_eq!(a.modifiers[0].name, "readonly");
    let words: Vec<&str> = b.modifiers.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(words, ["immutable", "writeonly"]);
    assert_eq!(b.ty.constraints.len(), 1);
    let Decl::Structure(view) = &parsed.file.items[1].decl else {
        panic!("structure");
    };
    let MemberKind::All(span) = &view.members[0].kind else {
        panic!("$*");
    };
    assert_eq!(span.end - span.start, 2, "the span covers `$*`");
    assert!(matches!(
        &view.members[1].kind,
        MemberKind::Elided { optional: true, .. }
    ));
}

#[test]
fn star_takes_no_spelling_or_presence() {
    let parsed = parse("structure V for R {\n    $* as x\n    $*?\n}");
    let messages: Vec<&str> = parsed
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(
        messages
            .iter()
            .all(|m| m.contains("`$*` takes no `as` or `?`"))
    );
}

#[test]
fn a_broken_entry_does_not_hide_the_next() {
    // An unknown keyword, then a good entry: the parser resumes there.
    let parsed = parse("resource R {\n    mutable property a: X\n    readonly property b: Y\n}");
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    let Decl::Resource(resource) = &parsed.file.items[0].decl else {
        panic!("resource");
    };
    assert!(resource.entries.iter().any(|e| e.name.name == "b"));
}

#[test]
fn the_new_syntax_formats_and_round_trips() {
    let messy = "resource R {\nid rId: X as id\n  readonly property a?: Y\nimmutable writeonly property b: Z\n}\nstructure V for R {\n$*\n      $a?\n}\n";
    let formatted = format(messy, &FormatOptions::default());
    assert_eq!(
        formatted,
        "resource R {\n    id rId: X as id\n    readonly property a?: Y\n    immutable writeonly property b: Z\n}\nstructure V for R {\n    $*\n    $a?\n}\n"
    );
    assert_eq!(format(&formatted, &FormatOptions::default()), formatted);
    // The token stream is still lossless: `*` is a token of its own.
    let parsed = parse(&formatted);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

// ---- the model above ---------------------------------------------------------

#[test]
fn the_inbox_model_compiles_cleanly() {
    let (errors, warnings) = split(INBOX);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn star_expands_own_identifiers_then_readable_properties() {
    let service = service(INBOX);
    // `$workspaceId` is inherited, so `$*` leaves it out and the view names
    // it: under its identifier name, not Workspace's own spelling. Then
    // Inbox's identifier as `id`, its readable properties in declaration
    // order with their presence (`secret` and `seed` are write-only), and
    // `$note?` and `$name?` loosen where `$*` put them.
    assert_eq!(
        names(&service, "InboxData"),
        [
            ("workspaceId".to_owned(), true),
            ("id".to_owned(), true),
            ("address".to_owned(), true),
            ("name".to_owned(), false),
            ("note".to_owned(), false),
            ("createdAt".to_owned(), true),
        ]
    );
    assert_eq!(
        names(&service, "WorkspaceData"),
        [
            ("id".to_owned(), true),
            ("name".to_owned(), true),
            ("createdAt".to_owned(), true),
        ]
    );
}

#[test]
fn an_identifier_is_spelled_in_views_and_outputs_and_bound_by_name_in_inputs() {
    let service = service(INBOX);
    assert_eq!(names(&service, "InboxSummary")[0].0, "id");
    assert_eq!(names(&service, "UpdateInboxOutput")[0].0, "id");
    let input: Vec<String> = names(&service, "UpdateInboxInput")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(&input[..2], ["workspaceId", "inboxId"]);
}

#[test]
fn members_carry_the_declared_mutability() {
    let service = service(INBOX);
    let mutability = |shape: &str, name: &str| member(&service, shape, name).mutability;
    // Views and outputs carry the property's mutability.
    assert_eq!(
        mutability("InboxData", "address"),
        Some(Mutability::CreateAndRead)
    );
    assert_eq!(mutability("InboxData", "name"), Some(Mutability::Full));
    assert_eq!(mutability("InboxData", "createdAt"), Some(Mutability::Read));
    assert_eq!(
        mutability("UpdateInboxOutput", "createdAt"),
        Some(Mutability::Read)
    );
    // So do the writes of `creates`, `puts` and `updates`.
    assert_eq!(
        mutability("CreateInboxInput", "secret"),
        Some(Mutability::Write)
    );
    assert_eq!(
        mutability("CreateInboxInput", "seed"),
        Some(Mutability::Create)
    );
    assert_eq!(
        mutability("UpdateInboxInput", "note"),
        Some(Mutability::Full)
    );
    // A filter is not the property, and an identifier is not a property.
    assert_eq!(mutability("ListInboxesInput", "name"), None);
    assert_eq!(mutability("ListInboxesInput", "workspaceId"), None);
    assert_eq!(mutability("InboxData", "id"), None);
    assert_eq!(mutability("UpdateInboxInput", "inboxId"), None);
}

#[test]
fn a_property_declares_its_presence_for_views_not_inputs() {
    let service = service(INBOX);
    // Optional in every view without a `?`...
    let view = names(&service, "InboxData");
    assert!(view.contains(&("note".to_owned(), false)));
    // ...while an input asks for what the operation needs.
    assert!(!member(&service, "UpdateInboxInput", "note").required);
    let service = self::service(&format!(
        "{INBOX}\noperation AddNote on Inbox {{\n    http POST \"/workspaces/{{workspaceId}}/inboxes/{{inboxId}}/note\"\n    input {{\n        $workspaceId\n        $inboxId\n        $note\n    }}\n}}"
    ));
    assert!(member(&service, "AddNoteInput", "note").required);
}

// ---- `$*` --------------------------------------------------------------------

const ONE: &str = r#"
resource Workspace {
    id workspaceId: WorkspaceId
    property name: Text
    property note?: Text
}

operation UpdateWorkspace updates Workspace {
    http PATCH "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $name?
        $note?
    }
    output { workspace: View }
}
"#;

#[test]
fn star_is_named_again_only_as_an_optional_after_it() {
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $*\n $name }}"
    ));
    assert_mentions(&errors, "`$name` is already included by `$*`");
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $*\n $name as title? }}"
    ));
    assert_mentions(&errors, "already included by `$*`");
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $name\n $* }}"
    ));
    assert_mentions(&errors, "`$name` is written before `$*`, which includes it");
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $*\n @deprecated(message: \"x\") $name? }}"
    ));
    assert_mentions(&errors, "already included by `$*`");
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $*\n $* }}"
    ));
    assert_mentions(&errors, "`$*` appears once in a view");
}

#[test]
fn star_belongs_in_a_for_view() {
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ $* }}\nstructure Plain {{ $* }}\nstructure UsesPlain {{ p: Plain }}"
    ));
    assert_mentions(&errors, "`$*` belongs in a `for` view, where it includes");
    let model = r#"
resource Workspace {
    id workspaceId: WorkspaceId
    readonly property name: Text
}
operation GetWorkspace reads Workspace {
    http GET "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $*
    }
    output { $* }
}
"#;
    let (errors, _) = split(model);
    assert_mentions(&errors, "an input binds what the operation takes");
    assert_mentions(&errors, "`$*` belongs in a `for` view, where it includes");
}

#[test]
fn star_and_a_plain_member_of_the_same_name_clash() {
    let (errors, _) = split(&format!(
        "{ONE}\nstructure View for Workspace {{ name: Text\n $* }}"
    ));
    assert_mentions(
        &errors,
        "`$*` includes `name`, which `View` already declares",
    );
}

// ---- keywords, presence and spelling on entries -------------------------------

#[test]
fn entry_keywords_are_checked() {
    let (errors, _) = split(
        "resource Workspace {\n    readonly id workspaceId?: WorkspaceId\n    readonly immutable property a: Text\n    writeonly writeonly property b: Text\n    property c: Text as d\n}",
    );
    assert_mentions(&errors, "an identifier is immutable and takes no keyword");
    assert_mentions(&errors, "an identifier is always present");
    assert_mentions(&errors, "`readonly` combines with no other keyword");
    assert_mentions(&errors, "`writeonly` is given twice");
    assert_mentions(&errors, "only an identifier names its spelling in views");
}

#[test]
fn a_spelling_is_unique_among_its_resources_view_names() {
    let (errors, _) = split(
        "resource Workspace {\n    id workspaceId: WorkspaceId as name\n    readonly property name: Text\n}",
    );
    assert_mentions(
        &errors,
        "`name` already names the property `name` in `Workspace`'s views",
    );
    let (errors, _) = split(
        "resource Workspace {\n    id workspaceId: WorkspaceId\n}\nresource Inbox in Workspace {\n    id inboxId: InboxId as workspaceId\n}",
    );
    assert_mentions(
        &errors,
        "`workspaceId` already names the inherited identifier `workspaceId` in `Inbox`'s views",
    );
}

#[test]
fn a_parent_and_its_child_may_both_spell_an_identifier_id() {
    // The spelling is the declaring resource's own: in the child's views the
    // parent's identifier keeps its name, so both can be `id`.
    let service = service(INBOX);
    let summary: Vec<String> = names(&service, "InboxSummary")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(summary, ["id", "address"]);
    assert_eq!(names(&service, "InboxData")[0].0, "workspaceId");
    assert_eq!(names(&service, "WorkspaceData")[0].0, "id");
}

// ---- bindings against keywords (34a) ------------------------------------------

const KEYWORDS: &str = r#"
resource Workspace {
    id workspaceId: WorkspaceId
    readonly property status: Text
    immutable property region: Text
    property name: Text
    writeonly property password: Text
}

structure WorkspaceView for Workspace {
    $workspaceId
    $status
    $region
    $name
}

operation CreateWorkspace creates Workspace {
    http POST "/workspaces"
    input {
        $region
        $name
        $password
    }
    output { workspace: WorkspaceView }
}

operation UpdateWorkspace updates Workspace {
    http PATCH "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $name?
        $password?
    }
}
"#;

#[test]
fn the_keyword_model_compiles_cleanly() {
    let (errors, warnings) = split(KEYWORDS);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

fn with(old: &str, new: &str) -> String {
    assert!(KEYWORDS.contains(old), "{old}");
    KEYWORDS.replacen(old, new, 1)
}

#[test]
fn an_update_cannot_write_a_readonly_or_immutable_property() {
    let (errors, _) = split(&with(
        "        $password?\n",
        "        $password?\n        $status?\n        $region?\n",
    ));
    assert_mentions(
        &errors,
        "an `updates` input cannot bind `status`, which is `readonly`",
    );
    assert_mentions(
        &errors,
        "an `updates` input cannot bind `region`, which is `immutable`",
    );
}

#[test]
fn a_create_or_put_cannot_write_a_readonly_property() {
    let (errors, _) = split(&with(
        "        $password\n",
        "        $password\n        $status\n",
    ));
    assert_mentions(
        &errors,
        "a `creates` input cannot bind `status`, which is `readonly`",
    );
    let put = "operation PutWorkspace puts Workspace {\n    http PUT \"/workspaces/{workspaceId}\"\n    input {\n        $workspaceId\n        $status\n    }\n}";
    let (errors, _) = split(&format!("{KEYWORDS}\n{put}"));
    assert_mentions(
        &errors,
        "a `puts` input cannot bind `status`, which is `readonly`",
    );
}

#[test]
fn filters_and_custom_actions_are_not_writes() {
    let extra = r#"
operation ListWorkspaces lists Workspace {
    http GET "/workspaces"
    input {
        @query
        $status?
    }
    output { workspaces: WorkspaceViews }
}
list WorkspaceViews: WorkspaceView

operation SearchWorkspaces on Workspace collection {
    http POST "/workspaces/search"
    input { $status? }
}

operation Suspend on Workspace {
    http POST "/workspaces/{workspaceId}/suspend"
    input {
        $workspaceId
        $status
    }
}
"#;
    let (errors, warnings) = split(&format!(
        "{}\n{extra}",
        with(
            "        $password?\n",
            "        $password?\n        @query\n        $region?\n"
        )
    ));
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn an_immutable_property_something_must_set() {
    let (errors, _) = split(&with("        $region\n        $name\n", "        $name\n"));
    assert_mentions(
        &errors,
        "`region` is `immutable`, but no `creates` or `puts` input binds `$region`: nothing could ever set it",
    );
}

#[test]
fn a_property_nothing_updates_is_declared_immutable_or_readonly() {
    let (errors, warnings) = split(&with("        $name?\n", ""));
    assert_eq!(errors, Vec::<String>::new());
    assert_mentions(
        &warnings,
        "no `updates` or `puts` input binds `$name`, so nothing changes it after creation: declare it `immutable`",
    );
    let (_, warnings) = split(&with("        $password?\n", ""));
    assert_mentions(&warnings, "declare it `immutable writeonly`");
    let (_, warnings) = split(&with("        $name\n", "").replacen("        $name?\n", "", 1));
    assert_mentions(
        &warnings,
        "no `creates`, `puts` or `updates` input binds `$name`, so the service alone sets it: declare it `readonly`",
    );
}

#[test]
fn a_puts_input_both_creates_and_updates() {
    let model = r#"
resource Workspace {
    id workspaceId: WorkspaceId
    immutable property region: Text
    property name: Text
}
idempotent operation PutWorkspace puts Workspace {
    http PUT "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $region
        $name
    }
}
"#;
    let (errors, warnings) = split(model);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn a_writeonly_property_is_never_shown() {
    let (errors, _) = split(&with("    $name\n}", "    $name\n    $password\n}"));
    assert_mentions(
        &errors,
        "`password` is `writeonly`: a view or an output never returns it",
    );
    let (errors, _) = split(&with(
        "        $password?\n    }\n}",
        "        $password?\n    }\n    output { $password }\n}",
    ));
    assert_mentions(
        &errors,
        "`password` is `writeonly`: a view or an output never returns it",
    );
}

#[test]
fn mutability_annotations_agree_with_the_keyword() {
    let (errors, _) = split(&with(
        "    $region\n    $name\n}",
        "    @mutability(full)\n    $region\n    $name\n}",
    ));
    assert_mentions(
        &errors,
        "`@mutability(full)` disagrees with `region`, which is `immutable` (`createAndRead`)",
    );
    let (errors, _) = split(&with(
        "    $region\n    $name\n}",
        "    @mutability(createAndRead)\n    $region\n    $name\n}",
    ));
    assert_eq!(errors, Vec::<String>::new());
    // A member that binds no property keeps its annotation.
    let model = with(
        "        $password\n    }",
        "        $password\n        @mutability(create)\n        localPart?: Text\n    }",
    );
    let service = service(&model);
    assert_eq!(
        member(&service, "CreateWorkspaceInput", "localPart").mutability,
        Some(Mutability::Create)
    );
}

// ---- 34h: a resource modelled beside its representation -----------------------

#[test]
fn outputs_that_show_no_property_are_warned_about() {
    let model = r#"
resource Workspace {
    id workspaceId: WorkspaceId
    readonly property name: Text
}
structure WorkspaceInfo {
    workspaceId: WorkspaceId
    name: Text
}
operation GetWorkspace reads Workspace {
    http GET "/workspaces/{workspaceId}"
    input { $workspaceId }
    output { workspace: WorkspaceInfo }
}
"#;
    let (errors, warnings) = split(model);
    assert_eq!(errors, Vec::<String>::new());
    assert_mentions(
        &warnings,
        "`Workspace` has `reads` operations, but no output shows one of its properties",
    );
    assert_mentions(
        &warnings,
        "`WorkspaceInfo` restates `Workspace`'s identifier `workspaceId` and property `name`: did you mean `structure WorkspaceInfo for Workspace`",
    );
    // A view of it, even through a list, quiets both.
    let model = model
        .replace(
            "structure WorkspaceInfo {",
            "structure WorkspaceInfo for Workspace {",
        )
        .replace("    workspaceId: WorkspaceId\n    name: Text\n", "    $*\n");
    let (errors, warnings) = split(&model);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
}

#[test]
fn a_restated_identifier_matches_by_its_spelling_too() {
    let (_, warnings) = split(
        "resource Workspace {\n    id workspaceId: WorkspaceId as id\n    readonly property name: Text\n}\nstructure Info {\n    id: WorkspaceId\n    name: Text\n}",
    );
    assert_mentions(
        &warnings,
        "`Info` restates `Workspace`'s identifier `workspaceId`",
    );
}

// ---- a child's properties are its own (decision 31) ---------------------------

const FAMILY: &str = r#"
string WorkspaceName
string InboxName

resource Workspace {
    id workspaceId: WorkspaceId
    property name: WorkspaceName
    readonly property createdAt: Text
}

resource Inbox in Workspace {
    id inboxId: InboxId
    property name: InboxName
    readonly property createdAt: Text
}

structure WorkspaceData for Workspace { $* }
structure InboxData for Inbox {
    $workspaceId
    $*
}

operation CreateWorkspace creates Workspace {
    http POST "/workspaces"
    input { $name }
    output { workspace: WorkspaceData }
}

operation RenameWorkspace updates Workspace {
    http PATCH "/workspaces/{workspaceId}"
    input {
        $workspaceId
        $name
    }
    output { $name }
}

operation CreateInbox creates Inbox {
    http POST "/workspaces/{workspaceId}/inboxes"
    input {
        $workspaceId
        $name
    }
    output { inbox: InboxData }
}

operation RenameInbox updates Inbox {
    http PATCH "/workspaces/{workspaceId}/inboxes/{inboxId}"
    input {
        $workspaceId
        $inboxId
        $name
    }
    output { $name }
}
"#;

#[test]
fn a_child_and_its_parent_each_have_their_own_properties() {
    let (errors, warnings) = split(FAMILY);
    assert_eq!(errors, Vec::<String>::new());
    assert_eq!(warnings, Vec::<String>::new());
    let service = service(FAMILY);
    let target = |shape: &str, name: &str| member(&service, shape, name).target.name().to_owned();
    assert_eq!(target("RenameWorkspaceInput", "name"), "WorkspaceName");
    assert_eq!(target("RenameWorkspaceOutput", "name"), "WorkspaceName");
    assert_eq!(target("WorkspaceData", "name"), "WorkspaceName");
    assert_eq!(target("RenameInboxInput", "name"), "InboxName");
    assert_eq!(target("RenameInboxOutput", "name"), "InboxName");
    assert_eq!(target("InboxData", "name"), "InboxName");
    assert_eq!(
        names(&service, "InboxData")
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["workspaceId", "inboxId", "name", "createdAt"]
    );
}

#[test]
fn a_property_cannot_share_an_inherited_identifiers_name() {
    let (errors, _) = split(&FAMILY.replacen(
        "    property name: InboxName\n",
        "    property name: InboxName\n    property workspaceId: Text\n",
        1,
    ));
    assert_mentions(
        &errors,
        "`workspaceId` is an identifier `Inbox` inherits; a property cannot share its name",
    );
    // Identifiers stay unique across the chain.
    let (errors, _) = split(&FAMILY.replacen(
        "    id inboxId: InboxId\n",
        "    id inboxId: InboxId\n    id workspaceId: WorkspaceId\n",
        1,
    ));
    assert_mentions(
        &errors,
        "`workspaceId` is already declared by an ancestor of `Inbox`",
    );
}

#[test]
fn a_child_does_not_inherit_its_parents_properties() {
    let model = FAMILY.replacen(
        "    readonly property createdAt: Text\n}\n\nstructure",
        "}\n\nstructure",
        1,
    );
    let model = model.replacen(
        "structure InboxData for Inbox {\n    $workspaceId\n    $*\n}",
        "structure InboxData for Inbox {\n    $workspaceId\n    $*\n    $createdAt\n}",
        1,
    );
    let (errors, _) = split(&model);
    assert_mentions(
        &errors,
        "resource `Inbox` has no identifier or property `createdAt`: `createdAt` is a property of its ancestor `Workspace`, and a child inherits only its ancestors' identifiers",
    );
}
