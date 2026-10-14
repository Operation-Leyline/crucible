use std::sync::Arc;

use crate::operation::OperationInfo;
use crate::{Context, Error, OperationMetadata, Prepare, Result};

type Name = Arc<dyn Fn(&Context) -> Option<String> + Send + Sync>;

/// Who is asking, as `Idempotency` and `PageTokens` bind a token to them:
/// the service's closure naming the caller, and whether a request it names
/// no one for shares one anonymous binding.
///
/// Sharing is an explicit choice (`allow_anonymous`): a closure that misses
/// a principal (a new authentication scheme, API keys beside JWTs) would
/// otherwise let one caller replay another's stored response or open
/// another's listing. Without it a nameless request is refused, and an
/// operation that requires authentication refuses one even with it: its
/// caller was authenticated, so `None` means the closure did not recognise
/// them.
#[derive(Clone)]
pub(crate) struct CallerBinding {
    name: Name,
    anonymous: bool,
}

impl CallerBinding {
    pub(crate) fn new(name: impl Fn(&Context) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            name: Arc::new(name),
            anonymous: false,
        }
    }

    /// Lets every request the closure names no one for share one anonymous
    /// binding, on operations that admit anonymous callers.
    pub(crate) fn allow_anonymous(&mut self) {
        self.anonymous = true;
    }

    /// This request's binding: the caller's name, or `None` for the shared
    /// anonymous binding. A refusal is the service's internal error: the
    /// binding is the service's wiring, not the request's fault.
    pub(crate) fn bind(
        &self,
        extension: &str,
        context: &Context,
        operation: &OperationInfo,
        metadata: &OperationMetadata,
    ) -> Result<Option<String>> {
        match (self.name)(context) {
            Some(name) => Ok(Some(name)),
            None if metadata.requires_authentication() => Err(Error::internal(format!(
                "{extension}: the caller binding named no caller for {}, which requires authentication; \
                 it must name every authenticated caller (does it read this request's credential?)",
                operation.name
            ))),
            None if self.anonymous => Ok(None),
            None => Err(Error::internal(format!(
                "{extension}: the caller binding named no caller for {}; \
                 `{extension}::allow_anonymous` lets anonymous callers share one binding",
                operation.name
            ))),
        }
    }

    /// Refuses the build when an entry `covers` selects in the extension's
    /// scope admits anonymous callers and sharing was not chosen: such a
    /// request would be refused at run time. Asked per served entry, since
    /// an older API version may authenticate differently.
    pub(crate) fn prepare(
        &self,
        extension: &str,
        prepare: &Prepare<'_>,
        covers: impl Fn(&OperationMetadata) -> bool,
    ) -> std::result::Result<(), String> {
        if self.anonymous {
            return Ok(());
        }
        let open: Vec<String> = prepare
            .served()
            .filter(|served| prepare.applies(served.info))
            .filter(|served| covers(served.metadata))
            .filter(|served| !served.metadata.requires_authentication())
            .map(|served| served.label())
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        Err(format!(
            "{extension} binds each token to its caller, and anonymous callers reach {} \
             (no authentication, or optional): call `{extension}::allow_anonymous` to let \
             requests that name no caller share one binding, or leave them out of its scope",
            open.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use crucible_wire::operation::AuthScheme;

    use crate::OperationMetadata;
    use crate::operation::Operation;

    fn metadata<O: Operation>() -> OperationMetadata {
        OperationMetadata::of::<O>()
    }

    macro_rules! operation {
        ($name:ident, $schemes:expr, $optional:expr) => {
            struct $name;
            impl Operation for $name {
                const INFO: &'static crate::OperationInfo = &INFO;
                const AUTH_SCHEMES: Option<&'static [AuthScheme]> = $schemes;
                const OPTIONAL_AUTH: bool = $optional;
                type Input = crate::operation::Unit;
                type Output = crate::operation::Unit;
            }
        };
    }

    static INFO: crate::OperationInfo = crucible_wire::__generated::operation_info(
        "t#Op",
        "Op",
        None,
        "POST",
        &[],
        &[],
        200,
        &[],
        &[],
        &[],
        false,
        false,
        false,
        None,
        &[],
    );

    operation!(Bearer, Some(&[AuthScheme::Bearer]), false);
    operation!(Optional, Some(&[AuthScheme::Bearer]), true);
    operation!(Public, Some(&[]), false);
    operation!(Unknown, None, false);

    #[test]
    fn only_a_scheme_without_optional_auth_requires_authentication() {
        assert!(metadata::<Bearer>().requires_authentication());
        assert!(!metadata::<Optional>().requires_authentication());
        assert!(!metadata::<Public>().requires_authentication());
        assert!(!metadata::<Unknown>().requires_authentication());
    }
}
