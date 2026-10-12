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
}

impl<'a> Prepare<'a> {
    pub(crate) fn new(registry: &'a crate::Operations) -> Self {
        Self {
            service: registry.service(),
            registry,
            scope: None,
            outer: None,
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
        }
    }
}
