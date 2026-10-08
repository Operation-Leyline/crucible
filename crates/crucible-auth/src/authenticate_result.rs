use crate::Principal;
use crucible_server_runtime_rust::Error;

/// A store/provider outage fails closed as a server failure, never as an invalid credential.
#[derive(Debug)]
pub enum AuthenticateResult {
    Refused,
    Unavailable(Error),
    Authenticated(Principal),
}
