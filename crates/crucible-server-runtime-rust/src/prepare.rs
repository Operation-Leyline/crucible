use std::cell::RefCell;

use crate::operation::{OperationInfo, ServiceInfo};

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
    /// The operations a `Permissions` covers, by id: a served operation
    /// declaring requirements outside all of them would run unchecked.
    pub(crate) authorized: Vec<&'static str>,
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

    /// Records that `Permissions` covers `operation`, so `build` knows its
    /// declared requirements are enforced.
    #[cfg_attr(not(feature = "permissions"), allow(dead_code))]
    pub(crate) fn authorizes(&self, operation: &'static OperationInfo) {
        let mut builtins = self.builtins.borrow_mut();
        if !builtins.authorized.contains(&operation.id) {
            builtins.authorized.push(operation.id);
        }
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

    /// The operations the extension runs for: the current version's, then
    /// those only an older API version serves (an operation a later release
    /// removed), newest version first. A request for one passes the same
    /// extensions, so whatever an extension prepares per operation (an
    /// authentication plan) must cover it.
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
            .filter(move |op| {
                let first = !seen.contains(&op.id);
                seen.push(op.id);
                first
            })
            .filter(|op| self.applies(op))
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
