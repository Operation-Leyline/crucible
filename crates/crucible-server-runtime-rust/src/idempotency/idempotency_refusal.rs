use crate::validate::Rejection;
use crate::{Error, FrameworkError};

/// Why [`Idempotency`](super::Idempotency) refused a request, for a
/// service that answers it with errors of its own
/// ([`Idempotency::refusal_factory`](super::Idempotency::refusal_factory)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdempotencyRefusal {
    /// The request carries no token, or an empty one. `member` is the token
    /// member as the model names it.
    Missing { member: &'static str },
    /// The token is longer than 256 characters, or has a character outside
    /// visible ASCII.
    Invalid { member: &'static str },
    /// This caller used the token on this operation before, for a request
    /// with other input.
    Mismatch,
    /// The token's first request is still running, so there is nothing to
    /// replay yet.
    InFlight,
    /// This request's own claim lapsed before it reached its point of no
    /// return ([`commit`](super::commit)): it ran longer than the lease, and
    /// another attempt may have taken the token. It must not take the
    /// irreversible step.
    Lapsed,
    /// The token's first request succeeded, but its response cannot be
    /// replayed: the operation's response changed since (a deploy changed
    /// the API version or the output's shape).
    Unreplayable,
}

impl IdempotencyRefusal {
    /// What the caller is told, and what they can do about it.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Missing { .. } => {
                "an idempotency token is required: send a unique value for each intended operation, and the same value when retrying it"
            }
            Self::Invalid { .. } => {
                "an idempotency token must be at most 256 visible ASCII characters"
            }
            Self::Mismatch => {
                "the idempotency token was already used for a different request; send a new token for a new request"
            }
            Self::InFlight => {
                "a request with this idempotency token is still in progress, so its response cannot be replayed yet; retry after it finishes"
            }
            Self::Lapsed => {
                "this request took longer than its idempotency claim lasts, so it stopped before changing anything; retry with the same token"
            }
            Self::Unreplayable => {
                "a request with this idempotency token succeeded, but its response cannot be replayed because the operation's response has changed since; send a new token for a new request"
            }
        }
    }

    /// The error the API standard asks for, used when the service gives no
    /// factory: the service's validation error naming the token member for
    /// a missing or malformed token, and a 409 [`FrameworkError::Conflict`]
    /// otherwise. A factory can answer some cases itself and fall back to
    /// this for the rest.
    pub fn default_error(self) -> Error {
        match self {
            Self::Missing { member } | Self::Invalid { member } => {
                Rejection::constraint(member, self.message()).into()
            }
            Self::Mismatch | Self::InFlight | Self::Lapsed | Self::Unreplayable => {
                FrameworkError::Conflict {
                    message: self.message(),
                }
                .into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IdempotencyRefusal;
    use crate::FrameworkError;
    use crate::validate::Rejection;

    #[test]
    fn defaults_follow_the_standard() {
        for refusal in [
            IdempotencyRefusal::Missing {
                member: "clientToken",
            },
            IdempotencyRefusal::Invalid {
                member: "clientToken",
            },
        ] {
            let error = refusal.default_error();
            let rejection = error.downcast_ref::<Rejection>().expect("a rejection");
            assert_eq!(rejection.violations[0].path, "clientToken");
        }
        for refusal in [
            IdempotencyRefusal::Mismatch,
            IdempotencyRefusal::InFlight,
            IdempotencyRefusal::Lapsed,
            IdempotencyRefusal::Unreplayable,
        ] {
            let error = refusal.default_error();
            let conflict = error.downcast_ref::<FrameworkError>().expect("a conflict");
            assert_eq!(conflict.status(), 409);
            assert_eq!(conflict.name(), "ConflictException");
            assert_eq!(conflict.message(), refusal.message());
        }
        assert!(
            IdempotencyRefusal::InFlight
                .message()
                .contains("in progress")
        );
    }
}
