use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::PageKeys;
use super::scope::PageScope;
use super::token::Sealer;
use crate::caller_binding::CallerBinding;
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
    caller: CallerBinding,
    refusal: Refusal,
}

impl PageTokens {
    /// `caller` names who is asking, from the request context, so one
    /// caller's token does not open for another. The name must carry
    /// everything that scopes what the caller may see and is not in the
    /// operation's input (the principal's issuer, subject and tenant),
    /// encoded unambiguously (a JSON array, as
    /// `crucible_auth::Principal::caller_binding` writes).
    ///
    /// `None` names no one. A request it is returned for is refused as the
    /// service's internal error, unless [`allow_anonymous`](Self::allow_anonymous)
    /// lets it share the anonymous binding; on an operation that requires
    /// authentication it is refused regardless, since its caller was
    /// authenticated and the closure missed them.
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
            caller: CallerBinding::new(caller),
            refusal: Arc::new(Error::new),
        }
    }

    /// Lets every request the caller binding names no one for share one
    /// anonymous binding, on operations that admit anonymous callers
    /// (`@optionalAuth`, or no authentication): any anonymous caller's
    /// token opens for any other with the same filters. Without it `build`
    /// refuses an operation in scope that admits anonymous callers, since
    /// its anonymous requests would be refused.
    pub fn allow_anonymous(mut self) -> Self {
        self.caller.allow_anonymous();
        self
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
        let caller =
            self.caller
                .bind("PageTokens", job.context(), job.operation(), job.metadata())?;
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
        // as absent would restart every listing on its first page. Asked per
        // served entry: an older API version served by a handler of its own
        // has that handler's token.
        let mut problems: Vec<String> = prepare
            .served()
            .filter(|served| prepare.applies(served.info))
            .filter_map(|served| {
                let pagination = served
                    .metadata
                    .pagination
                    .filter(|pagination| pagination.input_token_map)?;
                Some(format!(
                    "{}: PageTokens seals string tokens, and the page token `{}` is a map; \
                     leave the operation out of its scope",
                    served.label(),
                    pagination.input_token.name
                ))
            })
            .collect();
        if let Err(problem) = self.caller.prepare("PageTokens", prepare, |metadata| {
            metadata.pagination.is_some()
        }) {
            problems.push(problem);
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
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
        AuthScheme, ErrorInfo, Fault, Operation, OperationInfo, Pagination, PaginationMember,
        ServiceInfo,
    };
    use crate::validate::{Validate, Validator};
    use crate::versioning::ApiVersion;
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

    fn keys() -> super::PageKeys {
        super::PageKeys::new("k1", b"0123456789abcdef0123456789abcdef").unwrap()
    }

    fn tokens() -> PageTokens {
        PageTokens::new(keys(), |_| None).allow_anonymous()
    }

    /// A listing only authenticated callers reach.
    struct Authenticated;
    impl Operation for Authenticated {
        const INFO: &'static OperationInfo = &LIST;
        const PAGINATION: Option<Pagination> = pagination(false);
        const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
        type Input = Loose;
        type Output = Value;
    }

    /// The caller named by the request's `x-caller` header.
    fn header_caller(context: &crate::Context) -> Option<String> {
        context
            .headers()
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
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
            why.starts_with("List: ") && why.contains("`token` is a map"),
            "{why}"
        );
    }

    static LIST_V1: OperationInfo = operation_info(
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

    static RELEASE_V1: ServiceInfo = service_info(
        "t#Service",
        "v1",
        None,
        None,
        &[&LIST_V1],
        &[],
        classify,
        Some(|_| Box::new(Invalid)),
        None,
        None,
    );

    const V1: ApiVersion = ApiVersion::new(1, "v1");
    static OLDER: [(ApiVersion, Option<&str>, &ServiceInfo); 1] = [(V1, None, &RELEASE_V1)];

    /// v1's List, whose token was a map, kept by a handler of its own.
    struct LegacyMapToken;
    impl Operation for LegacyMapToken {
        const INFO: &'static OperationInfo = &LIST_V1;
        const PAGINATION: Option<Pagination> = pagination(true);
        type Input = Loose;
        type Output = Value;
    }

    #[test]
    fn an_older_version_served_with_a_map_token_is_refused() {
        // The current List seals a string token; v1's, served by its own
        // handler, has a map, which the current entry does not show.
        let ok = |_: Arc<()>, _| std::future::ready(Ok(json!({})));
        let operations = Operations::builder(&SERVICE)
            .add::<StringToken, _, _>(Arc::new(()), ok)
            .versions(ApiVersion::new(2, "v2"), &OLDER)
            .add_legacy::<LegacyMapToken, _, _>(V1, &RELEASE_V1, Arc::new(()), ok)
            .add_versioned::<LegacyMapToken, LegacyMapToken>(V1, &[], Some((V1, "List v1")))
            .build()
            .unwrap();
        let built = Crucible::builder(operations)
            .protocol(InvokeProtocol)
            .interceptor(tokens())
            .build();
        let Err(BuildError::Prepare(why)) = built else {
            panic!("built");
        };
        assert!(
            why.contains("List (API version v1)") && why.contains("`token` is a map"),
            "{why}"
        );
    }

    /// v1's List, public before a migration made it require authentication.
    struct PublicV1;
    impl Operation for PublicV1 {
        const INFO: &'static OperationInfo = &LIST_V1;
        const PAGINATION: Option<Pagination> = pagination(false);
        const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
        type Input = Loose;
        type Output = Value;
    }

    /// v1's List, authenticated before a migration made it public.
    struct BearerV1;
    impl Operation for BearerV1 {
        const INFO: &'static OperationInfo = &LIST_V1;
        const PAGINATION: Option<Pagination> = pagination(false);
        const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
        type Input = Loose;
        type Output = Value;
    }

    /// `Current` at v2, with `Old` serving v1 through it, under `tokens`.
    fn versioned<Current, Old>(tokens: PageTokens) -> Result<Crucible, BuildError>
    where
        Current: Operation<Input = Loose, Output = Value>,
        Old: Operation<Output = Value>,
    {
        let ok = |_: Arc<()>, _| std::future::ready(Ok(json!({})));
        let operations = Operations::builder(&SERVICE)
            .add::<Current, _, _>(Arc::new(()), ok)
            .versions(ApiVersion::new(2, "v2"), &OLDER)
            .add_versioned::<Old, Current>(V1, &[], None)
            .build()
            .unwrap();
        Crucible::builder(operations)
            .protocol(InvokeProtocol)
            .interceptor(tokens)
            .build()
    }

    // The caller binding reads authentication from the entry a request's
    // API version serves, as `job.metadata()` gives it, not from the current
    // entry: a migration that changes an operation's authentication is
    // judged by the version the request names. (Until #288, the
    // AuthenticatorChain plans per operation from the current entry, so the
    // two can disagree for such a migration; #288 makes the chain per
    // version too.)
    #[test]
    fn anonymous_callers_are_judged_by_each_api_versions_authentication() {
        // v1 was public, v2 requires authentication: only v1's anonymous
        // callers need the choice.
        let Err(BuildError::Prepare(why)) =
            versioned::<Authenticated, PublicV1>(PageTokens::new(keys(), |_| None))
        else {
            panic!("built");
        };
        assert!(
            why.contains("anonymous callers reach List (API version v1) ("),
            "{why}"
        );
        assert!(
            versioned::<Authenticated, PublicV1>(
                PageTokens::new(keys(), |_| None).allow_anonymous()
            )
            .is_ok()
        );
        // v1 required authentication, v2 is public: only v2's.
        let Err(BuildError::Prepare(why)) =
            versioned::<StringToken, BearerV1>(PageTokens::new(keys(), |_| None))
        else {
            panic!("built");
        };
        assert!(why.contains("anonymous callers reach List ("), "{why}");
        assert!(!why.contains("API version v1"), "{why}");
    }

    #[test]
    fn a_map_token_and_anonymous_callers_are_both_reported() {
        let Err(BuildError::Prepare(why)) =
            service::<MapToken>(PageTokens::new(keys(), |_| None)).0
        else {
            panic!("built");
        };
        assert!(why.contains("`token` is a map"), "{why}");
        assert!(why.contains("anonymous callers reach List ("), "{why}");
    }

    #[test]
    fn sharing_one_anonymous_binding_is_an_explicit_choice() {
        // The listing admits anonymous callers, whom the binding cannot tell
        // apart: the service must choose to let them share one binding.
        let why = refusal(PageTokens::new(keys(), |_| None));
        assert!(why.contains("anonymous callers reach List ("), "{why}");
        assert!(why.contains("PageTokens::allow_anonymous"), "{why}");
        assert!(
            service::<StringToken>(PageTokens::new(keys(), |_| None).allow_anonymous())
                .0
                .is_ok()
        );
        // An operation that requires authentication has no anonymous callers.
        assert!(
            service::<Authenticated>(PageTokens::new(keys(), |_| None))
                .0
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_binding_that_names_no_one_is_refused_on_an_authenticated_operation() {
        // Anonymous callers allowed, a nameless request to a listing that
        // admits them shares the anonymous binding.
        let (open, ran) = service::<StringToken>(tokens());
        let answer = open.unwrap().invoke("List", json!({})).await;
        assert!(answer["output"].is_object(), "{answer}");
        assert!(ran.load(Ordering::Relaxed));
        // A listing that requires authentication has no anonymous callers,
        // so a binding that names no one missed an authenticated caller:
        // the service's internal error, never the anonymous binding.
        let (closed, ran) = service::<Authenticated>(PageTokens::new(keys(), |_| None));
        let answer = closed.unwrap().invoke("List", json!({})).await;
        assert_eq!(answer["error"]["__type"], "InternalFailure", "{answer}");
        assert!(!ran.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn an_authenticated_caller_the_binding_misses_is_refused() {
        // With anonymous callers allowed, a request to an operation that
        // requires authentication still never falls into the shared binding:
        // its caller was authenticated, and the binding did not recognise
        // them.
        let (built, ran) =
            service::<Authenticated>(PageTokens::new(keys(), header_caller).allow_anonymous());
        let built = built.unwrap();
        let answer = built.invoke("List", json!({})).await;
        assert_eq!(answer["error"]["__type"], "InternalFailure", "{answer}");
        assert!(!ran.load(Ordering::Relaxed));
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
