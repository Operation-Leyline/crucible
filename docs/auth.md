# JWT bearer authentication

Install `crucible-auth` explicitly to verify JWTs and enforce the Smithy model's
authentication settings. `Crucible::defaults()` keeps authentication off. The
separate crate keeps JWT cryptography, JWKS HTTP fetching, and test issuers out
of services that do not use bearer authentication.

Authentication establishes identity. Services still own API-key storage,
tenant membership, permissions, revocation policy, and authorization.

## Configuring Cognito

Declare the authentication schemes on the service. Operation `@auth` overrides
the effective scheme list; `@auth([])` makes an operation public.

```smithy
@httpBearerAuth
@httpApiKeyAuth(name: "x-api-key", in: "header")
@auth([httpBearerAuth, httpApiKeyAuth])
service Example {
    version: "1"
    operations: [Read, Health]
}

@optionalAuth
@http(method: "GET", uri: "/read", code: 200)
operation Read {}

@auth([])
@http(method: "GET", uri: "/health", code: 200)
operation Health {}
```

Add `crucible-auth` from the same revision as your runtime. Construct an ID-token
authenticator with the pool's region, pool ID, and an explicit list of allowed
app client IDs:

```rust
use std::sync::Arc;
use crucible_auth::{AuthenticatorChain, IssuerConfig, JwtAuthenticator};

let config = IssuerConfig::cognito(
    "us-east-1", "us-east-1_EXAMPLE", vec!["APP_CLIENT_ID".into()],
);
let jwt = Arc::new(JwtAuthenticator::new(config)?);
let service = builder.operation_middleware(AuthenticatorChain::new(vec![jwt]))
    .build()?;
```

Register a custom API-key authenticator too when the service declares API-key
auth. Construction fails with collected operation IDs when a modeled scheme
has no configured authenticator. Bindings without explicit auth metadata also
fail at construction; regenerate old bindings before installing the chain.

The Cognito preset constructs
`https://cognito-idp.REGION.amazonaws.com/POOL_ID` as the issuer and appends
`/.well-known/jwks.json` for its keys. It permits RS256, requires
`token_use == "id"`, and checks `aud` against allowed app clients. `aud` can be
a string or an array of strings.

Use `IssuerConfig::cognito_access` for a separate access-token authenticator.
It requires `token_use == "access"` and checks `client_id`; no `aud` is required.
These are the distinct client claims described in
[Cognito token verification](https://docs.aws.amazon.com/cognito/latest/developerguide/amazon-cognito-user-pools-using-tokens-verifying-a-jwt.html).
An ID token is refused by an access-only instance. Install policies on separate
operation sets using the runtime's scoping extension:

```rust
use crucible_server_runtime_rust::OperationMiddlewareExt;

builder
    .operation_middleware(id_chain.except(&["MachineRead"]))
    .operation_middleware(access_chain.only(&["MachineRead"]));
```

## Other issuers and verification policy

`IssuerConfig::new(issuer, jwks_url, allowed_audiences)` accepts an explicit
HTTPS JWKS URL and uses audience validation. Discovery is deliberately omitted:
the issuer and key endpoint are deployment configuration. No URL supplied in a
token selects the trust store. Redirects, URL credentials, fragments, and query
strings are refused. Each authenticator owns its own issuer cache.

| Setting | Default and behavior |
| --- | --- |
| `algorithms` | RS256 and ES256 for a generic issuer; RS256 for Cognito. HMAC and `none` are unsupported. |
| `token_rules` | One complete rule must match its client claim, string claim requirements, and optional JOSE `typ`. Distinct token kinds require separate instances. |
| `leeway` | 30 seconds for `exp` and `nbf`. `exp`, `iss`, and a nonempty `sub` are required. Numeric dates must be unsigned integers. |
| `cache_ttl` | One hour. Public keys and verification configuration are prepared before use. |
| `min_refresh_interval` | 30 seconds between fetch attempts, including failures and unknown `kid` misses. Concurrent refreshes share one fetch. |
| `max_stale` | 24 hours beyond TTL during refresh failures or refresh cooldown. Set to zero to disable outage fallback. Token expiry and claims are always checked. |

A successful refresh replaces the key set, removing retired keys. During an
outage, only a matching cached key within the stale limit can verify a token.
A missing usable key or a store outage produces a sanitized 5xx response.
Malformed, expired, incorrectly scoped, or incorrectly signed credentials
produce 401. Provider error details are internal diagnostic causes; tokens and
credential headers are never logged by this crate.

The key's family, `alg`, `use`, and `key_ops` constrain verification independently
of the token header. Duplicate signing key IDs are refused. Documents are
limited to 1 MiB, tokens to 64 KiB, and HTTP requests to ten seconds.

`JwtAuthenticator::credential_header` changes the credential source from
`Authorization`. The bearer scheme is case-insensitive; bare tokens and
duplicate credential headers are refused. `JwtAuthenticator::clock` supplies
a clock for deterministic expiry tests or host integration.

## Reading identity and supplying API-key stores

Handlers and interceptors read verified identity through the ordinary Context
attribute API:

```rust
use crucible_auth::Principal;
use crucible_server_runtime_rust::WithContext;

let principal = self.context().attribute::<Principal>();
```

`Principal` contains subject, issuer, matched client ID, token kind, deduplicated
scopes from the space-separated `scope` claim, Cognito groups from
`cognito:groups`, and the verified raw claims. Public and anonymous optional
requests have no Principal. Claims remain identity-provider statements;
tenant reach and grants must still be checked by the service.

Implement `Authenticator` for your credential store:

- `supports` selects modeled schemes at service construction.
- `has_credentials` checks the configured source without a store lookup.
- `authenticate` returns `Authenticated(Principal)`, `Refused`, or
  `Unavailable(Error)`. Hash API keys and check revocation in your store.

The trait receives the static `AuthScheme` with API-key header/query placement
and optional scheme prefix. The service controls its source extraction and
storage policy. See the compiled
[API-key example and HTTP tests](../e2e/weather-service/tests/auth.rs).

The chain follows effective model scheme order, then configured authenticator
order for each scheme. A refusal permits another authenticator to succeed;
an unavailable provider stops the chain with a 5xx. Optional auth permits
absence, but refuses credentials that were presented and failed verification.
Scheme matching and operation plans are computed in `prepare`, never from raw
traits on each request.

## Refusals and authorization

`FrameworkError::Unauthenticated` and `FrameworkError::Forbidden` are typed
401/403 errors across all serving protocols. Bearer operations receive
`WWW-Authenticate: Bearer` on a missing credential, `invalid_token` on a chain
refusal, and `insufficient_scope` on a forbidden response, following
[RFC 6750 section 3](https://www.rfc-editor.org/rfc/rfc6750#section-3).
Authentication does not enforce scopes or groups; authorization interceptors
can return `FrameworkError::Forbidden`.

Use `AuthenticatorChain::refusal_factory` to preserve a service's modeled
refusal body:

```rust
let chain = AuthenticatorChain::new(authenticators).refusal_factory(|_| {
    AccessDeniedException { message: "access denied".into() }.into()
});
```

The error must be declared by the operation or service. The chain stores
`FrameworkError::Unauthenticated` as a typed Context attribute before calling
the factory, so extensions and alternative protocols can recognize a substituted
refusal without matching a model error name. Missing and incorrect credentials
can share an identical modeled body while retaining the bearer challenge.

## Test tokens

Enable `test-support` in dev-dependencies. `TestTokenIssuer::new` generates an
RSA key pair; `TestTokenIssuer::es256` generates a P-256 key pair. `jwks()` exposes
only public material. Inject it with `JwtAuthenticator::with_jwks`, or serve it
with a test HTTP server to exercise fetching and rotation.

`with_jwks` pins the configured keys for the authenticator's lifetime. It never
fetches `jwks_url`, even for an unknown key ID, and ignores cache TTL and stale
limits. Replace the authenticator to rotate or revoke these keys. Token expiry
and all claim checks still apply. Use `JwtAuthenticator::new` with a reachable
JWKS endpoint when keys should refresh automatically.

`id_claims`, `access_claims`, and `mint` create signed tokens with chosen
claims. `config` and `access_config` supply the corresponding test policies.
The feature allows explicitly opted-in HTTP loopback endpoints; production
configuration always requires HTTPS. This gives service tests real verified
identity without request-extension identity bypasses.

## Migrating Leyline and Pigeon

For Leyline, replace the Cognito JWT verifier with an ID-token authenticator
and preserve its app-client allowlist. Configure
`credential_header(HeaderName::from_static("x-leyline-authorization"))`:
CloudFront OAC uses `Authorization` for SigV4. Keep API-key caching,
revocation, replica health, last-use writes, and rate limits in a custom
Authenticator. Map the verified subject and key metadata into Leyline's Caller
and Charge attributes in an authorization/identity interceptor. Preserve its
modeled AccessDeniedException through the refusal factory. Replace harness
identity extensions with signed test tokens.

For Pigeon, replace the process-global once-only JWKS cache with an ID-token
authenticator. Keep DynamoDB hashing, revocation, workspace binding, and key
IDs in its custom Authenticator. Send `Authorization: Bearer TOKEN` explicitly:
Pigeon's current verifier accepts bare tokens, while this implementation requires
the scheme. Select a separately scoped access-token authenticator only for
operations deliberately accepting access tokens. Map `Principal` into Pigeon's
domain principal, and continue checking workspace membership in authorization.

Neither consumer repository is changed by this feature. Adopt the new crate,
regenerate server contracts with effective auth metadata, and update each
service's model and tests together.
