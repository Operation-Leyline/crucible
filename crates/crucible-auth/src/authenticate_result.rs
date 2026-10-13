use crate::Principal;
use crucible_server_runtime_rust::Error;

/// A store/provider outage fails closed as a server failure, never as an invalid credential.
#[derive(Debug)]
// `Authenticated` is the common result and moves straight into the Context;
// boxing it would cost an allocation per request and break every authenticator.
#[allow(clippy::large_enum_variant)]
pub enum AuthenticateResult {
    Refused,
    Unavailable(Error),
    Authenticated(Principal),
}
