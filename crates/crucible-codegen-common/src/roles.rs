//! Roles as every generator states them (docs/roles.md §5 and §7
//! "Clients"): the granted-by sentence an operation's documentation gets,
//! the role and scope registries as JSON data for descriptions and
//! OpenAPI, and the one rule every client's role registry is generated
//! from. Each reads the granted-by analysis `crucible_ir::roles::settle`
//! settled (`Operation::granted_by`) and never re-derives coverage.
//!
//! Read from the IR the caller already projected for its audience. With
//! `internal` false the helpers drop internal roles and scopes, and never
//! name an internal permission, whatever the audience: a client SDK
//! generated from the server contract says what its registry has.
//!
//! Separate from `crate::permissions`, whose requirement sentences these
//! follow, so each output appends granted-by after them.

use crucible_ir::ShapeId;
use crucible_ir::ir::{
    ClauseGrants, Mode, Operation, Requirement, Role, RoleGrant, Scope, ScopeTarget, Service, Shape,
};
use serde_json::{Value, json};

use crate::permissions::{
    HandlerDocs, clause_target, clause_text, client_visible, code_span, handler_docs, noun,
};

/// Whether a reader of this audience sees `role`.
fn role_visible(role: &Role, internal: bool) -> bool {
    internal || !role.internal
}

/// Whether a reader of this audience sees `scope`.
fn scope_visible(scope: &Scope, internal: bool) -> bool {
    internal || !scope.internal
}

/// The clause a granted-by entry is about, when the reader may see it
/// named: in this view's clauses and, for a client, not internal.
fn named_clause<'a>(
    service: &Service,
    op: &'a Operation,
    grants: &ClauseGrants,
    internal: bool,
) -> Option<&'a Requirement> {
    let clause = op.requirements.clauses().get(grants.clause?)?;
    let (resource, permission) = clause_target(service, clause)?;
    (internal || client_visible(resource, permission)).then_some(clause)
}

/// One entry's roles (or `by service` roles) as this audience sees them:
/// internal roles and scopes left out, and a role left with no scope too.
fn visible_grants<'a>(
    service: &'a Service,
    grants: &'a [RoleGrant],
    internal: bool,
) -> Vec<(&'a Role, Vec<&'a Scope>, Option<&'a str>)> {
    grants
        .iter()
        .filter_map(|grant| {
            let role = service.role(&grant.role)?;
            if !role_visible(role, internal) {
                return None;
            }
            let scopes: Vec<&Scope> = grant
                .scopes
                .iter()
                .filter_map(|symbol| service.scope(symbol))
                .filter(|scope| scope_visible(scope, internal))
                .collect();
            (!scopes.is_empty()).then_some((role, scopes, grant.docs.as_deref()))
        })
        .collect()
}

/// `a`, `a or b`, `a, b or c`.
fn or_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

/// The roles of one entry as prose: the roles that share their scopes
/// grouped, in catalogue order of each group's first role, then those the
/// service decides, then custom roles. Empty when no role this audience
/// sees grants it.
fn grantors(
    service: &Service,
    op: &Operation,
    grants: &ClauseGrants,
    internal: bool,
) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut groups: Vec<(Vec<&str>, Vec<String>)> = Vec::new();
    for (role, scopes, _) in visible_grants(service, &grants.roles, internal) {
        let scopes: Vec<&str> = scopes.iter().map(|s| s.value.as_str()).collect();
        match groups.iter_mut().find(|(at, _)| *at == scopes) {
            Some((_, roles)) => roles.push(code_span(&role.value)),
            None => groups.push((scopes, vec![code_span(&role.value)])),
        }
    }
    for (scopes, roles) in groups {
        let scopes: Vec<String> = scopes.into_iter().map(code_span).collect();
        parts.push(format!("{} at {}", or_list(&roles), or_list(&scopes)));
    }
    for (role, scopes, docs) in visible_grants(service, &grants.by_service, internal) {
        let scopes: Vec<String> = scopes.iter().map(|s| code_span(&s.value)).collect();
        let mut text = format!(
            "{} at {}, as the service decides",
            code_span(&role.value),
            or_list(&scopes)
        );
        // One paragraph reads inline, as a handler clause's does; more is
        // the role's to say on the Roles page.
        if let HandlerDocs::Inline(docs) = handler_docs(docs) {
            text.push_str(&format!(" ({docs})"));
        }
        parts.push(text);
    }
    // A custom role may hold only a grantable permission, which is never
    // internal: the clause is named whenever this applies.
    if grants.custom
        && let Some(clause) = named_clause(service, op, grants, internal)
        && let Some((_, permission)) = clause_target(service, clause)
    {
        let custom = format!("a custom role that grants {}", code_span(&permission.value));
        parts.push(if parts.is_empty() {
            custom
        } else {
            format!("or {custom}")
        });
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// What one entry is about, for an operation with several clauses: the
/// clause as its requirement sentence names it, or, for one this audience
/// does not see named, that it is internal.
fn subject(service: &Service, op: &Operation, grants: &ClauseGrants, internal: bool) -> String {
    if let Some(clause) = named_clause(service, op, grants, internal)
        && let Some((resource, permission)) = clause_target(service, clause)
    {
        if clause.mode == Mode::Filter {
            return format!(
                "{} on each {} it returns",
                code_span(&permission.value),
                noun(&resource.name)
            );
        }
        if let Some(text) = clause_text(service, clause) {
            return text;
        }
    }
    let mut text = "a permission internal to the service".to_owned();
    if grants.mode == Mode::Handler {
        text.push_str(", checked by the service");
    }
    if grants.conditional {
        text.push_str(", when a condition on the input holds");
    }
    text
}

/// Which roles grant what an operation requires (docs/roles.md §5), as
/// Markdown sentences to follow its requirement sentences: "Granted by
/// `graphReader` or `graphAdmin` at `allGraphs` or `graph`.", or, for an
/// operation with several clauses, one sentence per clause: "`note:write`
/// on the note: granted by ...". An operation whose requirements are
/// all internal still gets them, naming roles and scopes and never the
/// permission. Empty for a service without roles, and for an operation
/// that requires nothing.
pub fn granted_by_sentences(service: &Service, op: &Operation, internal: bool) -> Vec<String> {
    let single = op.granted_by.len() == 1;
    op.granted_by
        .iter()
        .filter_map(|grants| {
            let roles = grantors(service, op, grants, internal)?;
            Some(if single {
                format!("Granted by {roles}.")
            } else {
                let mut subject = subject(service, op, grants, internal);
                if let Some(first) = subject.get(..1) {
                    subject = format!("{}{}", first.to_ascii_uppercase(), &subject[1..]);
                }
                format!("{subject}: granted by {roles}.")
            })
        })
        .collect()
}

/// [`granted_by_sentences`] as one Markdown paragraph, or `None`.
pub fn granted_by_paragraph(service: &Service, op: &Operation, internal: bool) -> Option<String> {
    let sentences = granted_by_sentences(service, op, internal);
    (!sentences.is_empty()).then(|| sentences.join(" "))
}

/// One entry's roles as data: `[{"role", "scopes"}]`, values not symbols.
fn grants_data(service: &Service, grants: &[RoleGrant], internal: bool, docs: bool) -> Value {
    Value::Array(
        visible_grants(service, grants, internal)
            .into_iter()
            .map(|(role, scopes, text)| {
                let mut value = json!({
                    "role": role.value,
                    "scopes": scopes.iter().map(|s| s.value.as_str()).collect::<Vec<_>>(),
                });
                if docs && let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) {
                    value["documentation"] = json!(text);
                }
                value
            })
            .collect(),
    )
}

/// One entry as data (`grantedBy` in a description, `x-crucible-requires`
/// in OpenAPI): `roles`, `byService` and `custom`, each only when present.
fn entry_data(service: &Service, grants: &ClauseGrants, internal: bool) -> Value {
    let mut value = json!({});
    let roles = grants_data(service, &grants.roles, internal, false);
    if roles.as_array().is_some_and(|r| !r.is_empty()) {
        value["roles"] = roles;
    }
    let by_service = grants_data(service, &grants.by_service, internal, true);
    if by_service.as_array().is_some_and(|r| !r.is_empty()) {
        value["byService"] = by_service;
    }
    if grants.custom {
        value["custom"] = json!(true);
    }
    value
}

/// The mode as descriptions spell it.
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Check => "check",
        Mode::Filter => "filter",
        Mode::Handler => "handler",
    }
}

/// Granted-by as data, for an operation's requirements object (the
/// description's `requirements`, OpenAPI's `x-crucible-requires`), as
/// [`crate::permissions::requirements`] wrote it: each clause it lists
/// gets `grantedBy`, and the clauses this audience does not see named are
/// listed under `internalClauses` (`mode`, `conditional` and `grantedBy`,
/// never the permission). Unchanged for a service without roles.
pub fn annotate_requirements(
    service: &Service,
    op: &Operation,
    requirements: &mut Value,
    internal: bool,
) {
    if op.granted_by.is_empty() {
        return;
    }
    // `requirements` lists the clauses it can resolve, in order; the
    // entries are matched by clause, as the listed clauses were made.
    let listed: Vec<usize> = op
        .requirements
        .clauses()
        .iter()
        .enumerate()
        .filter(|(_, clause)| clause_target(service, clause).is_some())
        .map(|(index, _)| index)
        .collect();
    let mut hidden: Vec<Value> = Vec::new();
    for grants in &op.granted_by {
        let data = entry_data(service, grants, internal);
        let named = named_clause(service, op, grants, internal).is_some();
        let position = grants
            .clause
            .and_then(|index| listed.iter().position(|l| *l == index));
        match (named, position) {
            (true, Some(position)) => {
                if let Some(clause) = requirements
                    .get_mut("clauses")
                    .and_then(Value::as_array_mut)
                    .and_then(|clauses| clauses.get_mut(position))
                {
                    clause["grantedBy"] = data;
                }
            }
            _ => {
                let mut value = json!({"mode": mode_name(grants.mode), "grantedBy": data});
                if grants.conditional {
                    value["conditional"] = json!(true);
                }
                hidden.push(value);
            }
        }
    }
    if !hidden.is_empty() {
        requirements["internalClauses"] = Value::Array(hidden);
    }
}

/// The values a role's `holders` names: the principals enum's wire values,
/// by name, in the role's order; a name the view's enum lacks keeps its
/// name.
fn holder_values(service: &Service, role: &Role) -> Vec<String> {
    let values: Vec<(String, String)> = match service
        .role_settings
        .principals
        .as_ref()
        .and_then(|id| service.shapes.get(id))
    {
        Some(Shape::Enum(e)) => e
            .values
            .iter()
            .map(|v| (v.name.clone(), v.value.clone()))
            .collect(),
        Some(Shape::IntEnum(e)) => e
            .values
            .iter()
            .map(|v| (v.name.clone(), v.value.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    role.holders
        .iter()
        .map(|name| {
            values
                .iter()
                .find(|(n, _)| n == name)
                .map_or_else(|| name.clone(), |(_, value)| value.clone())
        })
        .collect()
}

/// One permission a client role registry lists.
pub struct RolePermission<'a> {
    /// The resource's authorization name: `graph`.
    pub resource_type: &'a str,
    /// What grants store: `graph:read`.
    pub value: &'a str,
}

/// One role as a client's `ROLES` registry carries it (docs/roles.md §7
/// "Clients"). Every client language generates from these, so they agree
/// on which roles appear, their order and each entry's fields.
pub struct ClientRole<'a> {
    pub role: &'a Role,
    /// What bindings store: `graphAdmin`.
    pub value: &'a str,
    /// The model's documentation, trimmed; empty when it has none.
    pub description: &'a str,
    /// The scope values it may be bound at, in catalogue order.
    pub scopes: Vec<&'a str>,
    /// The principal kinds (wire values) that may hold it; empty: any.
    pub holders: Vec<String>,
    /// The role values whose holders alone may assign it; empty: whoever
    /// the binding operation admits.
    pub assigned_by: Vec<&'a str>,
    /// What it grants, the model deciding, public permissions only, in the
    /// IR's order (resource, then declaration).
    pub permissions: Vec<RolePermission<'a>>,
}

/// One scope as a client's scope values carry it.
pub struct ClientScope<'a> {
    pub scope: &'a Scope,
    /// What bindings store: `allGraphs`.
    pub value: &'a str,
    /// The model's documentation, trimmed; empty when it has none.
    pub description: &'a str,
}

/// The roles a client's registry lists: every public role, in catalogue
/// order, whatever audience the service was projected for. Empty for a
/// service without roles, which then gets no registry.
pub fn client_roles(service: &Service) -> Vec<ClientRole<'_>> {
    service
        .roles
        .iter()
        .filter(|role| !role.internal)
        .map(|role| ClientRole {
            role,
            value: &role.value,
            description: role.docs.as_deref().map(str::trim).unwrap_or_default(),
            scopes: service
                .role_scopes
                .iter()
                .filter(|s| !s.internal && role.scopes.contains(&s.symbol))
                .map(|s| s.value.as_str())
                .collect(),
            holders: holder_values(service, role),
            assigned_by: role
                .assigned_by
                .iter()
                .filter_map(|symbol| service.role(symbol))
                .filter(|r| !r.internal)
                .map(|r| r.value.as_str())
                .collect(),
            permissions: role
                .effective
                .iter()
                .filter_map(|grant| {
                    let resource = service.permissions.get(&grant.resource)?;
                    let permission = resource.permission(&grant.permission)?;
                    client_visible(resource, permission).then_some(RolePermission {
                        resource_type: &resource.name,
                        value: &permission.value,
                    })
                })
                .collect(),
        })
        .collect()
}

/// The scopes a client's scope values list: every public scope, in
/// catalogue order.
pub fn client_scopes(service: &Service) -> Vec<ClientScope<'_>> {
    service
        .role_scopes
        .iter()
        .filter(|scope| !scope.internal)
        .map(|scope| ClientScope {
            scope,
            value: &scope.value,
            description: scope.docs.as_deref().map(str::trim).unwrap_or_default(),
        })
        .collect()
}

/// The resource a scope is on, as data: `{"kind": "service"}`,
/// `{"kind": "instance", "resource": id}` or `{"kind": "every", ..}`.
fn scope_target(target: &ScopeTarget) -> (&'static str, Option<&ShapeId>) {
    match target {
        ScopeTarget::Service => ("service", None),
        ScopeTarget::Instance(id) => ("instance", Some(id)),
        ScopeTarget::Every(id) => ("every", Some(id)),
    }
}

/// The role registry as data (the description's and OpenAPI's `roles`):
/// one entry per role in catalogue order. `internal` keeps internal roles,
/// their internal scopes and assigners, and internal permissions, for
/// server-audience output only; a client's has [`client_roles`]' entries.
pub fn roles_registry(service: &Service, internal: bool) -> Value {
    Value::Array(
        service
            .roles
            .iter()
            .filter(|role| role_visible(role, internal))
            .map(|role| {
                let scopes: Vec<&str> = service
                    .role_scopes
                    .iter()
                    .filter(|s| scope_visible(s, internal) && role.scopes.contains(&s.symbol))
                    .map(|s| s.value.as_str())
                    .collect();
                let assigned_by: Vec<&str> = role
                    .assigned_by
                    .iter()
                    .filter_map(|symbol| service.role(symbol))
                    .filter(|r| role_visible(r, internal))
                    .map(|r| r.value.as_str())
                    .collect();
                let permission = |resource: &ShapeId, symbol: &str| {
                    let r = service.permissions.get(resource)?;
                    let p = r.permission(symbol)?;
                    (internal || client_visible(r, p)).then_some((r, p))
                };
                let permissions: Vec<Value> = role
                    .effective
                    .iter()
                    .filter_map(|g| permission(&g.resource, &g.permission))
                    .map(|(r, p)| json!({"resourceType": r.name, "permission": p.value}))
                    .collect();
                let by_service: Vec<Value> = role
                    .by_service
                    .iter()
                    .filter_map(|g| {
                        let (r, p) = permission(&g.resource, &g.permission)?;
                        let mut value = json!({"resourceType": r.name, "permission": p.value});
                        if let Some(docs) =
                            g.docs.as_deref().map(str::trim).filter(|d| !d.is_empty())
                        {
                            value["documentation"] = json!(docs);
                        }
                        Some(value)
                    })
                    .collect();
                let mut value = json!({
                    "value": role.value,
                    "description": role.docs.as_deref().map(str::trim).unwrap_or_default(),
                    "scopes": scopes,
                    "holders": holder_values(service, role),
                    "assignedBy": assigned_by,
                    "permissions": permissions,
                    "deprecated": role.notes.deprecated.is_some(),
                });
                if !by_service.is_empty() {
                    value["byService"] = json!(by_service);
                }
                if role.internal {
                    value["internal"] = json!(true);
                }
                value
            })
            .collect(),
    )
}

/// The scope registry as data (the description's and OpenAPI's
/// `scopes`): one entry per scope in catalogue order, each with the
/// resource it is on (`kind` `service`, `instance` or `every`).
/// `internal` keeps internal scopes, for server-audience output only.
pub fn scopes_registry(service: &Service, internal: bool) -> Value {
    Value::Array(
        service
            .role_scopes
            .iter()
            .filter(|scope| scope_visible(scope, internal))
            .map(|scope| {
                let (kind, resource) = scope_target(&scope.on);
                let mut value = json!({
                    "value": scope.value,
                    "description": scope.docs.as_deref().map(str::trim).unwrap_or_default(),
                    "kind": kind,
                    "deprecated": scope.notes.deprecated.is_some(),
                });
                if let Some(id) = resource {
                    value["resource"] = json!(id.to_string());
                    if let Some(r) = service.permissions.get(id) {
                        value["resourceType"] = json!(r.name);
                    }
                }
                if scope.internal {
                    value["internal"] = json!(true);
                }
                value
            })
            .collect(),
    )
}

/// Whether the service declares custom roles, which a registry says
/// exist beside the predefined ones (§4.7).
pub fn custom(service: &Service) -> bool {
    service.role_settings.custom
}

#[cfg(test)]
mod tests {
    use super::or_list;

    #[test]
    fn lists_read_as_english() {
        assert_eq!(or_list(&[]), "");
        assert_eq!(or_list(&["a".into()]), "a");
        assert_eq!(or_list(&["a".into(), "b".into()]), "a or b");
        assert_eq!(or_list(&["a".into(), "b".into(), "c".into()]), "a, b or c");
    }
}
