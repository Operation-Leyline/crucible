//! Typed permissions in `.cru` (docs/permissions.md §4): `permission`
//! entries on resources, `requires` and `filters` clauses on operations,
//! `@authorizationName` and `@permissionTypes`.
//!
//! This module checks what only the syntax can get wrong (tiers written
//! twice, a binding name that is no identifier, `requires none` beside
//! another clause) and resolves implicit bindings; everything that holds of
//! the IR is `crucible_smithy::validate::permissions`.

use std::collections::{BTreeMap, BTreeSet};

use crucible_smithy::ShapeId;
use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{
    Condition, Mode, Permission, PermissionTypes, Requirement, Requirements, ResourcePermissions,
    Service, Tier,
};
use crucible_smithy::validate::{
    FindingSeverity, attaches_not_found, derived_authorization_name, not_found_id,
};
use serde_json::Value as Json;

use super::{DeclRef, Lowerer, Position};
use crate::ast::{
    Annotation, BindingValue, BodyKind, Condition as Cond, ConditionKind, Decl, Ident, Key,
    MemberKind, OpEntry, Operation, RequirementKind, Resource, Value,
};
use crate::compile::FileDiagnostic;
use crate::compile::Location;
use crate::diagnostic::Diagnostic;

/// Annotations a permission entry takes.
const PERMISSION_ANNOTATIONS: &[&str] = &[
    "permissionOnly",
    "notGrantable",
    "aliasOf",
    "deprecated",
    "since",
];

impl<'a> Lowerer<'a> {
    /// One resource's permission entries, checked once whether or not a
    /// service reaches it, and kept for the service that does.
    pub(super) fn declare_permissions(&mut self, decl: DeclRef<'a>, resource: &'a Resource) {
        let file = decl.file;
        let id = self.ref_id(&resource.name.name);
        let (name, name_declared) = self.authorization_name(file, decl, resource);
        self.permission_sites.insert(
            id.clone(),
            Location {
                file,
                span: resource.name.span,
            },
        );
        if resource.permissions.is_empty() {
            if let Some(annotation) = decl
                .item
                .annotations
                .iter()
                .find(|a| a.name.name == "authorizationName")
                .filter(|_| name_declared)
            {
                self.error(
                    file,
                    annotation.span,
                    format!(
                        "permissions.name: `@authorizationName` names `{}` in grants, but it declares no permissions; remove it, or declare its permissions",
                        resource.name.name
                    ),
                );
            }
            return;
        }
        let mut permissions = Vec::new();
        for entry in &resource.permissions {
            self.permission_sites.insert(
                id.with_member(&entry.name.name),
                Location {
                    file,
                    span: entry.span,
                },
            );
            let mut tiers: Vec<(Tier, &str)> = Vec::new();
            if entry.modifiers.iter().any(|m| m.name == "internal") {
                tiers.push((Tier::Internal, "`internal`"));
            }
            let mut alias_of = None;
            let mut notes = Vec::new();
            for annotation in &entry.annotations {
                let word = annotation.name.name.as_str();
                match word {
                    "permissionOnly" | "notGrantable" => {
                        if !annotation.args.is_empty() {
                            self.error(file, annotation.span, format!("`@{word}` takes no arguments"));
                        }
                        let tier = if word == "permissionOnly" {
                            Tier::PermissionOnly
                        } else {
                            Tier::NotGrantable
                        };
                        if tiers.iter().any(|(t, _)| *t == tier) {
                            self.error(file, annotation.span, format!("`@{word}` is given twice"));
                        } else {
                            tiers.push((tier, if word == "permissionOnly" { "`@permissionOnly`" } else { "`@notGrantable`" }));
                        }
                    }
                    "aliasOf" => match annotation.args.as_slice() {
                        [arg] if arg.key.is_none() && matches!(arg.value, Value::Ident(_)) => {
                            let Value::Ident(primary) = &arg.value else {
                                unreachable!("matched")
                            };
                            alias_of = Some(primary.name.clone());
                        }
                        _ => self.error(
                            file,
                            annotation.span,
                            "`@aliasOf` names the primary permission's symbol: `@aliasOf(mailSend)`",
                        ),
                    },
                    "deprecated" | "since" => notes.push(annotation.clone()),
                    _ => self.error(
                        file,
                        annotation.span,
                        format!(
                            "`@{word}` does not apply to a permission; a permission takes {}",
                            PERMISSION_ANNOTATIONS
                                .iter()
                                .map(|a| format!("`@{a}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    ),
                }
            }
            if tiers.len() > 1 {
                self.error(
                    file,
                    entry.span,
                    format!(
                        "permissions.tier: a permission is in one tier; {} exclude each other",
                        tiers
                            .iter()
                            .map(|(_, w)| *w)
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                );
            }
            let notes = self
                .annotations(file, &notes, &[], Position::EnumValue)
                .notes;
            permissions.push(Permission {
                symbol: entry.name.name.clone(),
                value: entry.value.value.clone(),
                tier: tiers.first().map_or(Tier::Public, |(t, _)| *t),
                alias_of,
                docs: self.doc(file, entry.doc.as_ref()),
                notes,
            });
        }
        let identifiers = {
            let (inherited, own) = self.identifiers(&resource.name.name);
            inherited.into_iter().chain(own).collect()
        };
        self.declared_permissions.insert(
            id,
            ResourcePermissions {
                name,
                name_declared,
                identifiers,
                permissions,
            },
        );
    }

    /// `@authorizationName("inbox")`, else the shape name in lowerCamelCase.
    fn authorization_name(
        &mut self,
        file: usize,
        decl: DeclRef<'a>,
        resource: &Resource,
    ) -> (String, bool) {
        let derived = derived_authorization_name(&resource.name.name);
        let Some(annotation) = decl
            .item
            .annotations
            .iter()
            .find(|a| a.name.name == "authorizationName")
        else {
            return (derived, false);
        };
        match annotation.args.as_slice() {
            [arg] if arg.key.is_none() => match &arg.value {
                Value::Str(name) => (name.value.clone(), true),
                _ => {
                    self.error(
                        file,
                        annotation.span,
                        "`@authorizationName` takes the name as a string: `@authorizationName(\"inbox\")`",
                    );
                    (derived, false)
                }
            },
            _ => {
                self.error(
                    file,
                    annotation.span,
                    "`@authorizationName` takes the name as a string: `@authorizationName(\"inbox\")`",
                );
                (derived, false)
            }
        }
    }

    /// The service's permissions (its reachable resources'), the facts the
    /// shared check needs, and its `@permissionTypes` enum.
    pub(super) fn collect_permissions(
        &mut self,
        reachable: &BTreeSet<String>,
    ) -> Option<PermissionTypes> {
        let decls: Vec<DeclRef<'a>> = self.decls.values().copied().collect();
        for decl in &decls {
            match &decl.item.decl {
                Decl::Resource(resource) if reachable.contains(&resource.name.name) => {
                    let id = self.ref_id(&resource.name.name);
                    let mut identifiers = Vec::new();
                    for (_, r) in self.chain(&resource.name.name).into_iter().rev() {
                        for entry in &r.entries {
                            if entry.kind == crate::ast::ResourceEntryKind::Id {
                                let ty = self
                                    .type_id(&entry.ty.name.name)
                                    .unwrap_or_else(|| self.ref_id(&entry.ty.name.name));
                                identifiers.push((entry.name.name.clone(), ty));
                            }
                        }
                    }
                    self.permission_facts
                        .resources
                        .insert(id.clone(), identifiers);
                    if let Some(permissions) = self.declared_permissions.get(&id) {
                        self.permissions.insert(id, permissions.clone());
                    }
                }
                Decl::Operation(operation) => {
                    if let Some(attachment) = &operation.attachment
                        && (matches!(attachment.role.name.as_str(), "creates" | "lists")
                            || attachment.collection)
                    {
                        let id = self.ref_id(&operation.name.name);
                        self.permission_facts.collection_operations.insert(id);
                    }
                }
                _ => {}
            }
        }
        let mut types: Option<PermissionTypes> = None;
        for decl in &decls {
            let Decl::Enum(e) = &decl.item.decl else {
                continue;
            };
            let Some(annotation) = decl
                .item
                .annotations
                .iter()
                .find(|a| a.name.name == "permissionTypes")
            else {
                continue;
            };
            if decl.ns != self.ns {
                continue;
            }
            let file = decl.file;
            if let Some(first) = &types {
                self.error(
                    file,
                    annotation.span,
                    format!(
                        "permissions.types: a service has one `@permissionTypes` enum, and `{}` is one",
                        first.shape.name()
                    ),
                );
                continue;
            }
            if e.base.is_some() {
                self.error(
                    file,
                    annotation.span,
                    "permissions.types: `@permissionTypes` applies to a string enum",
                );
                continue;
            }
            let mut excluded = Vec::new();
            for arg in &annotation.args {
                match (&arg.key, &arg.value) {
                    (Some(Key::Name(key)), Value::List(items, _)) if key.name == "exclude" => {
                        for item in items {
                            match item {
                                Value::Ident(name) => excluded.push(self.ref_id(&name.name)),
                                other => self.error(
                                    file,
                                    other.span(),
                                    "`exclude` lists resources by name",
                                ),
                            }
                        }
                    }
                    _ => self.error(
                        file,
                        arg.span,
                        "`@permissionTypes` takes only `exclude: [Resource, ..]`",
                    ),
                }
            }
            let id = self.ref_id(&e.name.name);
            self.permission_sites.insert(
                id.clone(),
                Location {
                    file,
                    span: annotation.span,
                },
            );
            self.visit(&id);
            types = Some(PermissionTypes {
                shape: id,
                excluded,
            });
        }
        types
    }

    /// An operation's `requires` and `filters` clauses, lowered.
    pub(super) fn requirements(
        &mut self,
        file: usize,
        id: &ShapeId,
        operation: &'a Operation,
    ) -> Requirements {
        let clauses: Vec<&crate::ast::Requirement> = operation
            .entries
            .iter()
            .filter_map(|entry| match entry {
                OpEntry::Requirement(r) => Some(r),
                _ => None,
            })
            .collect();
        if clauses.is_empty() {
            return Requirements::Undeclared;
        }
        let standalone: Vec<&crate::ast::Requirement> = clauses
            .iter()
            .copied()
            .filter(|c| {
                matches!(
                    c.kind,
                    RequirementKind::None(_) | RequirementKind::Pending(_)
                )
            })
            .collect();
        if let Some(first) = standalone.first() {
            if clauses.len() > 1 {
                let word = match first.kind {
                    RequirementKind::None(_) => "`requires none`",
                    _ => "`requires pending`",
                };
                for clause in clauses.iter().filter(|c| !std::ptr::eq(**c, *first)) {
                    self.error(
                        file,
                        clause.span,
                        format!(
                            "permissions.exclusive: {word} stands alone; an operation with it declares no other requirement"
                        ),
                    );
                }
            }
            return match &first.kind {
                RequirementKind::None(_) => {
                    self.doc(file, first.doc.as_ref());
                    Requirements::None
                }
                RequirementKind::Pending(reason) => {
                    self.doc(file, first.doc.as_ref());
                    Requirements::Pending {
                        reason: reason.value.clone(),
                    }
                }
                _ => unreachable!("filtered"),
            };
        }
        let input = operation.entries.iter().find_map(|entry| match entry {
            OpEntry::Body(body) if body.kind == BodyKind::Input => Some(body),
            _ => None,
        });
        // The identifiers the input binds: those of the operation's own
        // resource, inherited ones included, each by the member `$name` (or
        // its `as` spelling), else by a member of that name. `$x` names one
        // of these (check 10); any other member is bound by its path.
        let own = operation
            .attachment
            .as_ref()
            .map(|a| self.ref_id(&a.resource.name));
        let chain: Vec<String> = match &own {
            Some(own) => match self.permission_facts.resources.get(own) {
                Some(ids) => ids.iter().map(|(n, _)| n.clone()).collect(),
                None => {
                    let (inherited, own) = self.identifiers(own.name());
                    inherited.into_iter().chain(own).collect()
                }
            },
            None => Vec::new(),
        };
        let mut bound: BTreeMap<String, String> = BTreeMap::new();
        for member in input.map(|b| b.members.as_slice()).unwrap_or_default() {
            match &member.kind {
                MemberKind::Elided { name, alias, .. } if chain.contains(&name.name) => {
                    bound.insert(
                        name.name.clone(),
                        alias.as_ref().unwrap_or(name).name.clone(),
                    );
                }
                MemberKind::Field { name, .. } if chain.contains(&name.name) => {
                    bound
                        .entry(name.name.clone())
                        .or_insert_with(|| name.name.clone());
                }
                _ => {}
            }
        }
        if own.is_some() {
            self.permission_facts
                .identifier_members
                .insert(id.clone(), bound.clone());
        }
        let member_of = |identifier: &str| -> Vec<String> {
            vec![
                bound
                    .get(identifier)
                    .map_or(identifier, String::as_str)
                    .to_owned(),
            ]
        };
        let mut lowered = Vec::new();
        for clause in clauses {
            let index = lowered.len();
            let (mode, permission, resource_name, target, when) = match &clause.kind {
                RequirementKind::Filters {
                    permission,
                    resource,
                } => (Mode::Filter, permission, Some(resource), None, None),
                RequirementKind::Requires {
                    permission,
                    target,
                    when,
                    handler,
                } => {
                    let mode = if handler.is_some() {
                        Mode::Handler
                    } else {
                        Mode::Check
                    };
                    (
                        mode,
                        permission,
                        target.as_ref().map(|t| &t.resource),
                        target.as_ref(),
                        when.as_ref(),
                    )
                }
                _ => unreachable!("standalone clauses returned above"),
            };
            let resource = match (resource_name, &operation.attachment) {
                (Some(name), _) => self.ref_id(&name.name),
                (None, Some(attachment)) => {
                    let collection = matches!(attachment.role.name.as_str(), "creates" | "lists")
                        || attachment.collection;
                    if collection && mode != Mode::Handler {
                        self.error(
                            file,
                            clause.span,
                            format!(
                                "permissions.target: `{}` is a collection operation, so it has no `{}` instance: write `on {}(*)` to require it on every one, or name a parent",
                                operation.name.name, attachment.resource.name, attachment.resource.name
                            ),
                        );
                        continue;
                    }
                    self.ref_id(&attachment.resource.name)
                }
                (None, None) => {
                    self.error(
                        file,
                        clause.span,
                        format!(
                            "permissions.target: `{}` is attached to no resource, so name the one the permission is on: `requires {} on Resource`",
                            operation.name.name, permission.name
                        ),
                    );
                    continue;
                }
            };
            let identifiers: Vec<String> = self
                .permission_facts
                .resources
                .get(&resource)
                .map(|ids| ids.iter().map(|(n, _)| n.clone()).collect())
                .unwrap_or_else(|| {
                    let (inherited, own) = self.identifiers(resource.name());
                    inherited.into_iter().chain(own).collect()
                });
            // Written bindings: named ones by identifier, an unnamed one
            // binds the resource's own (last) identifier.
            let mut written: BTreeMap<String, Binding> = BTreeMap::new();
            let mut unnamed = 0;
            for arg in target
                .and_then(|t| t.bindings.as_ref())
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                let binding = match &arg.value {
                    BindingValue::All(_) => Binding::All,
                    BindingValue::Label(label) => {
                        if !bound.contains_key(&label.name) {
                            let binds = if bound.is_empty() {
                                "the input binds no identifier".to_owned()
                            } else {
                                format!(
                                    "the input binds {}",
                                    bound
                                        .keys()
                                        .map(|i| format!("`${i}`"))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            };
                            self.error(
                                file,
                                label.span,
                                format!(
                                    "permissions.binding: `${}` names an identifier the input binds, and {binds}; bind any other member by its path: `{}`",
                                    label.name, label.name
                                ),
                            );
                        }
                        Binding::Member(member_of(&label.name))
                    }
                    BindingValue::Path(path) => {
                        Binding::Member(path.parts.iter().map(|p| p.name.clone()).collect())
                    }
                };
                let identifier = match &arg.name {
                    Some(name) => {
                        if !identifiers.contains(&name.name) {
                            self.error(
                                file,
                                name.span,
                                format!(
                                    "permissions.binding: `{}` has no identifier `{}`; its identifiers are {}",
                                    resource.name(),
                                    name.name,
                                    identifiers.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(", ")
                                ),
                            );
                            continue;
                        }
                        name.name.clone()
                    }
                    None => {
                        unnamed += 1;
                        if unnamed > 1 {
                            self.error(
                                file,
                                arg.span,
                                "permissions.binding: one binding may go unnamed, and it binds the resource's own identifier; name the others: `workspaceId: $workspaceId`",
                            );
                            continue;
                        }
                        match identifiers.last() {
                            Some(last) => last.clone(),
                            None => continue,
                        }
                    }
                };
                if written.insert(identifier.clone(), binding).is_some() {
                    self.error(
                        file,
                        arg.span,
                        format!("permissions.binding: `{identifier}` is bound twice; bind each identifier once"),
                    );
                }
            }
            let last = identifiers.len().saturating_sub(1);
            let mut bindings = Vec::new();
            for (position, identifier) in identifiers.iter().enumerate() {
                let binding = match written.remove(identifier) {
                    Some(binding) => binding,
                    None if mode == Mode::Handler => continue,
                    None if mode == Mode::Filter && position == last => Binding::All,
                    None => Binding::Member(member_of(identifier)),
                };
                bindings.push(binding);
            }
            let when = when.and_then(|c| self.condition(file, c));
            self.requirement_sites.insert(
                (id.clone(), index),
                Location {
                    file,
                    span: clause.span,
                },
            );
            lowered.push(Requirement {
                mode,
                resource,
                permission: permission.name.clone(),
                bindings,
                when,
                docs: self.doc(file, clause.doc.as_ref()),
            });
        }
        if lowered.is_empty() {
            // Every clause is reported already; none is left to check.
            return Requirements::None;
        }
        Requirements::List(lowered)
    }

    fn condition(&mut self, file: usize, condition: &Cond) -> Option<Condition> {
        let path = |dotted: &crate::ast::Dotted| -> Vec<String> {
            dotted.parts.iter().map(|p| p.name.clone()).collect()
        };
        Some(match &condition.kind {
            ConditionKind::Has(p) => Condition::Has(path(p)),
            ConditionKind::Equals(p, value) => {
                Condition::Equals(path(p), self.compared(file, value)?)
            }
            ConditionKind::In(p, value) => {
                let Value::List(items, _) = value else {
                    self.error(
                        file,
                        value.span(),
                        "`in` takes a list: `kind in [\"a\", \"b\"]`",
                    );
                    return None;
                };
                let mut values = Vec::new();
                for item in items {
                    values.push(self.compared(file, item)?);
                }
                Condition::In(path(p), values)
            }
            ConditionKind::And(parts) | ConditionKind::Or(parts) => {
                let mut lowered = Vec::new();
                for part in parts {
                    lowered.push(self.condition(file, part)?);
                }
                if matches!(condition.kind, ConditionKind::And(_)) {
                    Condition::And(lowered)
                } else {
                    Condition::Or(lowered)
                }
            }
            ConditionKind::Not(inner) => Condition::Not(Box::new(self.condition(file, inner)?)),
        })
    }

    /// A value a condition compares with: a string, number, `true` or `false`.
    fn compared(&mut self, file: usize, value: &Value) -> Option<Json> {
        match value {
            Value::Str(s) => Some(Json::String(s.value.clone())),
            Value::Number(n) => self.number(file, n).map(Json::Number),
            Value::Ident(i) if i.name == "true" => Some(Json::Bool(true)),
            Value::Ident(i) if i.name == "false" => Some(Json::Bool(false)),
            other => {
                self.error(
                    file,
                    other.span(),
                    "permissions.condition: a condition compares with a string, a number, `true` or `false`",
                );
                None
            }
        }
    }

    /// `notFound X` in the service: the service's own 404, which check 17
    /// attaches instead of the framework's. An error of the service's own
    /// package; its status is the shared check's.
    pub(super) fn designate_not_found(&mut self, file: usize, name: &Ident) {
        let problem = match self.decl(&name.name) {
            Some(decl) if matches!(decl.item.decl, Decl::Error(_)) && decl.ns == self.ns => {
                let id = self.ref_id(&name.name);
                // It travels in responses, so its members take the
                // response timestamp format.
                self.http_outputs.insert(id.clone());
                self.visit(&id);
                self.not_found = Some(id);
                return;
            }
            Some(decl) if matches!(decl.item.decl, Decl::Error(_)) => format!(
                "permissions.not-found-error: `{}` is declared in `{}`; the service's not-found error is an error of its own package",
                name.name, decl.ns
            ),
            _ => format!(
                "permissions.not-found-error: `{}` is not an error; `notFound` names the error the service answers a missing resource with",
                name.name
            ),
        };
        self.error(file, name.span, problem);
    }

    /// Check 17: the service's not-found error (its own, or the framework
    /// NotFound) joins the operation's own errors.
    pub(super) fn attach_not_found(
        &mut self,
        requirements: &Requirements,
        own: usize,
        errors: &mut Vec<ShapeId>,
    ) {
        if !attaches_not_found(self.not_found.is_some(), requirements, &self.permissions) {
            return;
        }
        let id = match &self.not_found {
            Some(id) => id.clone(),
            None => {
                let id = not_found_id();
                let bundled = self.decl(id.name()).is_some_and(|decl| {
                    decl.ns == id.namespace() && matches!(decl.item.decl, Decl::Error(_))
                });
                // Without it the shared check reports the missing import.
                if !bundled {
                    return;
                }
                id
            }
        };
        if !errors.contains(&id) {
            self.visit(&id);
            errors.insert(own.min(errors.len()), id);
        }
    }

    /// The shared checks, each finding reported at the clause, permission or
    /// resource it is about.
    pub(super) fn check_permissions(&mut self, service: &Service) {
        let findings = crucible_smithy::validate::permissions(service, &self.permission_facts);
        for finding in findings {
            let location = finding
                .requirement
                .and_then(|index| self.requirement_sites.get(&(finding.shape.clone(), index)))
                .or_else(|| self.permission_sites.get(&finding.shape))
                .or_else(|| self.source_map.get(&finding.shape))
                .copied();
            let message = finding.text();
            let diagnostic = match (location, finding.severity) {
                (Some(Location { span, .. }), FindingSeverity::Error) => {
                    Diagnostic::new(span, message)
                }
                (Some(Location { span, .. }), FindingSeverity::Warning) => {
                    Diagnostic::warning(span, message)
                }
                (Some(Location { span, .. }), FindingSeverity::Advice) => {
                    Diagnostic::advice(span, message)
                }
                // A severity added later reads as a warning until it is mapped.
                (Some(Location { span, .. }), _) => Diagnostic::warning(span, message),
                (None, severity) => {
                    let message = format!("{}: {message}", finding.shape);
                    let span = crate::span::Span::default();
                    match severity {
                        FindingSeverity::Error => Diagnostic::new(span, message),
                        FindingSeverity::Advice => Diagnostic::advice(span, message),
                        _ => Diagnostic::warning(span, message),
                    }
                }
            };
            self.diagnostics.push(FileDiagnostic {
                file: location.map(|l| l.file),
                diagnostic,
            });
        }
    }
}

/// Whether an annotation on a resource is one the compiler supports there.
pub(super) fn resource_annotation(annotation: &Annotation) -> bool {
    annotation.name.name == "authorizationName"
}
