use super::ScopeKind;
use crate::permissions::{ResourceInfo, ResourceRef, Target};

/// One scope of a [`RoleCatalogue`](super::RoleCatalogue): a place a
/// binding is made at, and what a binding there covers. Generated,
/// `'static`; the coverage lists are the IR's coverage relation tabulated
/// over the resources a check can name, so deciding coverage per request is
/// a table read and an identifier-prefix comparison.
#[derive(Debug, PartialEq, Eq)]
pub struct ScopeInfo {
    /// What the model calls it: `allGraphs`.
    pub symbol: &'static str,
    /// Its identity, what bindings store: `allGraphs`.
    pub value: &'static str,
    /// The model's documentation for it, empty when it has none.
    pub description: &'static str,
    pub internal: bool,
    pub kind: ScopeKind,
    /// The resource it is on, by shape name (`Graph`); empty `on service`.
    pub resource: &'static str,
    /// The identifiers an instance carries, root to leaf.
    pub identifiers: &'static [&'static str],
    /// The resources one instance of which a binding here covers, when the
    /// instance's identifiers prefix the target's.
    pub instances: &'static [&'static ResourceInfo],
    /// The resources every instance of which under one parent (`R(*)`) a
    /// binding here covers, likewise.
    pub every: &'static [&'static ResourceInfo],
    /// The scopes (indexes into
    /// [`RoleCatalogue::scopes`](super::RoleCatalogue::scopes)) an instance
    /// of which a binding here covers, likewise: where an assigner holding
    /// its role here may write a binding (docs/roles.md §4.5).
    pub covers_scopes: &'static [u16],
}

impl ScopeInfo {
    /// Whether a binding at the instance of this scope that `identifiers`
    /// names covers `target`: the target's resource is one this scope
    /// covers (one instance, or every instance under a parent when the
    /// target's leaf is `*`), and the instance's identifiers are the
    /// target's first ones. A count of identifiers that is not the scope's,
    /// or a target that does not name every identifier of its resource,
    /// covers nothing.
    pub fn covers(&self, identifiers: &[String], target: &ResourceRef) -> bool {
        if identifiers.len() != self.identifiers.len() || !target.is_complete() {
            return false;
        }
        let ids = target.identifiers();
        let resources = match ids.last() {
            Some(Target::All) => self.every,
            _ => self.instances,
        };
        let resource = target.resource();
        if !resources
            .iter()
            .any(|r| std::ptr::eq(*r, resource) || r.name == resource.name)
        {
            return false;
        }
        ids.len() >= identifiers.len()
            && ids
                .iter()
                .zip(identifiers)
                .all(|(target, id)| target.as_one() == Some(id.as_str()))
    }
}
