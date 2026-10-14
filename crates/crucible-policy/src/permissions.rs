//! Permission compatibility (docs/permissions.md §5): a permission's value
//! and its resource's authorization name are stored data, in every grant and
//! audit record, so changing either breaks them; adding a requirement is how
//! holes get closed, so it is reported, never refused.

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
            for permission in &old.permissions {
                if new.by_value(&permission.value).is_some() {
                    continue;
                }
                let site = id.with_member(&permission.symbol);
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
                (Requirements::List(old), Requirements::List(new)) => new
                    .iter()
                    .filter(|clause| !old.iter().any(|before| self.same_clause(before, clause)))
                    .collect(),
                (_, Requirements::List(new)) => new.iter().collect(),
                _ => Vec::new(),
            };
            if added.is_empty() {
                continue;
            }
            let was = match &old_op.requirements {
                Requirements::Undeclared => " (it declared none)",
                Requirements::None => " (it was `requires none`)",
                Requirements::Pending { .. } => " (it was `requires pending`)",
                Requirements::List(_) => "",
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
    /// The same requirement, where naming the primary a requirement's old
    /// permission became an alias of is no change: the alias's grants
    /// satisfy it.
    fn same_clause(&self, old: &Requirement, new: &Requirement) -> bool {
        let renamed = || {
            self.new
                .permissions
                .get(&new.resource)
                .and_then(|r| r.permission(&old.permission))
                .is_some_and(|p| p.alias_of.as_deref() == Some(new.permission.as_str()))
        };
        // A resource renamed under its old authorization name is the same one.
        let resource = old.resource == new.resource
            || self
                .old
                .permissions
                .get(&old.resource)
                .zip(self.new.permissions.get(&new.resource))
                .is_some_and(|(a, b)| a.name == b.name);
        old.mode == new.mode
            && resource
            && old.bindings == new.bindings
            && old.when == new.when
            && (old.permission == new.permission || renamed())
    }
}

fn describe(clause: &Requirement, resource: Option<&ResourcePermissions>) -> String {
    let value = resource
        .and_then(|r| r.permission(&clause.permission))
        .map_or(clause.permission.as_str(), |p| p.value.as_str());
    format!("{value:?} on `{}`", clause.resource.name())
}
