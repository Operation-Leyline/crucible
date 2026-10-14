//! Checks 1-18 of docs/permissions.md §5: what holds of a lowered service's
//! permissions and requirements, whichever front end built it.
//!
//! Facts the contract IR does not carry are passed in [`PermissionFacts`]:
//! each resource's identifier types, which operations are collection
//! operations, and which input member binds each identifier of an
//! operation's resource. Every front end knows them while lowering, so the
//! checks that need them (the binding types of check 10, `filters` on a
//! collection operation in check 14, and `permissions.other-instance`) run
//! in the front ends only. [`permissions_from_ir`] runs the rest on an IR
//! alone and skips those.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde_json::Value;

use crate::ir::permissions::Binding;
use crate::ir::{
    Condition, Member, Mode, Operation, Requirement, Requirements, ResourcePermissions, Service,
    Shape, SimpleKind, Tier,
};
use crate::shape_id::ShapeId;

/// The error a hidden resource is refused with (decision 5, check 17).
pub fn not_found_id() -> ShapeId {
    ShapeId::new("crucible.errors", "NotFoundException")
}

/// What lowering knew that the IR does not keep.
#[derive(Debug, Clone, Default)]
pub struct PermissionFacts {
    /// Every resource of the service, with or without permissions: its
    /// identifiers and their types, root to leaf.
    pub resources: BTreeMap<ShapeId, Vec<(String, ShapeId)>>,
    /// Operations bound to a resource's collection: `creates`, `lists`,
    /// `on .. collection`; Smithy `create`, `list` and `collectionOperations`.
    pub collection_operations: BTreeSet<ShapeId>,
    /// Where a permission was declared, when that is not `resource$symbol`:
    /// a Smithy `@permissionsOf` enum's member.
    pub declared_at: BTreeMap<(ShapeId, String), ShapeId>,
    /// For an operation bound to a resource: the input member that binds
    /// each of the resource's identifiers (inherited ones included), by
    /// identifier. The handler loads the instance these name.
    pub identifier_members: BTreeMap<ShapeId, BTreeMap<String, String>>,
}

impl PermissionFacts {
    fn site(&self, resource: &ShapeId, symbol: &str) -> ShapeId {
        self.declared_at
            .get(&(resource.clone(), symbol.to_owned()))
            .cloned()
            .unwrap_or_else(|| resource.with_member(symbol))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum FindingSeverity {
    /// The model does not compile.
    Error,
    /// Probably unintended; the model still compiles.
    Warning,
    /// Worth deciding: something the model is silent about.
    Advice,
}

/// One problem with a service's permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionFinding {
    pub severity: FindingSeverity,
    /// `permissions.undeclared` and the like, stable for tooling.
    pub code: &'static str,
    /// The shape, member or operation the finding is about.
    pub shape: ShapeId,
    /// For a finding about one clause of an operation, its index in the
    /// operation's requirement list (source order).
    pub requirement: Option<usize>,
    pub message: String,
}

impl PermissionFinding {
    /// `code: message`, as diagnostics print it.
    pub fn text(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }

    /// `code: @requires[i]: message`: the text with the clause a Smithy
    /// model's `@requires` list holds it at.
    pub fn smithy_text(&self) -> String {
        match self.requirement {
            Some(index) => format!("{}: @requires[{index}]: {}", self.code, self.message),
            None => self.text(),
        }
    }
}

impl FindingSeverity {
    /// `error`, `warning` or `advice`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Advice => "advice",
        }
    }
}

/// `shape: severity: code: [@requires[i]: ]message`, the line a Smithy
/// build prints.
impl fmt::Display for PermissionFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}",
            self.shape,
            self.severity.label(),
            self.smithy_text()
        )
    }
}

/// Whether lowering attaches `crucible.errors#NotFoundException` to an
/// operation with these requirements (check 17): any clause that is not on
/// a root resource with `*` has something to hide.
pub fn needs_not_found(
    requirements: &Requirements,
    permissions: &BTreeMap<ShapeId, ResourcePermissions>,
) -> bool {
    requirements.clauses().iter().any(|clause| {
        let root = permissions
            .get(&clause.resource)
            .is_some_and(|r| r.identifiers.len() == 1);
        !(root && clause.bindings == [Binding::All])
    })
}

/// Whether `value` follows the permission value grammar (§4.1):
/// `segment { ":" segment }`, a segment a lowercase letter, then letters
/// and digits, with single hyphens between them.
pub fn permission_value_valid(value: &str) -> bool {
    !value.is_empty() && value.split(':').all(segment_valid)
}

fn segment_valid(segment: &str) -> bool {
    let mut parts = segment.split('-');
    let first = parts.next().unwrap_or_default();
    let starts_lower = first.chars().next().is_some_and(|c| c.is_ascii_lowercase());
    let word = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric());
    let after_hyphen = |part: &str| {
        word(part)
            && part
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    starts_lower && word(first) && parts.all(after_hyphen)
}

/// The authorization name a resource's shape name derives (§4.1):
/// lowerCamelCase, so `ApiKey` is `apiKey`.
pub fn derived_authorization_name(shape: &str) -> String {
    let mut chars = shape.chars();
    match chars.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

fn lower_camel(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_alphanumeric())
}

/// Every check of §5 against `service`. Findings are in model order:
/// resources by id, then operations by name, each clause in source order.
pub fn permissions(service: &Service, facts: &PermissionFacts) -> Vec<PermissionFinding> {
    check(service, Some(facts))
}

/// The checks of §5 that need nothing but the IR, for a service read back
/// from a snapshot. The binding types of check 10, check 14's collection
/// operation rule and `permissions.other-instance` need lowering's
/// [`PermissionFacts`]; they are skipped, never reported.
pub fn permissions_from_ir(service: &Service) -> Vec<PermissionFinding> {
    check(service, None)
}

fn check(service: &Service, facts: Option<&PermissionFacts>) -> Vec<PermissionFinding> {
    let empty = PermissionFacts::default();
    let mut check = Checker {
        service,
        facts: facts.unwrap_or(&empty),
        complete: facts.is_some(),
        findings: Vec::new(),
    };
    check.vocabulary();
    check.permission_types();
    for operation in &service.operations {
        check.operation(operation);
    }
    check.findings
}

struct Checker<'a> {
    service: &'a Service,
    facts: &'a PermissionFacts,
    /// `facts` came from lowering; without them the checks that need them
    /// are skipped.
    complete: bool,
    findings: Vec<PermissionFinding>,
}

/// One member a path passes through.
struct Step<'a> {
    member: &'a Member,
    /// The member is a union arm.
    arm: bool,
}

enum PathProblem {
    NoInput,
    /// No member `segment` at this index.
    Missing(usize),
    /// The member at this index is a list or map.
    Collection(usize, &'static str),
    /// The path continues past a member that is not a structure or union.
    Scalar(usize),
}

impl<'a> Checker<'a> {
    fn push(
        &mut self,
        severity: FindingSeverity,
        code: &'static str,
        shape: &ShapeId,
        requirement: Option<usize>,
        message: impl Into<String>,
    ) {
        self.findings.push(PermissionFinding {
            severity,
            code,
            shape: shape.clone(),
            requirement,
            message: message.into(),
        });
    }

    fn error(
        &mut self,
        code: &'static str,
        shape: &ShapeId,
        at: Option<usize>,
        message: impl Into<String>,
    ) {
        self.push(FindingSeverity::Error, code, shape, at, message);
    }

    fn warning(
        &mut self,
        code: &'static str,
        shape: &ShapeId,
        at: Option<usize>,
        message: impl Into<String>,
    ) {
        self.push(FindingSeverity::Warning, code, shape, at, message);
    }

    /// The symbols any requirement names, per resource.
    fn required(&self) -> BTreeSet<(&'a ShapeId, &'a str)> {
        self.service
            .operations
            .iter()
            .flat_map(|op| op.requirements.clauses())
            .map(|clause| (&clause.resource, clause.permission.as_str()))
            .collect()
    }

    // ---- checks 1-5 ---------------------------------------------------------

    fn vocabulary(&mut self) {
        let required = self.required();
        let mut names: BTreeMap<&str, &ShapeId> = BTreeMap::new();
        for (id, resource) in &self.service.permissions {
            if !lower_camel(&resource.name) {
                self.error(
                    "permissions.name",
                    id,
                    None,
                    format!(
                        "the authorization name `{}` is not lowerCamelCase; grants and audit records store it",
                        resource.name
                    ),
                );
            }
            if let Some(other) = names.insert(&resource.name, id) {
                self.error(
                    "permissions.name",
                    id,
                    None,
                    format!(
                        "`{}` and `{}` both have the authorization name `{}`; give one an `@authorizationName`",
                        other.name(),
                        id.name(),
                        resource.name
                    ),
                );
            }
            let mut symbols = BTreeSet::new();
            let mut values: BTreeMap<String, &str> = BTreeMap::new();
            for permission in &resource.permissions {
                let site = self.facts.site(id, &permission.symbol);
                if !symbols.insert(permission.symbol.as_str()) {
                    self.error(
                        "permissions.duplicate",
                        &site,
                        None,
                        format!(
                            "`{}` declares the permission `{}` twice",
                            id.name(),
                            permission.symbol
                        ),
                    );
                }
                if !permission_value_valid(&permission.value) {
                    self.error(
                        "permissions.value",
                        &site,
                        None,
                        format!(
                            "{:?} is not a permission value: write lowercase-led segments of letters and digits, joined by `:`, with single `-` inside a segment, such as \"mail:send\" or \"api-key:manage\"",
                            permission.value
                        ),
                    );
                }
                match values.get(&permission.value.to_lowercase()) {
                    Some(other) if *other == permission.value => self.error(
                        "permissions.duplicate",
                        &site,
                        None,
                        format!("`{}` declares the value {:?} twice", id.name(), permission.value),
                    ),
                    Some(other) => self.error(
                        "permissions.duplicate",
                        &site,
                        None,
                        format!(
                            "{:?} differs from {other:?} only in case; values are compared case-sensitively, so one would be a different permission that reads as the same",
                            permission.value
                        ),
                    ),
                    None => {
                        values.insert(permission.value.to_lowercase(), &permission.value);
                    }
                }
                let named = required.contains(&(id, permission.symbol.as_str()));
                if let Some(primary) = &permission.alias_of {
                    match resource.permission(primary) {
                        None => self.error(
                            "permissions.alias",
                            &site,
                            None,
                            format!("`{}` is an alias of `{primary}`, which `{}` does not declare", permission.symbol, id.name()),
                        ),
                        Some(target) if target.alias_of.is_some() => self.error(
                            "permissions.alias",
                            &site,
                            None,
                            format!(
                                "`{}` is an alias of `{primary}`, itself an alias; name the primary `{}`",
                                permission.symbol,
                                target.alias_of.as_deref().unwrap_or_default()
                            ),
                        ),
                        Some(target) if target.tier != permission.tier => self.error(
                            "permissions.alias",
                            &site,
                            None,
                            format!(
                                "`{}` is an alias of `{primary}` but in another tier; an alias is in its primary's tier",
                                permission.symbol
                            ),
                        ),
                        Some(_) => {}
                    }
                    if permission.notes.deprecated.is_none() {
                        self.error(
                            "permissions.alias",
                            &site,
                            None,
                            format!(
                                "the alias `{}` must be `@deprecated`: it is kept for grants stored under {:?} until it is removed",
                                permission.symbol, permission.value
                            ),
                        );
                    }
                } else if permission.tier == Tier::Public && !named {
                    self.warning(
                        "permissions.unrequired",
                        &site,
                        None,
                        format!(
                            "no operation requires `{}`; declare it `@permissionOnly` if the service's policy checks it, or require it",
                            permission.symbol
                        ),
                    );
                }
            }
        }
    }

    // ---- check 6 ------------------------------------------------------------

    fn permission_types(&mut self) {
        let Some(types) = &self.service.permission_types else {
            return;
        };
        let shape = &types.shape;
        let values: Vec<&str> = match self.service.shape(shape) {
            Some(Shape::Enum(e)) => e.values.iter().map(|v| v.value.as_str()).collect(),
            _ => {
                self.error(
                    "permissions.types",
                    shape,
                    None,
                    "`@permissionTypes` applies to a string enum",
                );
                return;
            }
        };
        let names: BTreeMap<&str, &ShapeId> = self
            .service
            .permissions
            .iter()
            .map(|(id, r)| (r.name.as_str(), id))
            .collect();
        for value in &values {
            if !names.contains_key(value) {
                let known = names.keys().map(|n| format!("`{n}`")).collect::<Vec<_>>();
                self.error(
                    "permissions.types",
                    shape,
                    None,
                    format!(
                        "{value:?} is not the authorization name of a resource that declares permissions{}",
                        if known.is_empty() {
                            String::new()
                        } else {
                            format!("; those are {}", known.join(", "))
                        }
                    ),
                );
            }
        }
        for excluded in &types.excluded {
            match self.service.permissions.get(excluded) {
                None => self.error(
                    "permissions.types",
                    shape,
                    None,
                    format!(
                        "`{}` is excluded but declares no permissions",
                        excluded.name()
                    ),
                ),
                Some(r) if values.contains(&r.name.as_str()) => self.error(
                    "permissions.types",
                    shape,
                    None,
                    format!(
                        "`{}` is excluded but {:?} is one of the values",
                        excluded.name(),
                        r.name
                    ),
                ),
                Some(_) => {}
            }
        }
        for (id, resource) in &self.service.permissions {
            let grantable = resource.permissions.iter().any(|p| {
                p.alias_of.is_none() && matches!(p.tier, Tier::Public | Tier::PermissionOnly)
            });
            if grantable
                && !values.contains(&resource.name.as_str())
                && !types.excluded.contains(id)
            {
                self.error(
                    "permissions.types",
                    shape,
                    None,
                    format!(
                        "`{}` has grantable permissions but {:?} is not a value; add `{} = {:?}`, or exclude it: `@permissionTypes(exclude: [{}])`",
                        id.name(),
                        resource.name,
                        resource.name,
                        resource.name,
                        id.name()
                    ),
                );
            }
        }
    }

    // ---- checks 7-18 --------------------------------------------------------

    fn operation(&mut self, op: &'a Operation) {
        let id = &op.id;
        // No caller to authorize: declared public, or no scheme at all.
        let public = op.public || op.auth.is_empty();
        match &op.requirements {
            Requirements::Undeclared => {
                if !public && !self.service.permissions.is_empty() {
                    self.error(
                        "permissions.undeclared",
                        id,
                        None,
                        format!(
                            "`{}` says nothing about authorization, and the service declares permissions: add `requires <permission> on <Resource>`, `filters`, `requires none` (with a `///` saying why) or `requires pending because \"..\"`",
                            op.name()
                        ),
                    );
                }
                return;
            }
            _ if public => {
                let why = if op.public || !self.service.auth.is_empty() {
                    "is public"
                } else {
                    "has no authentication scheme (the service declares none)"
                };
                self.error(
                    "permissions.public",
                    id,
                    None,
                    format!(
                        "`{}` {why}, so there is no caller to authorize; remove its requirement declarations",
                        op.name()
                    ),
                );
                return;
            }
            Requirements::None | Requirements::Internal => return,
            Requirements::Pending { reason } => {
                if reason.trim().is_empty() {
                    self.error(
                        "permissions.reason",
                        id,
                        None,
                        "`requires pending` needs a nonempty reason: say what checks the operation until it declares its requirements",
                    );
                } else {
                    self.warning(
                        "permissions.pending",
                        id,
                        None,
                        format!("`{}` has no declared requirements yet: {reason}", op.name()),
                    );
                }
                return;
            }
            Requirements::List(list) if list.is_empty() => {
                self.error(
                    "permissions.requires",
                    id,
                    None,
                    format!(
                        "`{}` lists no requirement; an operation that needs no permission says `requires none` (Smithy `@requiresNone(\"why\")`)",
                        op.name()
                    ),
                );
                return;
            }
            Requirements::List(_) => {}
        }
        let clauses = op.requirements.clauses();
        let mut seen: Vec<&Requirement> = Vec::new();
        let mut filters = 0;
        for (index, clause) in clauses.iter().enumerate() {
            if seen.contains(&clause) {
                self.error(
                    "permissions.duplicate-clause",
                    id,
                    Some(index),
                    format!(
                        "`{}` on `{}` is required twice",
                        clause.permission,
                        clause.resource.name()
                    ),
                );
                continue;
            }
            seen.push(clause);
            if clause.mode == Mode::Filter {
                filters += 1;
                if filters == 2 {
                    self.error(
                        "permissions.filters",
                        id,
                        Some(index),
                        "an operation filters by one permission: `filters` appears once",
                    );
                }
            }
            self.clause(op, index, clause);
        }
        // `filters` with a `requires` on the same resource.
        for (index, clause) in clauses.iter().enumerate() {
            if clause.mode == Mode::Filter
                && clauses
                    .iter()
                    .any(|other| other.mode == Mode::Check && other.resource == clause.resource)
            {
                self.warning(
                    "permissions.filters",
                    id,
                    Some(index),
                    format!(
                        "`{}` both filters and requires a permission on `{}`; a caller without the requirement never sees the filtered listing",
                        op.name(),
                        clause.resource.name()
                    ),
                );
            }
        }
        self.not_found(op);
        self.unchecked_references(op);
        self.other_instances(op);
    }

    fn clause(&mut self, op: &'a Operation, index: usize, clause: &'a Requirement) {
        let id = &op.id;
        let at = Some(index);
        let Some(resource) = self.service.permissions.get(&clause.resource) else {
            let message = if !self.complete || self.facts.resources.contains_key(&clause.resource) {
                let owners = self.owners(&clause.permission);
                format!(
                    "`{}` declares no permissions{}",
                    clause.resource.name(),
                    match owners.as_slice() {
                        [] => String::new(),
                        owners => format!(
                            "; `{}` is a permission of {}",
                            clause.permission,
                            owners
                                .iter()
                                .map(|o| format!("`{o}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }
                )
            } else {
                format!(
                    "`{}` is not a resource of this service",
                    clause.resource.name()
                )
            };
            self.error("permissions.unknown", id, at, message);
            return;
        };
        match resource.permission(&clause.permission) {
            None => {
                let mut message = format!(
                    "`{}` declares no permission `{}`",
                    clause.resource.name(),
                    clause.permission
                );
                let owners = self.owners(&clause.permission);
                if !owners.is_empty() {
                    message.push_str(&format!(
                        "; it is a permission of {}",
                        owners
                            .iter()
                            .map(|o| format!("`{o}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                } else if let Some(near) = closest(
                    &clause.permission,
                    self.service
                        .permissions
                        .values()
                        .flat_map(|r| r.permissions.iter().map(|p| p.symbol.as_str())),
                ) {
                    message.push_str(&format!("; did you mean `{near}`?"));
                }
                self.error("permissions.unknown", id, at, message);
            }
            Some(permission) => {
                if let Some(primary) = &permission.alias_of {
                    self.error(
                        "permissions.alias",
                        id,
                        at,
                        format!(
                            "`{}` is a deprecated alias; requirements name primaries: did you mean `{primary}`?",
                            clause.permission
                        ),
                    );
                }
                if permission.tier == Tier::PermissionOnly {
                    self.error(
                        "permissions.tier",
                        id,
                        at,
                        format!(
                            "`{}` is `@permissionOnly`: no operation requires it; remove the annotation to require it",
                            clause.permission
                        ),
                    );
                }
            }
        }
        if clause.mode == Mode::Handler {
            if !clause.bindings.is_empty() || clause.when.is_some() {
                self.error(
                    "permissions.handler",
                    id,
                    at,
                    "`by handler` takes no bindings and no `when`: the handler supplies the resource and decides whether the requirement applies",
                );
            }
            return;
        }
        if clause.mode == Mode::Filter {
            if self.complete && !self.facts.collection_operations.contains(id) {
                self.error(
                    "permissions.filters",
                    id,
                    at,
                    "`filters` answers a listing: it belongs on a `lists` or `on .. collection` operation",
                );
            }
            if clause.when.is_some() {
                self.error("permissions.filters", id, at, "`filters` takes no `when`");
            }
            if clause.bindings.last() != Some(&Binding::All) {
                self.error(
                    "permissions.filters",
                    id,
                    at,
                    "`filters` binds the parent, like a collection requirement: its own identifier is `*`",
                );
            }
        }
        let guaranteed = clause.when.as_ref().map(present).unwrap_or_default();
        if let Some(when) = &clause.when {
            self.condition(op, index, when);
        }
        self.bindings(op, index, clause, resource, &guaranteed);
    }

    /// The resources that declare a permission with this symbol.
    fn owners(&self, symbol: &str) -> Vec<&'a str> {
        self.service
            .permissions
            .iter()
            .filter(|(_, r)| r.permission(symbol).is_some())
            .map(|(id, _)| id.name())
            .collect()
    }

    fn resolve(&self, op: &Operation, path: &[String]) -> Result<Vec<Step<'a>>, PathProblem> {
        let mut current = op.input.clone().ok_or(PathProblem::NoInput)?;
        let mut steps = Vec::new();
        for (index, segment) in path.iter().enumerate() {
            let (members, arm) = match self.service.shape(&current) {
                Some(Shape::Structure(s)) => (&s.members, false),
                Some(Shape::Union(s)) => (&s.members, true),
                _ => return Err(PathProblem::Scalar(index)),
            };
            let member = members
                .iter()
                .find(|m| &m.name == segment)
                .ok_or(PathProblem::Missing(index))?;
            match self.service.shape(&member.target) {
                Some(Shape::List(_)) => return Err(PathProblem::Collection(index, "list")),
                Some(Shape::Map(_)) => return Err(PathProblem::Collection(index, "map")),
                _ => {}
            }
            steps.push(Step { member, arm });
            current = member.target.clone();
        }
        Ok(steps)
    }

    fn path_problem(
        &self,
        op: &Operation,
        path: &[String],
        problem: &PathProblem,
        what: &str,
    ) -> (&'static str, String) {
        let text = path.join(".");
        match problem {
            PathProblem::NoInput => (
                "permissions.binding",
                format!("{what} `{text}`, but `{}` has no input", op.name()),
            ),
            PathProblem::Missing(at) => (
                "permissions.binding",
                format!(
                    "{what} `{text}`, but {} has no member `{}`",
                    if *at == 0 {
                        "the input".to_owned()
                    } else {
                        format!("`{}`", path[..*at].join("."))
                    },
                    path[*at]
                ),
            ),
            PathProblem::Collection(at, kind) => (
                "permissions.list-binding",
                format!(
                    "{what} `{text}`, which passes through the {kind} `{}`; a requirement binds one instance, never an element: authorize on a shared parent every element belongs to, or check each element `by handler`",
                    path[..=*at].join(".")
                ),
            ),
            PathProblem::Scalar(at) => (
                "permissions.binding",
                format!(
                    "{what} `{text}`, but `{}` is not a structure or union",
                    path[..*at].join(".")
                ),
            ),
        }
    }

    fn bindings(
        &mut self,
        op: &'a Operation,
        index: usize,
        clause: &'a Requirement,
        resource: &ResourcePermissions,
        guaranteed: &BTreeSet<Vec<String>>,
    ) {
        let id = &op.id;
        let at = Some(index);
        let identifiers = &resource.identifiers;
        if clause.bindings.len() != identifiers.len() {
            self.error(
                "permissions.binding",
                id,
                at,
                format!(
                    "`{}` has {} identifiers ({}) and the requirement binds {}; bind each exactly once",
                    clause.resource.name(),
                    identifiers.len(),
                    identifiers.join(", "),
                    clause.bindings.len()
                ),
            );
            return;
        }
        let types = self.facts.resources.get(&clause.resource);
        let last = identifiers.len().saturating_sub(1);
        for (position, (identifier, binding)) in
            identifiers.iter().zip(&clause.bindings).enumerate()
        {
            let path = match binding {
                Binding::All if position == last => continue,
                Binding::All => {
                    self.error(
                        "permissions.binding",
                        id,
                        at,
                        format!(
                            "`*` binds only `{}`'s own identifier, never a parent: there is no cross-tenant wildcard; bind `{identifier}` from the input",
                            clause.resource.name()
                        ),
                    );
                    continue;
                }
                Binding::Member(path) => path,
            };
            let what = format!("`{identifier}` of `{}` is bound to", clause.resource.name());
            let steps = match self.resolve(op, path) {
                Ok(steps) => steps,
                Err(problem) => {
                    let (code, mut message) = self.path_problem(op, path, &problem, &what);
                    let collection = self.facts.collection_operations.contains(id)
                        && op.resource.as_ref() == Some(&clause.resource);
                    if matches!(problem, PathProblem::Missing(0)) && position == last && collection
                    {
                        message.push_str(&format!(
                            "; a collection operation has no instance: write `{}(*)` to require it on every instance, or require it on a parent",
                            clause.resource.name()
                        ));
                    }
                    self.error(code, id, at, message);
                    continue;
                }
            };
            // Check 13: every optional member or union arm on the way is
            // made present by the condition.
            for (depth, step) in steps.iter().enumerate() {
                if step.arm || step.member.optional_on_server() {
                    let prefix = path[..=depth].to_vec();
                    if !guaranteed.contains(&prefix) {
                        let prefix = prefix.join(".");
                        self.error(
                            "permissions.unguarded",
                            id,
                            at,
                            format!(
                                "`{}` passes through `{prefix}`, which a request may leave out: add `when has {prefix}`",
                                path.join(".")
                            ),
                        );
                        break;
                    }
                }
            }
            // Check 10: the member is the identifier's own type.
            let (Some(step), Some((_, expected))) =
                (steps.last(), types.and_then(|t| t.get(position)))
            else {
                continue;
            };
            let target = &step.member.target;
            if target != expected {
                let same = match (self.service.shape(target), self.service.shape(expected)) {
                    (Some(Shape::Simple(a)), Some(Shape::Simple(b))) => {
                        a.kind == SimpleKind::String
                            && b.kind == SimpleKind::String
                            && a.constraints == b.constraints
                    }
                    (Some(Shape::Simple(a)), None) => {
                        a.kind == SimpleKind::String
                            && expected == &ShapeId::new("smithy.api", "String")
                    }
                    _ => false,
                };
                let message = format!(
                    "{what} `{}`, a `{}`; the identifier is a `{}`",
                    path.join("."),
                    target.name(),
                    expected.name()
                );
                if same {
                    self.warning(
                        "permissions.binding-type",
                        id,
                        at,
                        format!(
                            "{message} with the same constraints; use `{}` so the two cannot drift",
                            expected.name()
                        ),
                    );
                } else {
                    self.error(
                        "permissions.binding",
                        id,
                        at,
                        format!("{message}; bind a member of that type"),
                    );
                }
            }
        }
    }

    fn condition(&mut self, op: &'a Operation, index: usize, condition: &Condition) {
        let id = &op.id;
        let at = Some(index);
        match condition {
            Condition::And(parts) | Condition::Or(parts) => {
                for part in parts {
                    self.condition(op, index, part);
                }
            }
            Condition::Not(inner) => self.condition(op, index, inner),
            Condition::Has(path) => {
                let Some(steps) = self.condition_path(op, index, path) else {
                    return;
                };
                if steps
                    .iter()
                    .all(|s| !s.arm && !s.member.optional_on_server())
                {
                    self.warning(
                        "permissions.always-true",
                        id,
                        at,
                        format!(
                            "`has {}` is always true: every member on the way is required",
                            path.join(".")
                        ),
                    );
                }
            }
            Condition::Equals(path, value) => {
                if let Some(steps) = self.condition_path(op, index, path) {
                    self.compared(op, index, path, &steps, std::slice::from_ref(value));
                }
            }
            Condition::In(path, values) => {
                if values.is_empty() {
                    self.error(
                        "permissions.condition",
                        id,
                        at,
                        format!("`{} in []` is never true", path.join(".")),
                    );
                }
                if let Some(steps) = self.condition_path(op, index, path) {
                    self.compared(op, index, path, &steps, values);
                }
            }
        }
    }

    fn condition_path(
        &mut self,
        op: &'a Operation,
        index: usize,
        path: &[String],
    ) -> Option<Vec<Step<'a>>> {
        match self.resolve(op, path) {
            Ok(steps) => Some(steps),
            Err(problem) => {
                let (code, message) = self.path_problem(op, path, &problem, "`when` reads");
                let code = if code == "permissions.binding" {
                    "permissions.condition"
                } else {
                    code
                };
                self.error(
                    code,
                    &op.id,
                    Some(index),
                    format!("{message}; `when` reads input members only"),
                );
                None
            }
        }
    }

    /// The values compared against a member fit its type.
    fn compared(
        &mut self,
        op: &'a Operation,
        index: usize,
        path: &[String],
        steps: &[Step<'a>],
        values: &[Value],
    ) {
        let Some(step) = steps.last() else {
            return;
        };
        let text = path.join(".");
        let problem = match self.service.shape(&step.member.target) {
            Some(Shape::Enum(e)) => values.iter().find_map(|v| match v {
                Value::String(s) if e.values.iter().any(|ev| &ev.value == s) => None,
                Value::String(s) => Some(format!(
                    "{s:?} is not a value of `{}`; its values are {}",
                    step.member.target.name(),
                    e.values
                        .iter()
                        .map(|v| format!("{:?}", v.value))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                other => Some(format!("`{text}` is an enum, compared with {other}")),
            }),
            Some(Shape::IntEnum(e)) => values.iter().find_map(|v| match v.as_i64() {
                Some(n) if e.values.iter().any(|ev| i64::from(ev.value) == n) => None,
                _ => Some(format!(
                    "{v} is not a value of `{}`",
                    step.member.target.name()
                )),
            }),
            Some(Shape::Simple(s)) => {
                let fits = |v: &Value| match s.kind {
                    SimpleKind::String => v.is_string(),
                    SimpleKind::Boolean => v.is_boolean(),
                    SimpleKind::Byte
                    | SimpleKind::Short
                    | SimpleKind::Integer
                    | SimpleKind::Long
                    | SimpleKind::BigInteger => v.is_i64() || v.is_u64(),
                    SimpleKind::Float | SimpleKind::Double | SimpleKind::BigDecimal => {
                        v.is_number()
                    }
                    _ => false,
                };
                values.iter().find(|v| !fits(v)).map(|v| {
                    format!(
                        "`{text}` is a {:?} value and cannot be compared with {v}",
                        s.kind
                    )
                })
            }
            _ => Some(format!(
                "`{text}` is not a scalar or enum; compare a scalar, or test it with `has`"
            )),
        };
        if let Some(problem) = problem {
            self.error("permissions.condition", &op.id, Some(index), problem);
        }
    }

    /// Check 17.
    fn not_found(&mut self, op: &'a Operation) {
        if !needs_not_found(&op.requirements, &self.service.permissions) {
            return;
        }
        let not_found = not_found_id();
        if !op.errors.contains(&not_found) || self.service.shape(&not_found).is_none() {
            self.error(
                "permissions.not-found",
                &op.id,
                None,
                "a requirement attaches `crucible.errors#NotFoundException`, which the model does not contain: import the `crucible.errors` package (Smithy: `errors.smithy` from crucible-traits)",
            );
            return;
        }
        for error in &op.errors {
            if error == &not_found {
                continue;
            }
            if let Some(Shape::Structure(s)) = self.service.shape(error)
                && s.error.as_ref().is_some_and(|e| e.status == 404)
            {
                self.warning(
                    "permissions.second-not-found",
                    &op.id,
                    None,
                    format!(
                        "`{}` also declares `{}`, another 404; report genuine absence with the framework NotFoundException, so a hidden resource and a missing one answer alike",
                        op.name(),
                        error.name()
                    ),
                );
            }
        }
    }

    /// A clause on the operation's own resource, or on an ancestor, that
    /// binds one of its identifiers from another member than the operation
    /// is bound by: the check authorizes an instance the handler does not
    /// load. Not reported when another clause checks the loaded instance,
    /// which makes the second one deliberate (a move between two inboxes).
    fn other_instances(&mut self, op: &'a Operation) {
        let (Some(bound), Some(own)) = (
            self.facts.identifier_members.get(&op.id),
            op.resource.as_ref(),
        ) else {
            return;
        };
        let Some(chain) = self.facts.resources.get(own) else {
            return;
        };
        let service = self.service;
        // The clause's resource, when it is `own` or an ancestor of it and
        // the clause binds each of its identifiers.
        let on_chain = |clause: &Requirement| -> Option<&'a ResourcePermissions> {
            let resource = service.permissions.get(&clause.resource)?;
            let ancestor = resource.identifiers.len() <= chain.len()
                && resource
                    .identifiers
                    .iter()
                    .zip(chain)
                    .all(|(a, (b, _))| a == b);
            (ancestor
                && clause.mode != Mode::Handler
                && clause.bindings.len() == resource.identifiers.len())
            .then_some(resource)
        };
        let mismatches = |clause: &Requirement, resource: &ResourcePermissions| {
            resource
                .identifiers
                .iter()
                .zip(&clause.bindings)
                .filter_map(
                    |(identifier, binding)| match (binding, bound.get(identifier)) {
                        (Binding::Member(path), Some(member))
                            if path.as_slice() != [member.clone()] =>
                        {
                            Some((identifier.clone(), path.join("."), member.clone()))
                        }
                        _ => None,
                    },
                )
                .collect::<Vec<_>>()
        };
        let clauses = op.requirements.clauses();
        let mut found = Vec::new();
        for (index, clause) in clauses.iter().enumerate() {
            let Some(resource) = on_chain(clause) else {
                continue;
            };
            let Some((identifier, path, member)) = mismatches(clause, resource).into_iter().next()
            else {
                continue;
            };
            let loaded_checked = clauses.iter().any(|other| {
                other.resource == clause.resource
                    && on_chain(other).is_some()
                    && mismatches(other, resource).is_empty()
            });
            if !loaded_checked {
                found.push((index, clause, identifier, path, member));
            }
        }
        for (index, clause, identifier, path, member) in found {
            self.warning(
                "permissions.other-instance",
                &op.id,
                Some(index),
                format!(
                    "`{identifier}` of `{}` is bound to `{path}`, but `{}` is bound to `{}` through `{member}`: the check authorizes another instance than the handler loads; bind `{member}`, or also require a permission on the instance the operation is bound to",
                    clause.resource.name(),
                    op.name(),
                    own.name()
                ),
            );
        }
    }

    /// Check 18.
    fn unchecked_references(&mut self, op: &'a Operation) {
        let Some(Shape::Structure(input)) = op.input.as_ref().and_then(|i| self.service.shape(i))
        else {
            return;
        };
        let covered: BTreeSet<&str> = op
            .requirements
            .clauses()
            .iter()
            .filter_map(|c| self.service.permissions.get(&c.resource))
            .flat_map(|r| r.identifiers.iter().map(String::as_str))
            .collect();
        for member in &input.members {
            for (id, resource) in &self.service.permissions {
                if resource.identifiers.last() == Some(&member.name)
                    && !covered.contains(member.name.as_str())
                {
                    self.push(
                        FindingSeverity::Advice,
                        "permissions.unchecked-reference",
                        &op.id,
                        None,
                        format!(
                            "the input binds `{}` of `{}`, and no requirement covers it; decide whether `{}` needs a permission on it",
                            member.name,
                            id.name(),
                            op.name()
                        ),
                    );
                }
            }
        }
    }
}

/// The member paths a condition guarantees present when it holds.
fn present(condition: &Condition) -> BTreeSet<Vec<String>> {
    let prefixes = |path: &[String]| -> BTreeSet<Vec<String>> {
        (1..=path.len()).map(|n| path[..n].to_vec()).collect()
    };
    match condition {
        Condition::Has(path) | Condition::Equals(path, _) | Condition::In(path, _) => {
            prefixes(path)
        }
        Condition::And(parts) => parts.iter().flat_map(present).collect(),
        Condition::Or(parts) => {
            let mut sets = parts.iter().map(present);
            let first = sets.next().unwrap_or_default();
            sets.fold(first, |acc, set| acc.intersection(&set).cloned().collect())
        }
        Condition::Not(_) => BTreeSet::new(),
    }
}

/// The candidate within a small edit distance of `word`, if any.
fn closest<'s>(word: &str, candidates: impl Iterator<Item = &'s str>) -> Option<&'s str> {
    let limit = (word.len() / 3).max(2);
    candidates
        .map(|c| (distance(word, c), c))
        .filter(|(d, _)| *d <= limit)
        .min()
        .map(|(_, c)| c)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous + usize::from(ca != cb);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(previous + 1);
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_value_grammar_allows_hyphens_inside_segments() {
        for valid in [
            "inbox:read",
            "api-key:manage",
            "apiKey:manage",
            "mail:send",
            "a",
            "x2:y-3z",
            "graph:read-write",
        ] {
            assert!(permission_value_valid(valid), "{valid}");
        }
        for invalid in [
            "*",
            "Inbox:Read",
            "mail send",
            "",
            "a:",
            ":a",
            "a--b",
            "a-",
            "-a",
            "a:B",
            "a-B",
            "a_b",
            "1a",
        ] {
            assert!(!permission_value_valid(invalid), "{invalid}");
        }
    }

    #[test]
    fn names_derive_in_lower_camel_case() {
        assert_eq!(derived_authorization_name("Inbox"), "inbox");
        assert_eq!(derived_authorization_name("ApiKey"), "apiKey");
    }

    #[test]
    fn suggestions_are_near_misses_only() {
        let symbols = ["mailSend", "mailRead", "manage"];
        assert_eq!(closest("mailsend", symbols.into_iter()), Some("mailSend"));
        assert_eq!(closest("delete", symbols.into_iter()), None);
    }
}
