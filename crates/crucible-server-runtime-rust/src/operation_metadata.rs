use crate::operation::{AuthScheme, Input, Operation, Pagination, PaginationMember};
use crate::permissions::{Check, Requirements};

/// [`Operation::CHECKS`] behind the type-erased input: `false` when the
/// input is not the operation's.
pub type ErasedChecks = fn(&dyn Input, &mut Vec<Check>) -> bool;

/// Effective wire and authentication settings retained after type-erased registration.
/// These describe the model; middleware remains responsible for enforcing authentication.
///
/// Non-exhaustive: the runtime builds it at registration, and new model facts
/// join it as fields. Read the fields; a pattern that names them needs `..`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct OperationMetadata {
    pub auth_schemes: Option<&'static [AuthScheme]>,
    /// Prepared once at registration for protocol-independent bearer challenges.
    pub bearer_auth: bool,
    pub has_input: bool,
    pub has_output: bool,
    /// Where a `@paginated` operation's input carries its token and page size.
    /// At an older API version the input is the current version's, so these
    /// are the current members.
    pub pagination: Option<Pagination>,
    /// Where the decoded input carries the operation's idempotency token. At
    /// an older API version the input is the current version's, so this is
    /// the current member.
    pub idempotency_token: Option<PaginationMember>,
    /// Whether a request at this entry's API version can send the token:
    /// `false` for an older version whose model had none, whose requests
    /// therefore run unclaimed rather than refused for a member their
    /// clients cannot send.
    pub idempotency_token_sent: bool,
    /// Whether a stored output can answer a retry
    /// ([`DecodedJob::replay_output`](crate::DecodedJob::replay_output)).
    pub replays_output: bool,
    /// What the operation requires of its caller ([`Operation::REQUIREMENTS`]).
    /// At an older API version, the current version's: requirements are
    /// written against the current model, whose input interceptors see.
    pub requirements: &'static Requirements,
    /// Evaluates the requirements' bindings and conditions against the
    /// decoded input ([`Operation::CHECKS`]).
    pub checks: Option<ErasedChecks>,
    /// Whether the model explicitly makes the operation anonymous
    /// ([`Operation::ANONYMOUS`]): it takes no credential, so there is
    /// nothing to authorize.
    pub anonymous: bool,
    /// The former name of [`OperationMetadata::anonymous`], always equal to it.
    #[deprecated(since = "0.10.0", note = "use `anonymous`")]
    pub public: bool,
}

impl OperationMetadata {
    /// Whether every request the operation serves has been authenticated:
    /// it names an authentication scheme. An operation with no scheme (an
    /// `anonymous` one) or without
    /// authentication metadata (bindings older than it) admits anonymous
    /// callers.
    pub fn requires_authentication(&self) -> bool {
        self.auth_schemes.is_some_and(|schemes| !schemes.is_empty())
    }

    // Fills the deprecated `public` beside `anonymous`, and reads the
    // deprecated `Operation::PUBLIC` a hand-written marker may still set.
    #[allow(deprecated)]
    pub(crate) fn of<O: Operation>() -> Self {
        let anonymous = O::ANONYMOUS || O::PUBLIC;
        Self {
            auth_schemes: O::AUTH_SCHEMES,
            bearer_auth: O::AUTH_SCHEMES.is_some_and(|schemes| {
                schemes
                    .iter()
                    .any(|scheme| matches!(scheme, AuthScheme::Bearer))
            }),
            has_input: O::HAS_INPUT,
            has_output: O::HAS_OUTPUT,
            pagination: O::PAGINATION,
            idempotency_token: O::IDEMPOTENCY_TOKEN,
            idempotency_token_sent: O::IDEMPOTENCY_TOKEN.is_some(),
            replays_output: O::OUTPUT_FROM_JSON.is_some(),
            requirements: const { &O::REQUIREMENTS },
            checks: match O::CHECKS {
                Some(_) => Some(erased_checks::<O>),
                None => None,
            },
            anonymous,
            public: anonymous,
        }
    }
}

fn erased_checks<O: Operation>(input: &dyn Input, out: &mut Vec<Check>) -> bool {
    match (O::CHECKS, input.as_any().downcast_ref::<O::Input>()) {
        (Some(checks), Some(input)) => {
            checks(input, out);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crucible_wire::__generated::operation_info;

    use super::OperationMetadata;
    use crate::operation::{Operation, OperationInfo, Unit};

    static INFO: OperationInfo = operation_info(
        "t#Ping",
        "Ping",
        None,
        "GET",
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

    struct Anonymous;
    impl Operation for Anonymous {
        const INFO: &'static OperationInfo = &INFO;
        const ANONYMOUS: bool = true;
        type Input = Unit;
        type Output = Unit;
    }

    /// A hand-written marker from before `ANONYMOUS` sets the deprecated
    /// `PUBLIC`.
    struct Public;
    #[allow(deprecated)]
    impl Operation for Public {
        const INFO: &'static OperationInfo = &INFO;
        const PUBLIC: bool = true;
        type Input = Unit;
        type Output = Unit;
    }

    struct Authenticated;
    impl Operation for Authenticated {
        const INFO: &'static OperationInfo = &INFO;
        type Input = Unit;
        type Output = Unit;
    }

    #[test]
    #[allow(deprecated)]
    fn anonymous_and_its_deprecated_name_agree() {
        for (metadata, anonymous) in [
            (OperationMetadata::of::<Anonymous>(), true),
            (OperationMetadata::of::<Public>(), true),
            (OperationMetadata::of::<Authenticated>(), false),
        ] {
            assert_eq!(metadata.anonymous, anonymous);
            assert_eq!(metadata.public, anonymous);
        }
        // The deprecated constant defaults to the new one.
        const { assert!(<Anonymous as Operation>::PUBLIC) };
    }
}
