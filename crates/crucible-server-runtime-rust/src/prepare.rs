use std::cell::RefCell;

use crate::operation::{OperationInfo, ServiceInfo};
use crate::versioning::ApiVersion;

/// The service as a extension sees it at `build`: the whole of it, and the
/// operations the extension's scope includes.
#[derive(Clone, Copy)]
pub struct Prepare<'a> {
    service: &'static ServiceInfo,
    registry: &'a crate::Operations,
    scope: Option<&'a (dyn Fn(&OperationInfo) -> bool + 'a)>,
    /// The scope this one narrows: an operation must pass both.
    outer: Option<&'a Prepare<'a>>,
    /// What the runtime's own extensions recorded while being prepared.
    builtins: &'a RefCell<Builtins>,
}

/// What the runtime's own extensions record at `build`, for the rules
/// between them and the checks `build` makes once all are prepared.
#[derive(Default)]
pub(crate) struct Builtins {
    /// Prepared so far, in registration order, for the ordering rules
    /// between them (`Permissions` before `PageTokens` and `Idempotency`).
    /// Never a service's extension: those are not named.
    pub(crate) order: Vec<&'static str>,
    /// The served entries a `Permissions` covers, by [`Served::key`]: one
    /// declaring requirements outside all of them would run unchecked.
    pub(crate) authorized: Vec<(Option<ApiVersion>, &'static str)>,
}

/// One operation as a request at one API version reaches it: the table's
/// version (`None` for the current one), the description a scope's
/// predicate sees for that request, and its effective metadata.
pub(crate) struct Served<'r> {
    pub(crate) version: Option<ApiVersion>,
    pub(crate) info: &'static OperationInfo,
    pub(crate) metadata: &'r crate::OperationMetadata,
}

impl Served<'_> {
    /// What identifies the entry: an operation name is unique in its table.
    pub(crate) fn key(&self) -> (Option<ApiVersion>, &'static str) {
        (self.version, self.info.name)
    }

    /// The operation's name, with its API version when an older one serves
    /// it, for a build error.
    pub(crate) fn label(&self) -> String {
        match self.version {
            Some(version) => format!("{} (API version {version})", self.info.name),
            None => self.info.name.to_owned(),
        }
    }
}

impl<'a> Prepare<'a> {
    pub(crate) fn new(registry: &'a crate::Operations, builtins: &'a RefCell<Builtins>) -> Self {
        Self {
            service: registry.service(),
            registry,
            scope: None,
            outer: None,
            builtins,
        }
    }

    /// Records that the runtime's own extension `name` is registered here,
    /// answering whether `before` was registered ahead of it.
    #[cfg_attr(
        not(any(
            feature = "idempotency",
            feature = "page-tokens",
            feature = "permissions"
        )),
        allow(dead_code)
    )]
    pub(crate) fn builtin(&self, name: &'static str, before: &str) -> bool {
        let earlier = self.registered(before);
        self.builtins.borrow_mut().order.push(name);
        earlier
    }

    /// Whether the runtime's own extension `name` was registered ahead of
    /// the one being prepared.
    #[cfg_attr(not(feature = "permissions"), allow(dead_code))]
    pub(crate) fn registered(&self, name: &str) -> bool {
        self.builtins.borrow().order.contains(&name)
    }

    /// Records that `Permissions` covers every served entry in this scope,
    /// so `build` knows their declared requirements are enforced. Asked per
    /// entry, not per operation: a request at an older API version carries
    /// that version's description, and the scope decides on it.
    #[cfg_attr(not(feature = "permissions"), allow(dead_code))]
    pub(crate) fn authorize_scope(&self) {
        let covered: Vec<_> = self
            .served()
            .filter(|served| self.applies(served.info))
            .map(|served| served.key())
            .collect();
        let mut builtins = self.builtins.borrow_mut();
        for key in covered {
            if !builtins.authorized.contains(&key) {
                builtins.authorized.push(key);
            }
        }
    }

    /// Every served entry, whatever this scope: the current version's
    /// table, then each older version's, oldest first.
    pub(crate) fn served(&self) -> impl Iterator<Item = Served<'a>> + 'a {
        let current = self.registry.entries.values().map(|entry| Served {
            version: None,
            info: entry.info,
            metadata: &entry.metadata,
        });
        let older = self.registry.versions.older.iter().flat_map(|older| {
            older.operations.entries.values().map(move |entry| Served {
                version: Some(older.version),
                info: entry.info,
                metadata: &entry.metadata,
            })
        });
        current.chain(older)
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    /// Effective metadata for an operation in this extension's scope.
    pub fn metadata(&self, operation: &OperationInfo) -> Option<&crate::OperationMetadata> {
        self.applies(operation)
            .then(|| self.registry.served_metadata(operation))
            .flatten()
    }

    /// Whether the extension runs for `operation`.
    pub fn applies(&self, operation: &OperationInfo) -> bool {
        self.outer.is_none_or(|outer| outer.applies(operation))
            && self.scope.is_none_or(|scope| scope(operation))
    }

    /// The operations the extension runs for, each id once: the current
    /// version's, then those only an older API version serves (an operation
    /// a later release removed), newest version first. A request for one
    /// passes the same extensions, so whatever an extension prepares per
    /// operation (an authentication plan) must cover it.
    ///
    /// The scope is asked of each version's own description, as a request
    /// at that version asks it, before an id is taken: an operation whose
    /// current description is outside the scope but whose older one is
    /// inside (a scope on a member a later release renamed) is listed by
    /// the older description, so the requests the extension runs for at
    /// that version find what it prepared.
    pub fn operations(&self) -> impl Iterator<Item = &'static OperationInfo> + '_ {
        let mut seen: Vec<&'static str> = Vec::new();
        let older = self
            .registry
            .versions
            .older
            .iter()
            .rev()
            .flat_map(|older| older.operations.service.operations.iter().copied());
        self.service
            .operations
            .iter()
            .copied()
            .chain(older)
            .filter(|op| self.applies(op))
            .filter(move |op| {
                let first = !seen.contains(&op.id);
                seen.push(op.id);
                first
            })
    }

    /// The same service, narrowed to the operations `scope` accepts too: for
    /// a extension that wraps another and runs it for fewer operations. It
    /// only ever narrows: an operation outside this scope stays outside.
    pub fn within<'b>(&'b self, scope: &'b (dyn Fn(&OperationInfo) -> bool + 'b)) -> Prepare<'b>
    where
        'a: 'b,
    {
        Prepare {
            service: self.service,
            registry: self.registry,
            scope: Some(scope),
            outer: Some(self),
            builtins: self.builtins,
        }
    }
}
