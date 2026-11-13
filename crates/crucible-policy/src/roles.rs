//! Role compatibility (docs/roles.md §6). A binding stores a role's value
//! and a scope's value, with the instance the scope names, so removing a
//! role or scope, or changing its value, orphans the bindings stored under
//! it. Holders rely on what a role grants: one that grants less, binds at
//! fewer scopes or admits fewer kinds of principal refuses them what they
//! had. One that grants more, by an edit or because its `*` covers a new
//! permission, is a security review, reported and never refused.
//!
//! Roles and scopes are matched by value, what bindings store, not by
//! symbol, and grants are compared by the stored pair (authorization name,
//! permission value), as requirements are: a grant whose value is now a
//! deprecated alias of a primary the role grants is kept. A permission
//! that is gone altogether is `compatibility.permissions`' finding, and a
//! scope that is gone `compatibility.scope`'s, so neither is reported again
//! for every role that granted or bound at it.

use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{GrantedSet, Role, ScopeTarget, Service, Shape};
use crucible_ir::roles::{Target, covers, parent, with_included};

use crate::Severity;
use crate::checker::Checker;

/// Where a role or scope finding is reported, and what a `[[exceptions]]`
/// entry names: `namespace#symbol` in the service's namespace. A role or
/// scope is a declaration of its own, and declaration names are unique in a
/// namespace, so the id names one of them.
pub fn role_id(service: &Service, symbol: &str) -> ShapeId {
    ShapeId::new(service.id.namespace(), symbol)
}

/// Whether the role `symbol` of `old` still names a role of `new` by its
/// value, and that role grants less, binds at fewer scopes, admits fewer
/// kinds of principal, or leaves a grant to the service: what a migration's
/// `narrow role` declares.
pub fn role_narrowed(old: &Service, new: &Service, symbol: &str) -> bool {
    let Some(role) = old.role(symbol) else {
        return false;
    };
    if !new.roles.iter().any(|r| r.value == role.value) {
        return false;
    }
    let site = role_id(old, symbol);
    changes(old, new)
        .iter()
        .any(|change| change.rule == "compatibility.role" && change.site == site)
}

/// One role or scope change, at the old declaration.
pub(crate) struct Change {
    pub(crate) site: ShapeId,
    pub(crate) rule: &'static str,
    /// An error at the configured severity; otherwise a warning, reported
    /// for review.
    pub(crate) breaking: bool,
    pub(crate) message: String,
}

impl Checker<'_> {
    /// Stored bindings and what their holders may do.
    pub(crate) fn roles_wire(&mut self) {
        for change in changes(self.old, self.new) {
            if change.breaking {
                self.push(change.rule, &change.site, change.message);
            } else if self.severity != Severity::Off {
                self.report
                    .push(change.rule, &change.site, Severity::Warning, change.message);
            }
        }
    }

    /// Generated SDK constants, by symbol, as permissions' are.
    pub(crate) fn roles_sdk(&mut self) {
        for old in self.old.roles.iter().filter(|r| !r.internal) {
            let site = role_id(self.old, &old.symbol);
            match self.new.role(&old.symbol) {
                None => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "the role constant `{}` was removed from client SDKs",
                        old.symbol
                    ),
                ),
                Some(now) if now.internal => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "`{}` became internal, so client SDKs lose its constant",
                        old.symbol
                    ),
                ),
                // A client's constant sends its value: kept under a new
                // value, it asks for another role, though stored bindings
                // may still match (two swapped, bodies and all).
                Some(now) if now.value != old.value => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "the role constant `{}` now sends {:?}, where it sent {:?}: a client that still names it asks for another role",
                        old.symbol, now.value, old.value
                    ),
                ),
                Some(_) => {}
            }
        }
        for old in self.old.role_scopes.iter().filter(|s| !s.internal) {
            let site = role_id(self.old, &old.symbol);
            match self.new.scope(&old.symbol) {
                None => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "the scope constant `{}` was removed from client SDKs",
                        old.symbol
                    ),
                ),
                Some(now) if now.internal => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "`{}` became internal, so client SDKs lose its constant",
                        old.symbol
                    ),
                ),
                // A client's constant sends its value: kept under a new
                // value, it asks for another scope, though stored bindings
                // may still match (two swapped, bodies and all).
                Some(now) if now.value != old.value => self.push(
                    "compatibility.sdk",
                    &site,
                    format!(
                        "the scope constant `{}` now sends {:?}, where it sent {:?}: a client that still names it asks for another scope",
                        old.symbol, now.value, old.value
                    ),
                ),
                Some(_) => {}
            }
        }
    }
}

/// Every role and scope change from `old` to `new`, scopes first, each
/// catalogue in its declaration order. A baseline without roles or scopes
/// has none: a catalogue added since is new, not changed.
pub(crate) fn changes(old: &Service, new: &Service) -> Vec<Change> {
    let mut out = Vec::new();
    for scope in &old.role_scopes {
        let site = role_id(old, &scope.symbol);
        let Some(now) = new.role_scopes.iter().find(|s| s.value == scope.value) else {
            let message = match new.scope(&scope.symbol) {
                Some(renamed) => format!(
                    "`{}` changed its value from {:?} to {:?}; bindings stored at the old value no longer match it: keep the value",
                    scope.symbol, scope.value, renamed.value
                ),
                None => format!(
                    "the scope {:?} (`{}`) was removed; bindings stored at it no longer match: keep it, and mark it `@deprecated`",
                    scope.value, scope.symbol
                ),
            };
            out.push(Change {
                site,
                rule: "compatibility.scope",
                breaking: true,
                message,
            });
            continue;
        };
        let (was, is) = (target(old, &scope.on), target(new, &now.on));
        if !same_target(&was, &is) {
            out.push(Change {
                site,
                rule: "compatibility.scope",
                breaking: true,
                message: format!(
                    "the scope {:?} (`{}`) is now on {}, where it was on {}; bindings stored at it name an instance of what it was on",
                    scope.value,
                    scope.symbol,
                    describe_target(&now.on),
                    describe_target(&scope.on)
                ),
            });
            continue;
        }
        let (was, is) = (instance(old, &scope.on), instance(new, &now.on));
        if !same_path(&was, &is) {
            out.push(Change {
                site,
                rule: "compatibility.scope",
                breaking: true,
                message: format!(
                    "the scope {:?} (`{}`) now names its instance as {}, where it named it as {}; bindings stored at it carry the old path",
                    scope.value,
                    scope.symbol,
                    describe_instance(&is),
                    describe_instance(&was)
                ),
            });
        }
    }
    for role in &old.roles {
        let site = role_id(old, &role.symbol);
        let Some(now) = new.roles.iter().find(|r| r.value == role.value) else {
            let message = match new.role(&role.symbol) {
                Some(renamed) => format!(
                    "`{}` changed its value from {:?} to {:?}; bindings stored under the old value no longer match it: keep the value",
                    role.symbol, role.value, renamed.value
                ),
                None => format!(
                    "the role {:?} (`{}`) was removed; bindings stored under it no longer match: keep it, and mark it `@deprecated`",
                    role.value, role.symbol
                ),
            };
            out.push(Change {
                site,
                rule: "compatibility.role",
                breaking: true,
                message,
            });
            continue;
        };
        let mut role_changes = RoleChanges {
            old,
            new,
            role,
            now,
            site,
            out: &mut out,
        };
        role_changes.scopes();
        role_changes.holders();
        role_changes.grants();
        role_changes.coverage();
        role_changes.assigners();
    }
    out
}

/// A resource as a stored binding's scope instance names it.
#[derive(Debug, Clone)]
struct Named {
    shape: ShapeId,
    /// Its authorization name, when it declares permissions.
    authorization: Option<String>,
}

impl Named {
    fn of(service: &Service, id: &ShapeId) -> Self {
        Self {
            shape: id.clone(),
            authorization: service.permissions.get(id).map(|r| r.name.clone()),
        }
    }

    /// The same resource in two models: by authorization name when both
    /// declare permissions, so a renamed shape that keeps it is the same;
    /// otherwise by shape, so a resource that gains its first permission
    /// (and with it an authorization name) has not moved.
    fn same(&self, other: &Self) -> bool {
        match (&self.authorization, &other.authorization) {
            (Some(a), Some(b)) => a == b,
            _ => self.shape == other.shape,
        }
    }

    fn label(&self) -> &str {
        self.authorization
            .as_deref()
            .unwrap_or_else(|| self.shape.name())
    }
}

/// What a scope's instance names, as stored data: the service, or one
/// instance of a resource, or every one under a parent.
fn target(service: &Service, on: &ScopeTarget) -> (u8, Option<Named>) {
    match on {
        ScopeTarget::Service => (0, None),
        ScopeTarget::Instance(id) => (1, Some(Named::of(service, id))),
        ScopeTarget::Every(id) => (2, Some(Named::of(service, id))),
    }
}

fn same_target(a: &(u8, Option<Named>), b: &(u8, Option<Named>)) -> bool {
    a.0 == b.0
        && match (&a.1, &b.1) {
            (Some(a), Some(b)) => a.same(b),
            (a, b) => a.is_none() && b.is_none(),
        }
}

/// One resource on the path a scope instance is named by, and the
/// identifiers it adds, in order.
type Segment = (Named, Vec<String>);

fn same_path(a: &[Segment], b: &[Segment]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|((a, ids), (b, other))| ids == other && a.same(b))
}

/// The path a scope's stored instance is named by, root first: each
/// resource from the root down to the scope's, with the identifiers it
/// adds. A `R(*)` instance is named by its parent, so its own identifier
/// is left out. A resource moved under another parent, or an identifier
/// renamed, changes what a stored instance means even where no
/// operation's route shows it.
fn instance(service: &Service, on: &ScopeTarget) -> Vec<Segment> {
    let Some(resource) = on.resource() else {
        return Vec::new();
    };
    let mut chain = vec![resource];
    // A cycle is refused where resources are lowered; the bound keeps a
    // hand-built IR from looping.
    while chain.len() <= service.resources.len() {
        match parent(service, chain[chain.len() - 1]) {
            Some(up) => chain.push(up),
            None => break,
        }
    }
    chain.reverse();
    let identifiers = |id: &ShapeId| -> BTreeSet<&String> {
        service
            .resources
            .get(id)
            .map(|r| r.identifiers.keys().collect())
            .unwrap_or_default()
    };
    let mut path = Vec::new();
    let mut inherited = BTreeSet::new();
    for id in chain {
        let all = identifiers(id);
        let own: Vec<String> = all.difference(&inherited).map(|n| (*n).clone()).collect();
        path.push((Named::of(service, id), own));
        inherited = all;
    }
    if matches!(on, ScopeTarget::Every(_))
        && let Some((_, own)) = path.last_mut()
    {
        own.clear();
    }
    path
}

/// `` `organization(organizationId)`/`graph(*)` ``.
fn describe_instance(path: &[Segment]) -> String {
    let last = path.len().saturating_sub(1);
    path.iter()
        .enumerate()
        .map(|(index, (named, own))| {
            let ids = if own.is_empty() && index == last {
                "*".to_owned()
            } else {
                own.join(", ")
            };
            format!("`{}({ids})`", named.label())
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn describe_target(on: &ScopeTarget) -> String {
    match on {
        ScopeTarget::Service => "the service".to_owned(),
        ScopeTarget::Instance(id) => format!("`{}`", id.name()),
        ScopeTarget::Every(id) => format!("`{}(*)`", id.name()),
    }
}

/// One role of the baseline against the role of the new model that has its
/// value.
struct RoleChanges<'a, 'o> {
    old: &'a Service,
    new: &'a Service,
    role: &'a Role,
    now: &'a Role,
    site: ShapeId,
    out: &'o mut Vec<Change>,
}

/// A granted permission, as the role's settled grants name it.
type Granted<'a> = (&'a ShapeId, &'a str);

/// The values a role still grants on one resource: as the baseline stores
/// them, and as the new model does.
type Kept<'a> = (Vec<&'a str>, Vec<&'a str>);

impl<'a> RoleChanges<'a, '_> {
    fn push(&mut self, rule: &'static str, breaking: bool, message: String) {
        self.out.push(Change {
            site: self.site.clone(),
            rule,
            breaking,
            message,
        });
    }

    /// Scopes it may be bound at, by value. A scope that is gone is
    /// `compatibility.scope`'s finding. One it may be bound at that it could
    /// not be gives no holder anything, but whoever may assign it can now
    /// grant it there: a security review.
    fn scopes(&mut self) {
        let values = |service: &'a Service, role: &'a Role| -> Vec<&'a str> {
            role.scopes
                .iter()
                .filter_map(|symbol| service.scope(symbol))
                .map(|scope| scope.value.as_str())
                .collect()
        };
        let now = values(self.new, self.now);
        let lost: Vec<String> = values(self.old, self.role)
            .into_iter()
            .filter(|value| !now.contains(value))
            .filter(|value| self.new.role_scopes.iter().any(|s| s.value == *value))
            .map(|value| format!("{value:?}"))
            .collect();
        if !lost.is_empty() {
            let message = format!(
                "`{}` can no longer be bound at {}; its holders bound there are refused what they had",
                self.role.symbol,
                lost.join(", ")
            );
            self.push("compatibility.role", true, message);
        }
        let was = values(self.old, self.role);
        let gained: Vec<String> = now
            .iter()
            .filter(|value| !was.contains(value))
            .map(|value| format!("{value:?}"))
            .collect();
        if !gained.is_empty() {
            let message = format!(
                "`{}` may now be bound at {}; whoever may assign it can grant it there without a new binding",
                self.role.symbol,
                gained.join(", ")
            );
            self.push("compatibility.role-widened", false, message);
        }
    }

    /// The kinds of principal that may hold it, by the `principals` enum's
    /// wire values. A role that names none is held by every kind, so it
    /// narrows when it names some; one that names some widens when it names
    /// another, or none.
    fn holders(&mut self) {
        let symbol = &self.role.symbol;
        let (was_every, is_every) = (self.role.holders.is_empty(), self.now.holders.is_empty());
        if was_every && is_every {
            return;
        }
        let (Some(was), Some(is)) = (holders(self.old, self.role), holders(self.new, self.now))
        else {
            return;
        };
        if is_every {
            // Every kind may hold it now. A binding for a kind a narrowing
            // made inert is valid again: no migration step converts stored
            // bindings, so they were kept.
            if let Holders::Only(was) = was {
                let message = format!(
                    "`{symbol}` may now be held by every kind of principal, where only {} could; bindings of the other kinds, inert until now, take effect",
                    names(was.values())
                );
                self.push("compatibility.role-widened", false, message);
            }
            return;
        }
        let Holders::Only(is) = is else {
            return;
        };
        match was {
            Holders::Every => {
                let message = format!(
                    "`{symbol}` may now be held only by {}; its other holders are refused what they had",
                    names(is.values())
                );
                self.push("compatibility.role", true, message);
            }
            Holders::Only(was) => {
                let lost: Vec<&String> = was
                    .iter()
                    .filter(|(value, _)| !is.contains_key(*value))
                    .map(|(_, name)| name)
                    .collect();
                if !lost.is_empty() {
                    let message = format!(
                        "`{symbol}` may no longer be held by {}; its holders among them are refused what they had",
                        names(lost.into_iter())
                    );
                    self.push("compatibility.role", true, message);
                }
                // A role that named every kind (no `holders`) cannot gain
                // one; one that named some can.
                if !was_every {
                    let gained: Vec<&String> = is
                        .iter()
                        .filter(|(value, _)| !was.contains_key(*value))
                        .map(|(_, name)| name)
                        .collect();
                    if !gained.is_empty() {
                        let message = format!(
                            "`{symbol}` may now be held by {}; bindings of those kinds, inert until now, take effect",
                            names(gained.into_iter())
                        );
                        self.push("compatibility.role-widened", false, message);
                    }
                }
            }
        }
    }

    /// What a binding at each scope it keeps covers of what the model still
    /// grants.
    /// Coverage follows the resource tree, so a resource moved out from
    /// under a scope (or in beneath one) changes what a stored binding there
    /// gives its holder, though the role's scopes and grants read the same.
    /// A scope whose resource or instance path changed is
    /// `compatibility.scope`'s finding, so it is left out here.
    fn coverage(&mut self) {
        let old_scopes: Vec<_> = self
            .role
            .scopes
            .iter()
            .filter_map(|symbol| self.old.scope(symbol))
            .collect();
        let new_scopes: Vec<_> = self
            .now
            .scopes
            .iter()
            .filter_map(|symbol| self.new.scope(symbol))
            .collect();
        // Only what the model grants: a `by service` grant applies where the
        // service decides, whatever the scope covers (it is exempt from
        // reachability), and a grant moving to or from the service is the
        // grant comparison's finding.
        let model: BTreeSet<Granted> = self
            .now
            .effective
            .iter()
            .map(|g| (&g.resource, g.permission.as_str()))
            .collect();
        // What the model still grants, by resource in both models: the old
        // resource, the new one, and the values granted on it in each.
        let mut kept: BTreeMap<(&ShapeId, &ShapeId), Kept> = BTreeMap::new();
        for (resource, permission) in self
            .role
            .effective
            .iter()
            .map(|g| (&g.resource, g.permission.as_str()))
        {
            let Some(now) = land(self.old, self.new, resource, permission) else {
                continue;
            };
            if !model.contains(&now) {
                continue;
            }
            let entry = kept.entry((resource, now.0)).or_default();
            entry.0.push(value(self.old, resource, permission));
            entry.1.push(value(self.new, now.0, now.1));
        }
        for scope in old_scopes {
            let Some(now) = new_scopes.iter().find(|s| s.value == scope.value) else {
                continue;
            };
            if !same_target(&target(self.old, &scope.on), &target(self.new, &now.on))
                || !same_path(&instance(self.old, &scope.on), &instance(self.new, &now.on))
            {
                continue;
            }
            for ((was_on, is_on), (was_values, is_values)) in &kept {
                let reach = |service: &Service, on: &ScopeTarget, resource: &ShapeId| {
                    (
                        covers(service, on, Target::Instance(resource)),
                        covers(service, on, Target::Every(resource)),
                    )
                };
                let before = reach(self.old, &scope.on, was_on);
                let after = reach(self.new, &now.on, is_on);
                let symbol = &self.role.symbol;
                if before.0 && !after.0 || before.1 && !after.1 {
                    let message = format!(
                        "`{symbol}` bound at {:?} no longer covers `{}`, where it grants {}; its holders bound there are refused what they had",
                        scope.value,
                        was_on.name(),
                        quoted(was_values)
                    );
                    self.push("compatibility.role", true, message);
                }
                if !before.0 && after.0 || !before.1 && after.1 {
                    let message = format!(
                        "`{symbol}` bound at {:?} now covers `{}`, where it grants {}; its holders bound there gain it without a new binding",
                        now.value,
                        is_on.name(),
                        quoted(is_values)
                    );
                    self.push("compatibility.role-widened", false, message);
                }
            }
        }
    }

    /// Who may grant or revoke it, by the values of the roles `assignedBy`
    /// names. None named is whoever the binding operation admits, which
    /// includes every holder of any role, so naming some narrows it and
    /// naming none again widens it. Assignment is authority over bindings:
    /// an assigner that loses it can no longer grant or revoke what it
    /// could, and one that gains it is a security review. An assigner role
    /// that is gone altogether is `compatibility.role`'s finding at that
    /// role, not again here.
    fn assigners(&mut self) {
        let values = |service: &'a Service, role: &'a Role| -> Vec<&'a str> {
            role.assigned_by
                .iter()
                .filter_map(|symbol| service.role(symbol))
                .map(|assigner| assigner.value.as_str())
                .collect()
        };
        let (was, is) = (values(self.old, self.role), values(self.new, self.now));
        let symbol = &self.role.symbol;
        match (self.role.assigned_by.is_empty(), self.now.assigned_by.is_empty()) {
            (true, true) => {}
            (true, false) => self.push(
                "compatibility.role",
                true,
                format!(
                    "`{symbol}` may now be assigned only by holders of {}; others the binding operation admits can no longer grant or revoke it",
                    quoted(&is)
                ),
            ),
            (false, true) => self.push(
                "compatibility.role-widened",
                false,
                format!(
                    "`{symbol}` may now be assigned by whoever the binding operation admits, where only holders of {} could; they can grant or revoke it without a new binding",
                    quoted(&was)
                ),
            ),
            (false, false) => {
                let lost: Vec<&str> = was
                    .iter()
                    .copied()
                    .filter(|value| !is.contains(value))
                    .filter(|value| self.new.roles.iter().any(|r| r.value == *value))
                    .collect();
                let gained: Vec<&str> = is
                    .iter()
                    .copied()
                    .filter(|value| !was.contains(value))
                    .collect();
                if !lost.is_empty() {
                    self.push(
                        "compatibility.role",
                        true,
                        format!(
                            "`{symbol}` may no longer be assigned by holders of {}; they can no longer grant or revoke it",
                            quoted(&lost)
                        ),
                    );
                }
                if !gained.is_empty() {
                    self.push(
                        "compatibility.role-widened",
                        false,
                        format!(
                            "`{symbol}` may now be assigned by holders of {}; they can grant or revoke it without a new binding",
                            quoted(&gained)
                        ),
                    );
                }
            }
        }
    }

    /// What it grants, the model or the service deciding.
    fn grants(&mut self) {
        let model: BTreeSet<Granted> = self
            .now
            .effective
            .iter()
            .map(|g| (&g.resource, g.permission.as_str()))
            .collect();
        let service: BTreeSet<Granted> = self
            .now
            .by_service
            .iter()
            .map(|g| (&g.resource, g.permission.as_str()))
            .collect();
        // Where each old grant landed: kept, left to the service, or lost.
        let mut landed: BTreeSet<Granted> = BTreeSet::new();
        for (resource, permission, by_service) in self
            .role
            .effective
            .iter()
            .map(|g| (&g.resource, g.permission.as_str(), false))
            .chain(
                self.role
                    .by_service
                    .iter()
                    .map(|g| (&g.resource, g.permission.as_str(), true)),
            )
        {
            // A permission gone altogether: `compatibility.permissions`.
            let Some(now) = land(self.old, self.new, resource, permission) else {
                continue;
            };
            landed.insert(now);
            let what = describe(self.old, resource, permission);
            let symbol = &self.role.symbol;
            match (by_service, model.contains(&now), service.contains(&now)) {
                (false, true, _) | (true, false, true) => {}
                (false, false, true) => self.push(
                    "compatibility.role",
                    false,
                    format!(
                        "`{symbol}` now leaves {what} to the service (`by service`); its holders keep it only where the service allows"
                    ),
                ),
                (true, true, _) => self.push(
                    "compatibility.role-widened",
                    false,
                    format!(
                        "`{symbol}` now grants {what} outright, where the service decided (`by service`); its holders gain it without the service's check"
                    ),
                ),
                (false, false, false) => self.push(
                    "compatibility.role",
                    true,
                    format!(
                        "`{symbol}` no longer grants {what}; its holders are refused what they had"
                    ),
                ),
                (true, false, false) => self.push(
                    "compatibility.role",
                    true,
                    format!(
                        "`{symbol}` no longer grants {what} (`by service`); its holders are refused what they had"
                    ),
                ),
            }
        }
        for (granted, by_service) in model
            .iter()
            .map(|g| (*g, false))
            .chain(service.iter().map(|g| (*g, true)))
        {
            if landed.contains(&granted) {
                continue;
            }
            let (resource, permission) = granted;
            let what = describe(self.new, resource, permission);
            let star = if self.through_new_star(resource, permission, by_service) {
                ", a new permission its `*` grant covers"
            } else {
                ""
            };
            let symbol = &self.role.symbol;
            let message = if by_service {
                format!(
                    "`{symbol}` now grants {what}{star} where the service decides (`by service`); its holders may gain it without a new binding"
                )
            } else {
                format!(
                    "`{symbol}` now grants {what}{star}; its holders gain it without a new binding"
                )
            };
            self.push("compatibility.role-widened", false, message);
        }
    }

    /// Whether the new role grants `permission` because a `*` grant of it
    /// (or of a role it includes) covers it, and no permission of the
    /// baseline stored under any value lands on it: a permission added
    /// since, which widened the role without an edit to it.
    fn through_new_star(&self, resource: &ShapeId, permission: &str, by_service: bool) -> bool {
        let star = with_included(self.new, self.now)
            .iter()
            .flat_map(|role| &role.grants)
            .any(|grant| {
                &grant.resource == resource
                    && grant.by_service == by_service
                    && matches!(grant.permissions, GrantedSet::All { .. })
            });
        let Some(name) = self.new.permissions.get(resource).map(|r| &r.name) else {
            return false;
        };
        let released = self
            .old
            .permissions
            .iter()
            .filter(|(_, r)| &r.name == name)
            .any(|(id, r)| {
                r.permissions.iter().any(|p| {
                    land(self.old, self.new, id, &p.symbol) == Some((resource, permission))
                })
            });
        star && !released
    }
}

/// Where a grant of `old`'s `permission` on `resource` lands in `new`: the
/// primary that a value stored under it satisfies there, matched by
/// authorization name and value, an alias standing for its primary. None
/// when the resource or the value is gone.
fn land<'n>(
    old: &Service,
    new: &'n Service,
    resource: &ShapeId,
    permission: &str,
) -> Option<Granted<'n>> {
    let before = old.permissions.get(resource)?;
    let value = &before.permission(permission)?.value;
    let (id, after) = new
        .permissions
        .iter()
        .find(|(_, r)| r.name == before.name)?;
    let holder = after.by_value(value)?;
    Some((id, after.primary(holder).symbol.as_str()))
}

/// The stored value of `permission` on `resource`.
fn value<'s>(service: &'s Service, resource: &ShapeId, permission: &'s str) -> &'s str {
    service
        .permissions
        .get(resource)
        .and_then(|r| r.permission(permission))
        .map_or(permission, |p| p.value.as_str())
}

fn quoted(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `"graph:read" on `Graph``: the stored value and the resource.
fn describe(service: &Service, resource: &ShapeId, permission: &str) -> String {
    let value = service
        .permissions
        .get(resource)
        .and_then(|r| r.permission(permission))
        .map_or(permission, |p| p.value.as_str());
    format!("{value:?} on `{}`", resource.name())
}

enum Holders {
    /// Every kind of principal: `holders` absent, and no `principals` enum
    /// to list the kinds.
    Every,
    /// These kinds, wire value to the name the model writes.
    Only(BTreeMap<String, String>),
}

/// Who may hold `role`: its `holders`, or every value of the `principals`
/// enum when it names none. None when the role names holders the enum does
/// not have, which the compiler refuses.
fn holders(service: &Service, role: &Role) -> Option<Holders> {
    let values: Option<BTreeMap<String, String>> = service
        .role_settings
        .principals
        .as_ref()
        .and_then(|id| service.shapes.get(id))
        .and_then(|shape| match shape {
            Shape::Enum(e) => Some(
                e.values
                    .iter()
                    .map(|v| (v.name.clone(), v.value.clone()))
                    .collect(),
            ),
            Shape::IntEnum(e) => Some(
                e.values
                    .iter()
                    .map(|v| (v.name.clone(), v.value.to_string()))
                    .collect(),
            ),
            _ => None,
        });
    if role.holders.is_empty() {
        return Some(values.map_or(Holders::Every, |values| {
            Holders::Only(
                values
                    .into_iter()
                    .map(|(name, value)| (value, name))
                    .collect(),
            )
        }));
    }
    let values = values?;
    role.holders
        .iter()
        .map(|name| values.get(name).map(|value| (value.clone(), name.clone())))
        .collect::<Option<BTreeMap<_, _>>>()
        .map(Holders::Only)
}

fn names<'n>(names: impl Iterator<Item = &'n String>) -> String {
    names
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
