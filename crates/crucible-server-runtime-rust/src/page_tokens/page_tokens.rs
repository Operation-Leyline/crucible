use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::PageKeys;
use super::scope::PageScope;
use super::token::Sealer;
use crate::canonical_json::write_canonical;
use crate::validate::Rejection;
use crate::{Context, DecodedJob, Error, Hook, Interceptor, Prepare, Result};

/// The default token lifetime: long enough that no interactive session
/// notices, short enough that a leaked token is soon worthless.
const DEFAULT_TTL: Duration = Duration::from_secs(24 * 60 * 60);

type Caller = Arc<dyn Fn(&Context) -> Option<String> + Send + Sync>;
type Refusal = Arc<dyn Fn(Rejection) -> Error + Send + Sync>;

/// Seals the pagination tokens of every `@paginated` operation it serves, and
/// opens the one a request carries before the operation runs (see the
/// [module docs](super)).
///
/// An `Interceptor`: it reads the decoded input, so the binding is the same
/// whichever protocol or binding carried a filter. Operations that are not
/// `@paginated` pass through untouched.
pub struct PageTokens {
    sealer: Arc<Sealer>,
    caller: Caller,
    refusal: Refusal,
}

impl PageTokens {
    /// `caller` names who is asking, from the request context (an
    /// authenticated principal's issuer and subject, say), so one caller's
    /// token does not open for another. `None` is an anonymous caller, and
    /// every anonymous caller shares one binding: return a name for every
    /// authenticated request.
    pub fn new(
        keys: PageKeys,
        caller: impl Fn(&Context) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            sealer: Arc::new(Sealer {
                keys,
                ttl: DEFAULT_TTL,
                clock: Arc::new(SystemTime::now),
            }),
            caller: Arc::new(caller),
            refusal: Arc::new(Error::new),
        }
    }

    /// How long a token opens after it is issued. 24 hours by default. The
    /// expiry is sealed into each token, so a change applies to tokens
    /// issued after it.
    pub fn ttl(mut self, ttl: Duration) -> Self {
        Arc::make_mut(&mut self.sealer).ttl = ttl;
        self
    }

    /// The clock expiry is read from, for deterministic tests.
    pub fn clock(mut self, clock: impl Fn() -> SystemTime + Send + Sync + 'static) -> Self {
        Arc::make_mut(&mut self.sealer).clock = Arc::new(clock);
        self
    }

    /// How a token that does not open is answered. By default it is the
    /// rejection itself, which the service answers as its validation error
    /// naming the token member; a service with its own reason for it (an
    /// `invalidPaginationToken` enum value, say) builds that error here.
    pub fn refusal(mut self, refusal: impl Fn(Rejection) -> Error + Send + Sync + 'static) -> Self {
        self.refusal = Arc::new(refusal);
        self
    }

    fn scope(&self, job: &DecodedJob<'_>) -> Result<Option<PageScope>> {
        let Some(pagination) = job.metadata().pagination else {
            return Ok(None);
        };
        let mut input = job.input().to_json();
        let token = input.as_object_mut().and_then(|members| {
            if let Some(size) = pagination.page_size {
                members.remove(size.wire_name);
            }
            members.remove(pagination.input_token.wire_name)
        });
        let caller = (self.caller)(job.context());
        let binding = binding(
            job.service().id,
            job.operation().id,
            caller.as_deref(),
            &input,
        );
        let resumed = match token.as_ref().and_then(Value::as_str) {
            None | Some("") => None,
            Some(token) => Some(self.sealer.open(&binding, token).map_err(|_| {
                (self.refusal)(Rejection::constraint(
                    pagination.input_token.name,
                    "the pagination token is not valid for this request; start the listing again",
                ))
            })?),
        };
        Ok(Some(PageScope::new(self.sealer.clone(), binding, resumed)))
    }
}

impl Interceptor for PageTokens {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        Hook::Ready(self.scope(job).map(|scope| {
            if let Some(scope) = scope {
                job.context().set_attribute(scope);
            }
        }))
    }

    fn prepare(&mut self, _: &Prepare<'_>) -> std::result::Result<(), String> {
        if self.sealer.ttl.is_zero() {
            return Err("a page token lifetime must be positive".to_owned());
        }
        Ok(())
    }
}

/// What a token is bound to: the service, the operation, the caller and the
/// rest of the input. Each part is length-prefixed, so no two different
/// requests digest alike by moving bytes from one part to the next. The input
/// is written in a canonical form, so its digest does not depend on the order
/// members arrived in or a map iterated in.
fn binding(service: &str, operation: &str, caller: Option<&str>, input: &Value) -> [u8; 32] {
    let mut canonical = Vec::new();
    write_canonical(input, &mut canonical);
    let input = canonical;
    let mut digest = Sha256::new();
    digest.update(b"crucible page token binding v1");
    for part in [
        service.as_bytes(),
        operation.as_bytes(),
        caller.map_or(&b""[..], str::as_bytes),
        &input,
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.update([u8::from(caller.is_some())]);
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::binding;

    #[test]
    fn every_part_of_the_request_binds() {
        let base = binding("s#S", "s#List", Some("alice"), &json!({"tag": "a"}));
        assert_eq!(
            base,
            binding("s#S", "s#List", Some("alice"), &json!({"tag": "a"}))
        );
        for other in [
            binding("s#T", "s#List", Some("alice"), &json!({"tag": "a"})),
            binding("s#S", "s#Other", Some("alice"), &json!({"tag": "a"})),
            binding("s#S", "s#List", Some("bob"), &json!({"tag": "a"})),
            binding("s#S", "s#List", None, &json!({"tag": "a"})),
            binding("s#S", "s#List", Some(""), &json!({"tag": "a"})),
            binding("s#S", "s#List", Some("alice"), &json!({"tag": "b"})),
            binding("s#S", "s#List", Some("alice"), &json!({})),
        ] {
            assert_ne!(base, other);
        }
    }

    #[test]
    fn parts_cannot_trade_bytes() {
        assert_ne!(
            binding("ab", "c", None, &json!({})),
            binding("a", "bc", None, &json!({}))
        );
    }

    #[test]
    fn nested_member_order_does_not_matter() {
        let one: serde_json::Value =
            serde_json::from_str(r#"{"f":{"x":[{"b":1,"a":2}],"y":"q"},"e":null}"#).unwrap();
        let two: serde_json::Value =
            serde_json::from_str(r#"{"e":null,"f":{"y":"q","x":[{"a":2,"b":1}]}}"#).unwrap();
        assert_eq!(binding("s", "o", None, &one), binding("s", "o", None, &two));
        // Arrays keep their order: a list filter in another order is another listing.
        assert_ne!(
            binding("s", "o", None, &json!({"t": ["a", "b"]})),
            binding("s", "o", None, &json!({"t": ["b", "a"]}))
        );
    }

    #[test]
    fn member_order_does_not_matter() {
        let one: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        let two: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        assert_eq!(binding("s", "o", None, &one), binding("s", "o", None, &two));
    }
}
