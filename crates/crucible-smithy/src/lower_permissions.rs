//! The `crucible.auth` permission traits (docs/permissions.md §4.11),
//! lowered to the same IR the `.cru` front end produces. What holds of the
//! IR is checked by `validate::permissions`; this module reports only what
//! the traits themselves get wrong.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::ast::{Kind, Traits};
use crate::ir::permissions::Binding;
use crate::ir::{
    Bindings, DocNotes, Mode, Permission, PermissionTypes, Requirement, Requirements,
    ResourcePermissions, Tier,
};
use crate::lower::Lowerer;
use crate::permission_syntax::{self, BindingText};
use crate::shape_id::ShapeId;
use crate::validate::{derived_authorization_name, needs_not_found, not_found_id};

fn auth(name: &str) -> ShapeId {
    ShapeId::new("crucible.auth", name)
}

fn smithy(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

impl Lowerer<'_> {
    /// The service's resource tree (identifier types root to leaf, and
    /// collection operations), every `@permissionsOf` enum for one of its
    /// resources, and the `@permissionTypes` enum.
    pub(crate) fn lower_permissions(
        &mut self,
        service: &crate::ast::Shape,
    ) -> Option<PermissionTypes> {
        // The resource tree, parents first.
        let mut pending: Vec<(ShapeId, Vec<(String, ShapeId)>)> = service
            .resources
            .iter()
            .rev()
            .map(|r| (r.target.clone(), Vec::new()))
            .collect();
        let mut seen = BTreeSet::new();
        while let Some((id, inherited)) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(resource) = self.model.get(&id) else {
                continue;
            };
            let mut identifiers = inherited.clone();
            for (name, target) in &resource.identifiers {
                if !identifiers.iter().any(|(n, _)| n == name) {
                    identifiers.push((name.clone(), target.target.clone()));
                }
            }
            for operation in resource
                .create
                .iter()
                .chain(&resource.list)
                .chain(&resource.collection_operations)
            {
                self.permission_facts
                    .collection_operations
                    .insert(operation.target.clone());
            }
            for child in resource.resources.iter().rev() {
                pending.push((child.target.clone(), identifiers.clone()));
            }
            self.permission_facts.resources.insert(id, identifiers);
        }

        let enums: Vec<(ShapeId, Value)> = self
            .model
            .iter()
            .filter_map(|(id, shape)| {
                shape
                    .traits
                    .get(&auth("permissionsOf"))
                    .map(|value| (id.clone(), value.clone()))
            })
            .collect();
        for (enum_id, value) in enums {
            self.permissions_of(&enum_id, &value);
        }

        let mut types: Option<PermissionTypes> = None;
        let declared: Vec<(ShapeId, Value)> = self
            .model
            .iter()
            .filter_map(|(id, shape)| {
                shape
                    .traits
                    .get(&auth("permissionTypes"))
                    .map(|value| (id.clone(), value.clone()))
            })
            .collect();
        for (id, value) in declared {
            if self.model[&id].kind != Kind::Enum {
                self.diagnostics.push(
                    &id,
                    "permissions.types: @crucible.auth#permissionTypes applies to a string enum",
                );
                continue;
            }
            if let Some(first) = &types {
                self.diagnostics.push(
                    &id,
                    format!(
                        "permissions.types: a service has one @permissionTypes enum; {} is one already",
                        first.shape
                    ),
                );
                continue;
            }
            let mut excluded = Vec::new();
            match value.get("exclude") {
                None => {}
                Some(Value::Array(items)) => {
                    for item in items {
                        match item.as_str().map(str::parse::<ShapeId>) {
                            Some(Ok(resource)) => excluded.push(resource),
                            _ => self
                                .diagnostics
                                .push(&id, "permissions.types: `exclude` lists resource shape ids"),
                        }
                    }
                }
                Some(_) => self
                    .diagnostics
                    .push(&id, "permissions.types: `exclude` lists resource shape ids"),
            }
            self.visit(&id);
            types = Some(PermissionTypes {
                shape: id,
                excluded,
            });
        }
        types
    }

    fn permissions_of(&mut self, enum_id: &ShapeId, value: &Value) {
        let shape = &self.model[enum_id];
        if shape.kind != Kind::Enum {
            self.diagnostics.push(
                enum_id,
                "permissions.vocabulary: @crucible.auth#permissionsOf applies to a string enum",
            );
            return;
        }
        let Some(resource) = value.as_str().and_then(|s| s.parse::<ShapeId>().ok()) else {
            self.diagnostics.push(
                enum_id,
                "permissions.vocabulary: @crucible.auth#permissionsOf names a resource shape id",
            );
            return;
        };
        match self.model.get(&resource) {
            Some(target) if target.kind == Kind::Resource => {}
            _ => {
                self.diagnostics.push(
                    enum_id,
                    format!("permissions.vocabulary: {resource} is not a resource"),
                );
                return;
            }
        }
        // A resource of another service in the same model is that service's.
        let Some(identifiers) = self.permission_facts.resources.get(&resource) else {
            return;
        };
        let identifiers: Vec<String> = identifiers.iter().map(|(n, _)| n.clone()).collect();
        if let Some(existing) = self
            .permission_facts
            .declared_at
            .iter()
            .find(|((r, _), _)| r == &resource)
            .map(|(_, site)| site.root())
        {
            self.diagnostics.push(
                enum_id,
                format!(
                    "permissions.vocabulary: {existing} already declares the permissions of {resource}; one enum per resource"
                ),
            );
            return;
        }
        let resource_shape = &self.model[&resource];
        let (name, name_declared) = match resource_shape.traits.get(&auth("authorizationName")) {
            Some(Value::String(name)) => (name.clone(), true),
            Some(_) => {
                self.diagnostics.push(
                    &resource,
                    "permissions.name: @crucible.auth#authorizationName is a string",
                );
                (derived_authorization_name(resource.name()), false)
            }
            None => (derived_authorization_name(resource.name()), false),
        };
        let members: Vec<(String, Traits)> = shape
            .members
            .iter()
            .map(|(name, member)| (name.clone(), member.traits.clone()))
            .collect();
        let mut permissions = Vec::new();
        for (symbol, traits) in members {
            let member_id = enum_id.with_member(&symbol);
            let value = match traits.get(&smithy("enumValue")) {
                Some(Value::String(value)) => value.clone(),
                Some(_) => {
                    self.diagnostics.push(
                        &member_id,
                        "permissions.value: a permission's value is a string",
                    );
                    continue;
                }
                None => symbol.clone(),
            };
            let tiers: Vec<(Tier, &str)> = [
                (Tier::Internal, "@internal"),
                (Tier::PermissionOnly, "@permissionOnly"),
                (Tier::NotGrantable, "@notGrantable"),
            ]
            .into_iter()
            .filter(|(tier, _)| {
                traits.contains_key(&match tier {
                    Tier::Internal => smithy("internal"),
                    Tier::PermissionOnly => auth("permissionOnly"),
                    _ => auth("notGrantable"),
                })
            })
            .collect();
            if tiers.len() > 1 {
                self.diagnostics.push(
                    &member_id,
                    format!(
                        "permissions.tier: a permission is in one tier; {} exclude each other",
                        tiers
                            .iter()
                            .map(|(_, a)| *a)
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                );
            }
            let alias_of = match traits.get(&auth("aliasOf")) {
                None => None,
                Some(Value::String(primary)) => Some(primary.clone()),
                Some(_) => {
                    self.diagnostics.push(
                        &member_id,
                        "permissions.alias: @crucible.auth#aliasOf names the primary enum member",
                    );
                    None
                }
            };
            self.permission_facts
                .declared_at
                .insert((resource.clone(), symbol.clone()), member_id.clone());
            permissions.push(Permission {
                symbol,
                value,
                tier: tiers.first().map_or(Tier::Public, |(tier, _)| *tier),
                alias_of,
                docs: traits
                    .get(&smithy("documentation"))
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                notes: DocNotes::lower(&member_id, &traits, &mut self.diagnostics),
            });
        }
        self.permissions.insert(
            resource,
            ResourcePermissions {
                name,
                name_declared,
                identifiers,
                permissions,
            },
        );
    }

    /// An operation's `@requires`, `@requiresNone` or `@requiresPending`.
    pub(crate) fn requirements_of(
        &mut self,
        id: &ShapeId,
        traits: &Traits,
        input: Option<&ShapeId>,
        request: &Bindings,
    ) -> Requirements {
        let declared: Vec<&str> = ["requires", "requiresNone", "requiresPending"]
            .into_iter()
            .filter(|name| traits.contains_key(&auth(name)))
            .collect();
        if declared.len() > 1 {
            self.diagnostics.push(
                id,
                format!(
                    "permissions.exclusive: {} exclude each other; `requiresNone` and `requiresPending` stand alone",
                    declared.iter().map(|d| format!("@{d}")).collect::<Vec<_>>().join(", ")
                ),
            );
        }
        if let Some(value) = traits.get(&auth("requiresNone")) {
            if !value.is_string() {
                self.diagnostics.push(
                    id,
                    "permissions.exclusive: @crucible.auth#requiresNone takes the reason as a string",
                );
            }
            return Requirements::None;
        }
        if let Some(value) = traits.get(&auth("requiresPending")) {
            let reason = match value.as_str() {
                Some(reason) => reason.to_owned(),
                None => {
                    self.diagnostics.push(
                        id,
                        "permissions.reason: @crucible.auth#requiresPending takes the reason as a string",
                    );
                    "?".to_owned()
                }
            };
            return Requirements::Pending { reason };
        }
        let Some(value) = traits.get(&auth("requires")) else {
            return Requirements::Undeclared;
        };
        let Some(entries) = value.as_array() else {
            self.diagnostics.push(
                id,
                "permissions.requires: @crucible.auth#requires is a list of requirements",
            );
            return Requirements::List(Vec::new());
        };
        if entries.is_empty() {
            self.diagnostics.push(
                id,
                "permissions.requires: @crucible.auth#requires lists no requirement; an operation that needs no permission says @requiresNone(\"why\")",
            );
        }
        let mut clauses = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if let Some(clause) = self.requirement(id, index, entry, input, request) {
                clauses.push(clause);
            }
        }
        Requirements::List(clauses)
    }

    fn requirement(
        &mut self,
        id: &ShapeId,
        index: usize,
        entry: &Value,
        input: Option<&ShapeId>,
        request: &Bindings,
    ) -> Option<Requirement> {
        let at = format!("@requires[{index}]");
        let Some(entry) = entry.as_object() else {
            self.diagnostics.push(
                id,
                format!("permissions.requires: {at} is an object with `permission` and `resource`"),
            );
            return None;
        };
        for key in entry.keys() {
            if !matches!(
                key.as_str(),
                "permission" | "resource" | "bind" | "when" | "mode" | "documentation"
            ) {
                self.diagnostics.push(
                    id,
                    format!("permissions.requires: {at} has an unknown key `{key}`; the keys are permission, resource, bind, when, mode and documentation"),
                );
            }
        }
        let text = |key: &str| entry.get(key).and_then(Value::as_str);
        let (Some(value), Some(resource)) = (text("permission"), text("resource")) else {
            self.diagnostics.push(
                id,
                format!("permissions.requires: {at} names a `permission` value and a `resource`"),
            );
            return None;
        };
        let Ok(resource) = resource.parse::<ShapeId>() else {
            self.diagnostics.push(
                id,
                format!("permissions.requires: {at}: `{resource}` is not a shape id"),
            );
            return None;
        };
        let mode = match text("mode") {
            None | Some("check") => Mode::Check,
            Some("filter") => Mode::Filter,
            Some("handler") => Mode::Handler,
            Some(other) => {
                self.diagnostics.push(
                    id,
                    format!("permissions.requires: {at}: mode {other:?} is one of \"check\", \"filter\", \"handler\""),
                );
                Mode::Check
            }
        };
        // The value names the permission; the IR names its symbol.
        let permission = match self.permissions.get(&resource) {
            Some(permissions) => match permissions.by_value(value) {
                Some(p) => p.symbol.clone(),
                None => {
                    let values: Vec<String> = permissions
                        .permissions
                        .iter()
                        .map(|p| format!("{:?}", p.value))
                        .collect();
                    self.diagnostics.push(
                        id,
                        format!(
                            "permissions.unknown: {at}: {} declares no permission {value:?}; its values are {}",
                            resource.name(),
                            values.join(", ")
                        ),
                    );
                    return None;
                }
            },
            // Reported by the shared check, which knows what it is.
            None => value.to_owned(),
        };
        let when = match entry.get("when") {
            None => None,
            Some(Value::String(text)) => match permission_syntax::condition(text) {
                Ok(condition) => Some(condition),
                Err(problem) => {
                    self.diagnostics.push(
                        id,
                        format!("permissions.condition: {at}: `when` {text:?}: {problem}"),
                    );
                    None
                }
            },
            Some(_) => {
                self.diagnostics.push(
                    id,
                    format!("permissions.condition: {at}: `when` is a string"),
                );
                None
            }
        };
        let mut written: BTreeMap<String, BindingText> = BTreeMap::new();
        match entry.get("bind") {
            None => {}
            Some(Value::Object(bind)) => {
                for (identifier, text) in bind {
                    match text.as_str().map(permission_syntax::binding) {
                        Some(Ok(binding)) => {
                            written.insert(identifier.clone(), binding);
                        }
                        Some(Err(problem)) => self.diagnostics.push(
                            id,
                            format!("permissions.binding: {at}: `{identifier}`: {problem}"),
                        ),
                        None => self.diagnostics.push(
                            id,
                            format!(
                                "permissions.binding: {at}: `{identifier}` is bound by a string"
                            ),
                        ),
                    }
                }
            }
            Some(_) => self.diagnostics.push(
                id,
                format!("permissions.binding: {at}: `bind` maps identifiers to bindings"),
            ),
        }
        let identifiers: Vec<String> = self
            .permission_facts
            .resources
            .get(&resource)
            .map(|ids| ids.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default();
        for name in written.keys() {
            if !identifiers.contains(name) {
                self.diagnostics.push(
                    id,
                    format!(
                        "permissions.binding: {at}: `{}` has no identifier `{name}`; its identifiers are {}",
                        resource.name(),
                        identifiers.join(", ")
                    ),
                );
            }
        }
        let mut bindings = Vec::new();
        let handler = mode == Mode::Handler;
        let last = identifiers.len().saturating_sub(1);
        for (position, identifier) in identifiers.iter().enumerate() {
            let binding = match written.get(identifier) {
                Some(BindingText::All) => Binding::All,
                Some(BindingText::Path(path)) => Binding::Member(path.clone()),
                Some(BindingText::Label(label)) => {
                    if !request.labels.contains(label) {
                        self.diagnostics.push(
                            id,
                            format!(
                                "permissions.binding: {at}: `${label}` names no URI label of the operation"
                            ),
                        );
                    }
                    Binding::Member(vec![label.clone()])
                }
                // `by handler` binds nothing: the handler supplies the resource.
                None if handler => continue,
                None if mode == Mode::Filter && position == last => Binding::All,
                None => Binding::Member(vec![self.identifier_member(input, identifier)]),
            };
            bindings.push(binding);
        }
        Some(Requirement {
            mode,
            resource,
            permission,
            bindings,
            when,
            docs: text("documentation").map(str::to_owned),
        })
    }

    /// The input member that carries `identifier`: the one marked
    /// `@resourceIdentifier` for it, else the one of its name.
    fn identifier_member(&self, input: Option<&ShapeId>, identifier: &str) -> String {
        input
            .and_then(|input| self.model.get(input))
            .and_then(|input| {
                input.members.iter().find_map(|(name, member)| {
                    (member
                        .traits
                        .get(&smithy("resourceIdentifier"))
                        .and_then(Value::as_str)
                        == Some(identifier))
                    .then(|| name.clone())
                })
            })
            .unwrap_or_else(|| identifier.to_owned())
    }

    /// Check 17: the framework NotFound joins the operation's own errors.
    pub(crate) fn attach_not_found(
        &mut self,
        requirements: &Requirements,
        own: usize,
        errors: &mut Vec<ShapeId>,
    ) {
        if !needs_not_found(requirements, &self.permissions) {
            return;
        }
        let id = not_found_id();
        // A model without it is reported by the shared check.
        if self.model.contains_key(&id) && !errors.contains(&id) {
            self.visit(&id);
            errors.insert(own.min(errors.len()), id);
        }
    }
}
