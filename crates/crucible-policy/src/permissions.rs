//! Permission compatibility (docs/permissions.md §5): a permission's value
//! and its resource's authorization name are stored data, in every grant and
//! audit record, so changing either breaks them; so does changing which
//! permission a stored value satisfies. Adding a requirement is how holes
//! get closed, so it is reported, never refused.

use std::collections::BTreeMap;

use crucible_smithy::ir::{Requirement, Requirements, ResourcePermissions, Tier};

use crate::Severity;
use crate::checker::Checker;

impl Checker<'_> {
    /// Stored grants: permission values and authorization names.
    pub(crate) fn permissions_wire(&mut self) {
        for (id, old) in &self.old.permissions {
            // A renamed shape that keeps its authorization name keeps its
            // grants: grants name the authorization name, not the shape.
            let new = self.new.permissions.get(id).or_else(|| {
                self.new
                    .permissions
                    .iter()
                    .find(|(other, r)| {
                        r.name == old.name && !self.old.permissions.contains_key(*other)
                    })
                    .map(|(_, r)| r)
            });
            let Some(new) = new else {
                for permission in &old.permissions {
                    self.push(
                        "compatibility.permissions",
                        &id.with_member(&permission.symbol),
                        format!(
                            "`{}` no longer declares permissions, so grants stored under {:?} on {:?} match nothing; a renamed resource keeps them with `@authorizationName(\"{}\")`",
                            id.name(),
                            permission.value,
                            old.name,
                            old.name
                        ),
                    );
                }
                continue;
            };
            if old.name != new.name {
                self.push(
                    "compatibility.permissions",
                    id,
                    format!(
                        "the authorization name changed from {:?} to {:?}; every stored grant and audit record names {:?}: keep it with `@authorizationName(\"{}\")`",
                        old.name, new.name, old.name, old.name
                    ),
                );
            }
            // A brand-new primary that took over the values of released ones,
            // by the old primaries it took over.
            let mut takeovers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for permission in &old.permissions {
                let site = id.with_member(&permission.symbol);
                // The symbol is kept and its value changed, even when the old
                // value now belongs to another symbol (a swap). A deprecated
                // alias that keeps the old value for the same primary keeps
                // every stored grant.
                if let Some(now) = new.permission(&permission.symbol)
                    && now.value != permission.value
                    && let Some(holder) = new.by_value(&permission.value)
                    && primary(new, &holder.symbol) != primary(old, &permission.symbol)
                {
                    self.push(
                        "compatibility.permissions",
                        &site,
                        format!(
                            "`{}` changed its value from {:?} to {:?}; grants stored under the old value no longer match it: keep the value, and give the new one to a new primary with this one as its deprecated alias (`@aliasOf`)",
                            permission.symbol, permission.value, now.value
                        ),
                    );
                }
                if let Some(holder) = new.by_value(&permission.value) {
                    // Which permission a grant stored under the value satisfies.
                    let before = primary(old, &permission.symbol);
                    let after = primary(new, &holder.symbol);
                    if before == after {
                        continue;
                    }
                    if old.permission(after).is_some() {
                        self.push(
                            "compatibility.permissions",
                            &site,
                            format!(
                                "grants stored under {:?} satisfied `{before}` and now satisfy `{after}`, another released permission: every holder of {:?} changes access without a new grant; keep the value on `{before}`",
                                permission.value, permission.value
                            ),
                        );
                    } else {
                        let taken = takeovers.entry(after).or_default();
                        if !taken.contains(&before) {
                            taken.push(before);
                        }
                    }
                    continue;
                }
                let message = match (&permission.alias_of, new.permission(&permission.symbol)) {
                    (Some(_), _) => format!(
                        "the alias {:?} was removed; grants stored under it no longer satisfy any check",
                        permission.value
                    ),
                    (None, Some(renamed)) if renamed.alias_of.is_none() => format!(
                        "`{}` changed its value from {:?} to {:?}; grants stored under the old value no longer match: keep it as a deprecated alias (`@aliasOf({})`)",
                        permission.symbol, permission.value, renamed.value, permission.symbol
                    ),
                    _ => format!(
                        "the permission {:?} (`{}`) was removed; grants stored under it no longer match: keep it, or keep its value as a deprecated alias of its replacement",
                        permission.value, permission.symbol
                    ),
                };
                self.push("compatibility.permissions", &site, message);
            }
            for (primary, before) in takeovers {
                if before.len() > 1 {
                    self.push(
                        "compatibility.permissions",
                        &id.with_member(primary),
                        format!(
                            "the new permission `{primary}` takes over the values of {}: grants of one now satisfy requirements of the others; a renamed permission replaces one",
                            before
                                .iter()
                                .map(|p| format!("`{p}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
            }
        }
        self.authorization();
    }

    /// Generated SDK constants.
    pub(crate) fn permissions_sdk(&mut self) {
        for (id, old) in &self.old.permissions {
            let new = self.new.permissions.get(id);
            for permission in old.permissions.iter().filter(|p| p.tier != Tier::Internal) {
                let site = id.with_member(&permission.symbol);
                match new.and_then(|n| n.permission(&permission.symbol)) {
                    None => self.push(
                        "compatibility.sdk",
                        &site,
                        format!(
                            "the permission constant `{}` of `{}` was removed from client SDKs",
                            permission.symbol,
                            id.name()
                        ),
                    ),
                    Some(now) if now.tier == Tier::Internal => self.push(
                        "compatibility.sdk",
                        &site,
                        format!(
                            "`{}` became internal, so client SDKs lose its constant",
                            permission.symbol
                        ),
                    ),
                    Some(_) => {}
                }
            }
        }
    }

    /// Requirements added to a released operation: reported as warnings.
    fn authorization(&mut self) {
        if self.severity == Severity::Off {
            return;
        }
        for old_op in &self.old.operations {
            let Some(new_op) = self.new.operations.iter().find(|op| op.id == old_op.id) else {
                continue;
            };
            let added: Vec<&Requirement> = match (&old_op.requirements, &new_op.requirements) {
                // A projected IR is never compared; `Partial` reads as the
                // list it is.
                (
                    Requirements::List(old) | Requirements::Partial(old),
                    Requirements::List(new) | Requirements::Partial(new),
                ) => new
                    .iter()
                    .filter(|clause| !old.iter().any(|before| self.same_clause(before, clause)))
                    .collect(),
                (_, Requirements::List(new) | Requirements::Partial(new)) => new.iter().collect(),
                _ => Vec::new(),
            };
            if added.is_empty() {
                continue;
            }
            let was = match &old_op.requirements {
                Requirements::Undeclared => " (it declared none)",
                Requirements::None => " (it was `requires none`)",
                Requirements::Pending { .. } => " (it was `requires pending`)",
                Requirements::List(_) | Requirements::Partial(_) | Requirements::Internal => "",
            };
            let clauses: Vec<String> = added
                .iter()
                .map(|clause| describe(clause, self.new.permissions.get(&clause.resource)))
                .collect();
            self.report.push(
                "compatibility.authorization",
                &old_op.id,
                Severity::Warning,
                format!(
                    "an authorization change: `{}` now requires {}{was}; callers without it are refused",
                    old_op.name(),
                    clauses.join(", ")
                ),
            );
        }
    }
}

impl Checker<'_> {
    /// The same requirement: what a check compares is the stored pair
    /// (authorization name, value), not the symbols naming it. Naming the
    /// primary the old value became an alias of is no change: grants under
    /// the alias satisfy it.
    fn same_clause(&self, old: &Requirement, new: &Requirement) -> bool {
        if old.mode != new.mode || old.bindings != new.bindings || old.when != new.when {
            return false;
        }
        let (Some(before), Some(after)) = (
            self.old.permissions.get(&old.resource),
            self.new.permissions.get(&new.resource),
        ) else {
            return old.resource == new.resource && old.permission == new.permission;
        };
        let (Some(was), Some(now)) = (
            before.permission(&old.permission),
            after.permission(&new.permission),
        ) else {
            return false;
        };
        before.name == after.name
            && (was.value == now.value
                || after
                    .by_value(&was.value)
                    .is_some_and(|p| p.alias_of.as_deref() == Some(now.symbol.as_str())))
    }
}

/// The primary a permission's grants satisfy: itself, or the one it is an
/// alias of.
fn primary<'r>(resource: &'r ResourcePermissions, symbol: &'r str) -> &'r str {
    resource
        .permission(symbol)
        .and_then(|p| p.alias_of.as_deref())
        .unwrap_or(symbol)
}

fn describe(clause: &Requirement, resource: Option<&ResourcePermissions>) -> String {
    let value = resource
        .and_then(|r| r.permission(&clause.permission))
        .map_or(clause.permission.as_str(), |p| p.value.as_str());
    format!("{value:?} on `{}`", clause.resource.name())
}
