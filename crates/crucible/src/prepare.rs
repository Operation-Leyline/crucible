use crate::operation::{OperationInfo, ServiceInfo};

/// The service as a extension sees it at `build`: the whole of it, and the
/// operations the extension's scope includes.
#[derive(Clone, Copy)]
pub struct Prepare<'a> {
    service: &'static ServiceInfo,
    scope: Option<&'a (dyn Fn(&OperationInfo) -> bool + 'a)>,
    /// The scope this one narrows: an operation must pass both.
    outer: Option<&'a Prepare<'a>>,
}

impl<'a> Prepare<'a> {
    pub(crate) fn new(service: &'static ServiceInfo) -> Self {
        Self {
            service,
            scope: None,
            outer: None,
        }
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    /// Whether the extension runs for `operation`.
    pub fn applies(&self, operation: &OperationInfo) -> bool {
        self.outer.is_none_or(|outer| outer.applies(operation))
            && self.scope.is_none_or(|scope| scope(operation))
    }

    /// The operations the extension runs for.
    pub fn operations(&self) -> impl Iterator<Item = &'static OperationInfo> + '_ {
        self.service
            .operations
            .iter()
            .copied()
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
            scope: Some(scope),
            outer: Some(self),
        }
    }
}
