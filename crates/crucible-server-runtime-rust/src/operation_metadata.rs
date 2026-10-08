use crate::operation::{AuthScheme, Operation, Protocol};

/// Effective wire and authentication settings retained after type-erased registration.
/// These describe the model; middleware remains responsible for enforcing authentication.
#[derive(Debug, Clone, Copy)]
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
        }
    }
}
