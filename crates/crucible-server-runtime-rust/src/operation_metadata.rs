use crate::operation::{AuthScheme, Input, Operation, Pagination, PaginationMember, Protocol};
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
    pub protocol: Protocol,
    pub auth_schemes: Option<&'static [AuthScheme]>,
    /// Prepared once at registration for protocol-independent bearer challenges.
    pub bearer_auth: bool,
    pub optional_auth: bool,
    /// Every one of these OAuth scopes is required; empty requires none.
    pub scopes: &'static [&'static str],
    /// The service default scope list, advertised on authentication challenges.
    pub service_scopes: &'static [&'static str],
    pub unsigned_payload: bool,
    pub has_input: bool,
    pub has_output: bool,
    pub input_events: Option<&'static crate::event_stream::EventStreamInfo>,
    pub output_events: Option<&'static crate::event_stream::EventStreamInfo>,
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
    /// Whether the model explicitly makes the operation public
    /// ([`Operation::PUBLIC`]), so there is nothing to authorize.
    pub public: bool,
}

impl OperationMetadata {
    pub(crate) fn of<O: Operation>() -> Self {
        Self {
            protocol: O::PROTOCOL,
            auth_schemes: O::AUTH_SCHEMES,
            bearer_auth: O::AUTH_SCHEMES.is_some_and(|schemes| {
                schemes
                    .iter()
                    .any(|scheme| matches!(scheme, AuthScheme::Bearer))
            }),
            optional_auth: O::OPTIONAL_AUTH,
            scopes: O::SCOPES,
            service_scopes: O::SERVICE_SCOPES,
            unsigned_payload: O::UNSIGNED_PAYLOAD,
            has_input: O::HAS_INPUT,
            has_output: O::HAS_OUTPUT,
            input_events: O::INPUT_EVENTS,
            output_events: O::OUTPUT_EVENTS,
            pagination: O::PAGINATION,
            idempotency_token: O::IDEMPOTENCY_TOKEN,
            idempotency_token_sent: O::IDEMPOTENCY_TOKEN.is_some(),
            replays_output: O::OUTPUT_FROM_JSON.is_some(),
            requirements: const { &O::REQUIREMENTS },
            checks: match O::CHECKS {
                Some(_) => Some(erased_checks::<O>),
                None => None,
            },
            public: O::PUBLIC,
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
