//! Authentication across API versions: an older version's entry keeps its
//! own authentication facts, so an extension prepares from them, not from
//! the current version's, and a request at that version is authenticated by
//! its own release's contract, scoped chain or not.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use crucible_server_runtime_rust::__generated as generated;
use crucible_server_runtime_rust::{
    Crucible, Operation, OperationMiddlewareExt, Operations, Unversioned,
    operation::{AuthScheme, OperationInfo, Segment, ServiceInfo, Unit},
    versioning::ApiVersion,
};

const V1: ApiVersion = ApiVersion::new(0, "v1");
const V2: ApiVersion = ApiVersion::new(1, "v2");

/// The operation as v2 describes it: authentication optional.
static CURRENT: OperationInfo = generated::operation_info(
    "example.versions#Probe",
    "Probe",
    None,
    "GET",
    &[Segment::Literal("probe")],
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
/// The same operation as v1 described it: authentication required.
static OLDER: OperationInfo = generated::operation_info(
    "example.versions#Probe",
    "Probe",
    None,
    "GET",
    &[Segment::Literal("probe")],
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
/// v2 drops authentication altogether (`auth []`) from an operation v1
/// required it on.
static OPEN_CURRENT: OperationInfo = generated::operation_info(
    "example.versions#Open",
    "Open",
    None,
    "GET",
    &[Segment::Literal("open")],
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
static OPEN_OLDER: OperationInfo = generated::operation_info(
    "example.versions#Open",
    "Open",
    None,
    "GET",
    &[Segment::Literal("open")],
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
/// The other direction: v1 served the operation without authentication,
/// v2 requires bearer auth.
static GATE_CURRENT: OperationInfo = generated::operation_info(
    "example.versions#Gate",
    "Gate",
    None,
    "GET",
    &[Segment::Literal("gate")],
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
static GATE_OLDER: OperationInfo = generated::operation_info(
    "example.versions#Gate",
    "Gate",
    None,
    "GET",
    &[Segment::Literal("gate")],
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
static SERVICE: ServiceInfo = generated::service_info(
    "example.versions#Versions",
    "v2",
    None,
    None,
    &[&CURRENT, &OPEN_CURRENT, &GATE_CURRENT],
    &[],
    |_| None,
    None,
    None,
    None,
);
static RELEASE_V1: ServiceInfo = generated::service_info(
    "example.versions#Versions",
    "v1",
    None,
    None,
    &[&OLDER, &OPEN_OLDER, &GATE_OLDER],
    &[],
    |_| None,
    None,
    None,
    None,
);

static OLDER_VERSIONS: [(ApiVersion, Option<&str>, &ServiceInfo); 1] = [(V1, None, &RELEASE_V1)];

struct Current;
impl Operation for Current {
    const INFO: &'static OperationInfo = &CURRENT;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const OPTIONAL_AUTH: bool = true;
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct Older;
impl Operation for Older {
    const INFO: &'static OperationInfo = &OLDER;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct OpenCurrent;
impl Operation for OpenCurrent {
    const INFO: &'static OperationInfo = &OPEN_CURRENT;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct OpenOlder;
impl Operation for OpenOlder {
    const INFO: &'static OperationInfo = &OPEN_OLDER;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct GateCurrent;
impl Operation for GateCurrent {
    const INFO: &'static OperationInfo = &GATE_CURRENT;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct GateOlder;
impl Operation for GateOlder {
    const INFO: &'static OperationInfo = &GATE_OLDER;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

/// Supports bearer tokens and refuses every one: these tests send none.
struct Bearer;
#[crucible_server_runtime_rust::async_trait]
impl crucible_auth::Authenticator for Bearer {
    fn supports(&self, scheme: &AuthScheme) -> bool {
        matches!(scheme, AuthScheme::Bearer)
    }
    fn has_credentials(&self, request: &Parts, _: &AuthScheme) -> bool {
        request.headers.contains_key(AUTHORIZATION)
    }
    async fn authenticate(&self, _: &Parts, _: &AuthScheme) -> crucible_auth::AuthenticateResult {
        crucible_auth::AuthenticateResult::Refused
    }
}

fn operations() -> Operations {
    Operations::builder(&SERVICE)
        .add::<Current, _, _>(Arc::new(()), |_, _| async { Ok(Unit {}) })
        .add::<OpenCurrent, _, _>(Arc::new(()), |_, _| async { Ok(Unit {}) })
        .add::<GateCurrent, _, _>(Arc::new(()), |_, _| async { Ok(Unit {}) })
        .versions(V2, &OLDER_VERSIONS)
        .add_versioned::<Older, Current>(V1, &[], None)
        .add_versioned::<OpenOlder, OpenCurrent>(V1, &[], None)
        .add_versioned::<GateOlder, GateCurrent>(V1, &[], None)
        .build()
        .unwrap()
}

async fn status(service: &Crucible, version: &str) -> u16 {
    call(service, "/probe", version).await
}

async fn call(service: &Crucible, path: &str, version: &str) -> u16 {
    let request = Request::get(path)
        .header("crucible-version", version)
        .body(Body::empty())
        .unwrap();
    service.handle(request).await.status().as_u16()
}

/// An authentication chain scoped to v1's description alone prepares v1's
/// plan from v1's own schemes: a v1 request without credentials is refused,
/// though the current version makes authentication optional.
#[tokio::test]
async fn an_extension_scoped_to_an_older_version_prepares_from_its_own_metadata() {
    let service = Crucible::builder(operations())
        .defaults()
        .api_version(Unversioned::Oldest)
        // These descriptions have no validation error of their own.
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(
            crucible_auth::AuthenticatorChain::new(vec![Arc::new(Bearer)])
                .when(|op| std::ptr::eq(op, &OLDER)),
        )
        .build()
        .unwrap();
    assert_eq!(status(&service, "v1").await, 401);
    // The current version is outside the chain's scope.
    assert_eq!(status(&service, "v2").await, 200);
}

/// An unscoped chain: every API version's request is authenticated by its
/// own release's contract, not by the current version's.
fn unscoped() -> Crucible {
    Crucible::builder(operations())
        .defaults()
        .api_version(Unversioned::Oldest)
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(crucible_auth::AuthenticatorChain::new(vec![Arc::new(
            Bearer,
        )]))
        .build()
        .unwrap()
}

/// v1 required bearer auth where v2 makes it optional: a v1 request without
/// credentials is refused, a v2 one passes.
#[tokio::test]
async fn an_unscoped_chain_keeps_an_older_versions_required_authentication() {
    let service = unscoped();
    assert_eq!(call(&service, "/probe", "v1").await, 401);
    assert_eq!(call(&service, "/probe", "v2").await, 200);
}

/// v1 required bearer auth where v2 has no auth schemes at all.
#[tokio::test]
async fn an_unscoped_chain_refuses_an_older_version_that_required_authentication_now_dropped() {
    let service = unscoped();
    assert_eq!(call(&service, "/open", "v1").await, 401);
    assert_eq!(call(&service, "/open", "v2").await, 200);
}

/// The other direction: v1 had no auth schemes where v2 requires bearer
/// auth. v1's clients were promised an unauthenticated call; v2's were not.
#[tokio::test]
async fn an_unscoped_chain_keeps_an_older_versions_unauthenticated_access() {
    let service = unscoped();
    assert_eq!(call(&service, "/gate", "v1").await, 200);
    assert_eq!(call(&service, "/gate", "v2").await, 401);
}

/// One served entry as `Prepare::served_operations` lists it: the name,
/// the older API version, whether the description is v1's own, and the
/// entry's own authentication facts.
type Listed = (&'static str, Option<ApiVersion>, bool, bool, usize);

/// Records what `Prepare::served_operations` lists for its scope.
struct Recorder(Arc<std::sync::Mutex<Vec<Listed>>>);
impl crucible_server_runtime_rust::OperationAwareMiddleware for Recorder {
    fn prepare(
        &mut self,
        prepare: &crucible_server_runtime_rust::Prepare<'_>,
    ) -> Result<(), String> {
        let older = [&OLDER, &OPEN_OLDER, &GATE_OLDER];
        let mut seen = self.0.lock().unwrap();
        for served in prepare.served_operations() {
            let metadata = served.metadata();
            seen.push((
                served.operation().name,
                served.api_version(),
                older.iter().any(|op| std::ptr::eq(*op, served.operation())),
                metadata.optional_auth,
                metadata.auth_schemes.unwrap().len(),
            ));
        }
        Ok(())
    }
}

/// Each API version's entry is listed with its own description and its own
/// authentication facts, and the scope is asked of that description: a
/// scope that takes v1's `Gate` alone lists no other version's.
#[test]
fn served_operations_lists_each_versions_own_entry_within_the_scope() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    Crucible::builder(operations())
        .defaults()
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(Recorder(seen.clone()))
        .operation_middleware(
            Recorder(seen.clone()).when(|op| op.name != "Gate" || std::ptr::eq(op, &GATE_OLDER)),
        )
        .build()
        .unwrap();
    let seen = seen.lock().unwrap();
    let gate = ("Gate", None, false, false, 1);
    let open = ("Open", None, false, false, 0);
    let probe = ("Probe", None, false, true, 1);
    let gate_v1 = ("Gate", Some(V1), true, false, 0);
    let open_v1 = ("Open", Some(V1), true, false, 1);
    let probe_v1 = ("Probe", Some(V1), true, false, 1);
    assert_eq!(
        *seen,
        [
            // Unscoped: every entry, the current version's first.
            gate, open, probe, gate_v1, open_v1, probe_v1,
            // Scoped: v2's Gate is outside, v1's inside.
            open, probe, gate_v1, open_v1, probe_v1,
        ]
    );
}

/// An operation v3 removed, which v1 and v2 still serve through v2's legacy
/// handler (`served_by`): v1 required bearer auth on it, v2 dropped auth.
const L1: ApiVersion = ApiVersion::new(0, "v1");
const L2: ApiVersion = ApiVersion::new(1, "v2");
const L3: ApiVersion = ApiVersion::new(2, "v3");

static RETIRED_V1: OperationInfo = generated::operation_info(
    "example.legacy#Retired",
    "Retired",
    None,
    "GET",
    &[Segment::Literal("retired")],
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
static RETIRED_V2: OperationInfo = generated::operation_info(
    "example.legacy#Retired",
    "Retired",
    None,
    "GET",
    &[Segment::Literal("retired")],
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
static LEGACY_SERVICE: ServiceInfo = generated::service_info(
    "example.legacy#Legacy",
    "v3",
    None,
    None,
    &[],
    &[],
    |_| None,
    None,
    None,
    None,
);
static LEGACY_RELEASE_V1: ServiceInfo = generated::service_info(
    "example.legacy#Legacy",
    "v1",
    None,
    None,
    &[&RETIRED_V1],
    &[],
    |_| None,
    None,
    None,
    None,
);
static LEGACY_RELEASE_V2: ServiceInfo = generated::service_info(
    "example.legacy#Legacy",
    "v2",
    None,
    None,
    &[&RETIRED_V2],
    &[],
    |_| None,
    None,
    None,
    None,
);
static LEGACY_OLDER: [(ApiVersion, Option<&str>, &ServiceInfo); 2] = [
    (L1, None, &LEGACY_RELEASE_V1),
    (L2, None, &LEGACY_RELEASE_V2),
];

struct RetiredV1;
impl Operation for RetiredV1 {
    const INFO: &'static OperationInfo = &RETIRED_V1;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[AuthScheme::Bearer]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

struct RetiredV2;
impl Operation for RetiredV2 {
    const INFO: &'static OperationInfo = &RETIRED_V2;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
    const HAS_INPUT: bool = false;
    const HAS_OUTPUT: bool = false;
    type Input = Unit;
    type Output = Unit;
}

/// Each legacy entry is authenticated by its own release's schemes, though
/// both are served by v2's handler and the current version has no entry
/// for the operation to describe it by.
#[tokio::test]
async fn a_legacy_served_by_entry_keeps_its_own_releases_authentication() {
    let operations = Operations::builder(&LEGACY_SERVICE)
        .versions(L3, &LEGACY_OLDER)
        .add_legacy::<RetiredV2, _, _>(L2, &LEGACY_RELEASE_V2, Arc::new(()), |_, _| async {
            Ok(Unit {})
        })
        .add_versioned::<RetiredV2, RetiredV2>(L2, &[], Some((L2, "RetiredV2")))
        .add_versioned::<RetiredV1, RetiredV2>(L1, &[], Some((L2, "RetiredV2")))
        .build()
        .unwrap();
    let service = Crucible::builder(operations)
        .defaults()
        .api_version(Unversioned::Oldest)
        .on_rejection(weather_service::weather::SERVICE.reject.unwrap())
        .operation_middleware(crucible_auth::AuthenticatorChain::new(vec![Arc::new(
            Bearer,
        )]))
        .build()
        .unwrap();
    assert_eq!(call(&service, "/retired", "v1").await, 401);
    assert_eq!(call(&service, "/retired", "v2").await, 200);
}
