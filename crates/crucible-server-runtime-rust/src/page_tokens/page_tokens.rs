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

/// The longest token lifetime. A token cannot be revoked short of dropping
/// its key, so one that outlives every listing it could resume is only a
/// liability; a longer lifetime is almost always a unit slip.
const MAX_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

type Caller = Arc<dyn Fn(&Context) -> Option<String> + Send + Sync>;
type Refusal = Arc<dyn Fn(Rejection) -> Error + Send + Sync>;

/// Seals the pagination tokens of every `@paginated` operation it serves, and
/// opens the one a request carries before the operation runs (see the
/// [module docs](super)).
///
/// An `Interceptor`: it reads the decoded input, so the binding is the same
/// whichever protocol or binding carried a filter. Operations that are not
/// `@paginated` pass through untouched. Register it after `Permissions`
/// (the other order fails the build), so a refused request is refused
/// before its token is opened.
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

    /// How long a token opens after it is issued: 24 hours by default, from
    /// one second to 30 days (`build` refuses any other), counted in whole
    /// seconds. The expiry is sealed into each token, so a change applies
    /// only to tokens issued after it: lowering it revokes nothing already
    /// issued (dropping the key that sealed them does).
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
        // An input is a structure, so its JSON form is an object; anything
        // else (a value that did not serialize) would make a token the
        // request carries look absent and bind the listing to nothing.
        let mut input = job.input().to_json();
        let Some(members) = input.as_object_mut() else {
            return Err(Error::internal(format!(
                "{}: the input is not an object, so its page token cannot be read",
                job.operation().name
            )));
        };
        if let Some(size) = pagination.page_size {
            members.remove(size.wire_name);
        }
        let token = members.remove(pagination.input_token.wire_name);
        let caller = (self.caller)(job.context());
        let binding = binding(
            job.service().id,
            job.operation().id,
            caller.as_deref(),
            &input,
        );
        let resumed = match token {
            None | Some(Value::Null) => None,
            Some(Value::String(token)) if token.is_empty() => None,
            Some(Value::String(token)) => Some(self.sealer.open(&binding, &token).map_err(|_| {
                (self.refusal)(Rejection::constraint(
                    pagination.input_token.name,
                    "the pagination token is not valid for this request; start the listing again",
                ))
            })?),
            // `build` refuses an operation whose token is a map, so this is
            // an input whose token member is not the model's.
            Some(_) => {
                return Err(Error::internal(format!(
                    "{}: the page token member `{}` is not a string",
                    job.operation().name,
                    pagination.input_token.name
                )));
            }
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

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        // Records the registration order for `Permissions`, which refuses
        // to follow.
        prepare.builtin("page_tokens", "permissions");
        let ttl = self.sealer.ttl;
        if ttl < Duration::from_secs(1) || ttl > MAX_TTL {
            return Err(format!(
                "a page token lifetime is from one second to 30 days, not {ttl:?}"
            ));
        }
        // A sealed token is a string, so an operation whose token is a map
        // (which Smithy allows) cannot resume from one; reading its token
        // as absent would restart every listing on its first page.
        for operation in prepare.operations() {
            let token = prepare
                .metadata(operation)
                .and_then(|metadata| metadata.pagination)
                .filter(|pagination| pagination.input_token_map);
            if let Some(pagination) = token {
                return Err(format!(
                    "{}: PageTokens seals string tokens, and the page token `{}` is a map; \
                     leave the operation out of its scope",
                    operation.id, pagination.input_token.name
                ));
            }
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    use crucible_wire::__generated::{error_info, operation_info, service_info};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};

    use super::{PageTokens, binding};
    use crate::error::ModelledError;
    use crate::operation::{
        ErrorInfo, Fault, Operation, OperationInfo, Pagination, PaginationMember, ServiceInfo,
    };
    use crate::validate::{Validate, Validator};
    use crate::{BuildError, Crucible, InvokeProtocol, Operations};

    static LIST: OperationInfo = operation_info(
        "t#List",
        "List",
        None,
        "POST",
        &[],
        &[],
        200,
        &[],
        &[],
        &[],
        true,
        false,
        false,
        None,
        &[],
    );

    fn classify<'a>(_: &'a (dyn std::error::Error + 'static)) -> Option<&'a dyn ModelledError> {
        None
    }

    static INVALID: ErrorInfo = error_info("t#Invalid", 400, Fault::Client, false, false, &[]);

    /// The service's validation error.
    #[derive(Debug)]
    struct Invalid;

    impl std::fmt::Display for Invalid {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("invalid")
        }
    }

    impl std::error::Error for Invalid {}

    impl ModelledError for Invalid {
        fn info(&self) -> &'static ErrorInfo {
            &INVALID
        }

        fn to_json(&self) -> Value {
            json!({})
        }
    }

    static SERVICE: ServiceInfo = service_info(
        "t#Service",
        "v1",
        None,
        None,
        &[&LIST],
        &[],
        classify,
        Some(|_| Box::new(Invalid)),
        None,
        None,
    );

    const fn pagination(input_token_map: bool) -> Option<Pagination> {
        Some(Pagination {
            input_token: PaginationMember {
                name: "token",
                wire_name: "token",
            },
            page_size: None,
            input_token_map,
        })
    }

    /// An input whose token member holds whatever JSON it is sent.
    #[derive(Serialize, Deserialize)]
    struct Loose {
        token: Option<Value>,
    }

    /// An input whose JSON form is not an object, as no structure's is.
    #[derive(Serialize, Deserialize)]
    struct Bare(String);

    impl Validate for Loose {
        fn validate(&self, _: &mut Validator) {}
    }

    impl Validate for Bare {
        fn validate(&self, _: &mut Validator) {}
    }

    struct StringToken;
    impl Operation for StringToken {
        const INFO: &'static OperationInfo = &LIST;
        const PAGINATION: Option<Pagination> = pagination(false);
        type Input = Loose;
        type Output = Value;
    }

    struct MapToken;
    impl Operation for MapToken {
        const INFO: &'static OperationInfo = &LIST;
        const PAGINATION: Option<Pagination> = pagination(true);
        type Input = Loose;
        type Output = Value;
    }

    struct BareInput;
    impl Operation for BareInput {
        const INFO: &'static OperationInfo = &LIST;
        const PAGINATION: Option<Pagination> = pagination(false);
        type Input = Bare;
        type Output = Value;
    }

    fn tokens() -> PageTokens {
        let keys = super::PageKeys::new("k1", b"0123456789abcdef0123456789abcdef").unwrap();
        PageTokens::new(keys, |_| None)
    }

    /// The service serving `O`, and whether its implementation ran.
    fn service<O: Operation<Output = Value>>(
        tokens: PageTokens,
    ) -> (Result<Crucible, BuildError>, Arc<AtomicBool>) {
        let ran = Arc::new(AtomicBool::new(false));
        let operations = Operations::builder(&SERVICE)
            .add::<O, _, _>(ran.clone(), |ran, _| {
                ran.store(true, Ordering::Relaxed);
                std::future::ready(Ok(json!({})))
            })
            .build()
            .unwrap();
        let service = Crucible::builder(operations)
            .protocol(InvokeProtocol)
            .interceptor(tokens)
            .build();
        (service, ran)
    }

    fn refusal(tokens: PageTokens) -> String {
        match service::<StringToken>(tokens).0 {
            Err(BuildError::Prepare(why)) => why,
            Err(other) => panic!("{other}"),
            Ok(_) => panic!("built"),
        }
    }

    #[test]
    fn a_lifetime_is_from_a_second_to_thirty_days() {
        for ttl in [
            Duration::ZERO,
            Duration::from_millis(999),
            Duration::from_secs(30 * 24 * 60 * 60 + 1),
            Duration::MAX,
        ] {
            let why = refusal(tokens().ttl(ttl));
            assert!(why.contains("from one second to 30 days"), "{why}");
        }
        for ttl in [
            Duration::from_secs(1),
            Duration::from_secs(30 * 24 * 60 * 60),
        ] {
            assert!(service::<StringToken>(tokens().ttl(ttl)).0.is_ok());
        }
    }

    #[test]
    fn an_operation_with_a_map_token_is_refused() {
        let Err(BuildError::Prepare(why)) = service::<MapToken>(tokens()).0 else {
            panic!("built");
        };
        assert!(
            why.contains("t#List") && why.contains("`token` is a map"),
            "{why}"
        );
    }

    #[tokio::test]
    async fn a_token_that_is_not_a_string_is_not_read_as_absent() {
        let (service, ran) = service::<StringToken>(tokens());
        let service = service.unwrap();
        let answer = service.invoke("List", json!({ "token": {"k": "v"} })).await;
        assert!(answer["error"].is_object(), "{answer}");
        assert!(!ran.load(Ordering::Relaxed));
        // A string and an absent token both reach the operation.
        for input in [json!({}), json!({ "token": null }), json!({ "token": "" })] {
            let answer = service.invoke("List", input).await;
            assert!(answer["output"].is_object(), "{answer}");
        }
    }

    #[tokio::test]
    async fn an_input_that_is_not_an_object_is_an_internal_error() {
        let (service, ran) = service::<BareInput>(tokens());
        let answer = service.unwrap().invoke("List", json!("x")).await;
        assert!(answer["error"].is_object(), "{answer}");
        assert!(!ran.load(Ordering::Relaxed));
    }

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
