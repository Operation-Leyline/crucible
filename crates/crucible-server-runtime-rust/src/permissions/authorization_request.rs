use super::OperationFacts;
use crate::Context;

/// The request an Authorizer decides for: its context (where the
/// authenticated principal is, as a typed attribute the service reads) and
/// the operation's facts.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct AuthorizationRequest<'a> {
    pub context: &'a Context,
    pub operation: OperationFacts,
}

impl<'a> AuthorizationRequest<'a> {
    pub(crate) fn new(context: &'a Context, operation: OperationFacts) -> Self {
        Self { context, operation }
    }
}
