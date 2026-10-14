//! Authentication across API versions: an older version's entry keeps its
//! own authentication facts, so an extension scoped to that version's
//! description prepares from them, not from the current version's.

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
static SERVICE: ServiceInfo = generated::service_info(
    "example.versions#Versions",
    "v2",
    None,
    None,
    &[&CURRENT],
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
    &[&OLDER],
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
        .versions(V2, &OLDER_VERSIONS)
        .add_versioned::<Older, Current>(V1, &[], None)
        .build()
        .unwrap()
}

async fn status(service: &Crucible, version: &str) -> u16 {
    let request = Request::get("/probe")
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
