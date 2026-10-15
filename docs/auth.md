# Bearer authentication: JWTs and API keys

Install `crucible-auth` explicitly to verify JWTs and API keys and enforce the
model's authentication settings. `Crucible::defaults()` keeps
authentication off. The separate crate keeps JWT cryptography, JWKS HTTP
fetching, and test issuers out of services that do not use bearer
authentication.

Authentication establishes identity. Services still own the API-key table,
tenant membership for token callers, permissions, and authorization;
[`ApiKeyAuthenticator`](#api-keys) owns key recognition, hashing and caching.

## Configuring Cognito

Declare `auth bearer` on the service; a `public` operation opts out. Bearer is
the only scheme.

```
service Example {
    version "1"
    protocol json
    auth bearer
}

operation Read { .. }

public operation Health { .. }
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

Add an [`ApiKeyAuthenticator`](#api-keys) to the same chain for bearer API
keys, or register a custom authenticator. Construction fails with collected
operation IDs when the bearer scheme has no configured authenticator. Bindings without explicit auth metadata also
fail at construction; regenerate old bindings before installing the chain.
At a service with [API versions](api-versions.md), the chain prepares a plan
for each version's entry of an operation: a request is authenticated by the
schemes its own version's release declared, not the current version's.

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

Before any key lookup, an authenticator refuses tokens whose unverified `iss`
is missing, malformed, or different from its configured issuer. This refusal
lets other issuers in the chain try without fetching keys or starting a refresh
cooldown. The unverified claim grants no trust: authentication still requires
signature verification and validation of the signed issuer and all other claims.

| Setting | Default and behavior |
| --- | --- |
| `algorithms` | RS256 and ES256 for a generic issuer; RS256 for Cognito. HMAC and `none` are unsupported. |
| `token_rules` | One complete rule must match its client claim, string claim requirements, and optional JOSE `typ`. Distinct token kinds require separate instances. |
| `leeway` | 30 seconds for `exp` and `nbf`. `exp`, `iss`, and a nonempty `sub` are required. Numeric dates must be unsigned integers. |
| `cache_ttl` | One hour. Public keys and verification configuration are prepared before use. |
| `min_refresh_interval` | 30 seconds. After a failed fetch, no fetch is attempted for this long. A `kid` that a successful fetch did not contain causes no new fetch for this long either. A `kid` not looked up yet may cause a fetch one second after the last one (or this interval, if shorter). Concurrent refreshes share one fetch. |
| `max_stale` | 24 hours beyond TTL during refresh failures or refresh cooldown. Set to zero to disable outage fallback. Token expiry and claims are always checked. |

A token's `kid` is unverified until its signature checks out, so a token
with a forged `kid` can make the authenticator fetch the key set. Each forged
`kid` is remembered as missing and fetches nothing again for
`min_refresh_interval`, and new ones together cause at most about one fetch a
second. A forged `kid` therefore no longer holds the key set fixed for the
whole interval: a key the provider rotates in is picked up at most one second
after the last fetch, not 30. The exception is a `kid` that was looked up
before the provider published it (a token signed with a key not yet in the
JWKS): that lookup found it missing, so it waits the full
`min_refresh_interval` like a forged one. The floor has a cost: forged key IDs
can cause about one fetch a second instead of one per interval, which makes it
more likely that the identity provider throttles the JWKS endpoint, and a
failed fetch (throttled or not) holds every fetch off for the interval.

A successful refresh replaces the key set, removing retired keys. A key in a
fetched set that does not parse (a corrupt entry, or a key type this crate
cannot read) is skipped with a warning in the log, and the other keys stay in
use. `with_jwks` refuses such a key as a configuration error instead. During an
outage, only a matching cached key within the stale limit can verify a token.
A missing usable key or a store outage produces a sanitized 5xx response.
Malformed, expired, incorrectly scoped, or incorrectly signed credentials
produce 401. Provider error details are internal diagnostic causes; tokens and
credential headers are never logged by this crate.

The key's family, `alg`, `use`, and `key_ops` constrain verification independently
of the token header. Duplicate signing key IDs are refused. Documents are
limited to 1 MiB, tokens to 64 KiB, and HTTP requests to ten seconds.

`JwtAuthenticator::credential_header` changes the credential source from
`Authorization`; `credential_headers([..])` takes an ordered list, and the
first header present is the only one read (an empty list is a configuration
error). The bearer scheme is case-insensitive; bare tokens and
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

`Principal` contains subject, issuer, matched client ID, token kind, Cognito
groups from `cognito:groups`, the verified raw claims, and `tenant`. Public and anonymous
optional requests have no Principal. Claims remain identity-provider statements;
tenant reach and grants must still be checked by the service.

`tenant` is the workspace or organization the credential itself is bound to.
An API key fills it from its stored record. ID and access tokens leave it
`None`, and no token claim ever sets it, so a token caller's tenant is still the
service's membership check. `Principal` is non-exhaustive: an authenticator
outside this crate builds one with
`Principal::new(subject, issuer, token_kind)` and the `with_client_id`,
`with_groups`, `with_claims` and `with_tenant` setters.

`Principal::caller_binding` names the principal as the runtime's
`Idempotency` and `PageTokens` bind a token to its caller: the JSON array
`[issuer, subject, tenant]`. `Principal::caller_binding_of` is that binding
for the request's principal (`None` without one), a caller closure as it
stands. A service whose callers are scoped by more than these, where the
operation's input does not carry it, binds its own array
([framework.md](framework.md#binding-a-token-to-its-caller)).

For a credential other than a bearer API key, implement `Authenticator`:

- `supports` selects the modeled scheme (`AuthScheme::Bearer`) at service construction.
- `has_credentials` checks the configured source without a store lookup.
- `authenticate` returns `Authenticated(Principal)`, `Refused`, or
  `Unavailable(Error)`.

The service controls its source extraction and storage policy. See the compiled
[API-key example and HTTP tests](../e2e/weather-service/tests/auth.rs).

The chain follows configured authenticator order. A refusal permits another authenticator to succeed;
an unavailable provider stops the chain with a 5xx. The chain wraps that
failure in the runtime's `OpaqueError`, so `ExceptionTranslator` rules never
look into its cause: a rule that turns the service's database-client errors
into a modelled 4xx for its operations cannot turn a key store outage on the
same client into one. A rule may still match `OpaqueError` itself (to answer
every such outage with a modelled 503, say). Operation plans are computed in
`prepare`, never from raw traits on each request.

## API keys

`ApiKeyAuthenticator` checks bearer API keys against a table the service owns,
and does the caching every such table needs. It joins an `AuthenticatorChain`
beside a `JwtAuthenticator`, in either order: it claims only credentials with
its prefix, so a JWT falls through to the JWT verifier without a store read.

It serves the model's bearer scheme: keys are sent as
`Authorization: Bearer KEY` or in the configured headers.

```rust
use std::sync::Arc;
use crucible_auth::{
    ApiKeyAuthenticator, ApiKeyRecord, ApiKeyState, ApiKeyStore, AuthenticatorChain,
};

struct Keys { /* a DynamoDB client, a table name */ }

#[crucible_server_runtime_rust::async_trait]
impl ApiKeyStore for Keys {
    async fn lookup(&self, digest: &str)
        -> Result<Option<ApiKeyRecord>, crucible_server_runtime_rust::Error>
    {
        let Some(row) = self.get(digest).await? else { return Ok(None) };
        let state = if row.status == "active" { ApiKeyState::Active } else { ApiKeyState::Revoked };
        Ok(Some(ApiKeyRecord::new(row.key_id, row.workspace_id, state)
            .with_attributes(serde_json::json!({ "name": row.name }))))
    }
}

let keys = Arc::new(ApiKeyAuthenticator::new("pgn_", Keys { /* .. */ })?);
let chain = AuthenticatorChain::new(vec![keys.clone(), jwt]);
```

### The store

`ApiKeyStore::lookup(digest)` reads one row by digest and maps it to an
`ApiKeyRecord`: the key ID (`Principal::subject`), the tenant
(`Principal::tenant`), an `ApiKeyState` mapped from the store's own status
field, an optional expiry (`SystemTime`), and free-form `attributes`
(`Principal::claims`: a display name, rate limits). The principal's token kind
is `TokenKind::ApiKey` and its issuer is `issuer(..)` (default `"api-key"`);
it has no client ID or groups. Row mapping stays in the store.

- `Ok(None)`: no such key. Refused (401).
- `Err(..)`: the store could not answer. A sanitized 5xx, never a refusal, and
  nothing is cached. Keep the digest out of the error: it reaches the log.
- A record admits a request only when its state is `Active` and, if it has an
  expiry, the expiry is still in the future (fail closed).
- An `Active` record with an empty or blank key ID or tenant is a mapping fault
  in the store, not a key: it is a sanitized 5xx with the cause in the log, and
  nothing is cached. A principal never carries an empty tenant.

A read that takes longer than `lookup_timeout` (5 s, counting the wait for a
read slot) is an outage too, and its future is dropped: `lookup` must be safe
to cancel, which a plain read is.

`api_key_digest(credential)` is the lookup key: lowercase hex SHA-256 of the
whole credential, prefix included. Call it when minting a key and store the
digest, never the key, so minting and checking cannot disagree. A fast hash
suffices because a key is random data, not a password.

### Recognition and credential source

Before any read, a credential must start with the prefix, have something after
it, contain no whitespace, be at most 1 KiB, and pass the optional
`well_formed(|credential| ..)` check (given the whole credential, prefix
included: Leyline's Crockford base32 payload and CRC-32 checksum).
`has_credentials` is true only for a credential that passes, so anything else
falls through to the next authenticator.

Mint keys with a checksum and check it in `well_formed`. Without one, every
made-up `PREFIX_<random>` costs a store read: refusal backoff only helps
against a key that is repeated, and a flood of distinct forged keys fills the
`lookup_concurrency` read slots, so a valid key that misses the cache waits
for a slot and, past `lookup_timeout`, gets a 5xx. A checksum turns such a
flood away before any read.

The credential comes from `Authorization: Bearer KEY` (scheme
case-insensitive) by default. `credential_headers([..])` sets an ordered list
of headers; the first one present is the only one read (an empty list is a
configuration error). `allow_bare(true)` also accepts a key with no scheme
(off by default). A duplicated header that holds a recognizable key is
refused, never treated as absent.

### Caching and the revocation window

| Setting | Default and behavior |
| --- | --- |
| `valid_ttl` | 5 minutes, from the read: a hit does not extend it. Expiry is re-checked on every hit. Zero turns the valid-key cache off and nothing else. |
| `valid_capacity` | 10,000 valid keys, least recently used evicted. |
| `refresh_ahead` | The last fifth of the TTL. A hit in it is answered from cache and starts one background re-read (one per digest) that refreshes the entry, or evicts it if the key is gone or no longer valid. Zero disables it. |
| `refusal_backoff(initial, ceiling)` | 2 s and 60 s. An unknown digest is refused without a read for 2 s, then 4, 8, 16, 32 and 60 s while it stays unknown. A revoked, disabled or expired row goes straight to 60 s. Both zero turn refusal caching off; otherwise `initial` must be nonzero and at most `ceiling`, or the setter returns a configuration error. |
| `refused_capacity` | 20,000 refused digests, in their own LRU, so a flood of forged keys never evicts a valid one. |
| `lookup_concurrency` | 32 store reads at once for cache misses. Concurrent misses for one digest share one read (single flight) regardless. Refresh-ahead reads have their own slots on top, a quarter as many (at least one), so misses (a flood of forged keys among them) cannot stop cached valid keys from refreshing. |
| `lookup_timeout` | 5 s for one store read, including its wait for a read slot. Past it the read is an outage (a 5xx, nothing cached) and gives its slot back, so a hung store cannot hold every slot or a request. Zero is a configuration error. |

The refusal ceiling stays below the valid TTL by default: a correct answer is
cached longer than an incorrect one. A valid read clears a digest's refusal,
and a refusal clears its valid entry. A store outage caches nothing, and a
valid entry keeps serving through one until its TTL runs out.

Refusals are cached per instance, like valid keys. A key that was revoked,
disabled or expired and is then made `Active` again (or a key minted with a
digest an instance recently refused) stays refused on each instance that saw
it refused, for up to the refusal ceiling (60 s by default). `forget` on the
reactivation path clears it at once, on the calling instance only.

An answer is as old as the moment its read started, not when it arrived. A
cached entry's TTL runs from that moment, and an answer already older than
`valid_ttl` when it arrives (a slow store, or a refresh-ahead read paused by a
Lambda freeze that a later request joined) is not served or cached: the read is
repeated once, and a second such answer is an outage.

A revocation is seen at once on the instance that calls `forget(digest)` (or
`forget_credential(key)`) from its revoke or delete path: that removes the
digest from both caches, and a read already in flight does not cache its
result. Other instances see it within `valid_ttl`. Two things make that hold:

- Call `forget` after the store write that revokes or deletes the key has
  committed, not before.
- Make `ApiKeyStore::lookup` strongly consistent (DynamoDB `GetItem` with
  `ConsistentRead`). An eventually consistent read just after the revocation
  can still return the key as active; it would be cached for `valid_ttl` (or,
  with `valid_ttl` zero, admit that one request).

The in-flight guard is one generation for the whole authenticator, not per
digest: any `forget` stops every read then in flight from caching its answer.
That costs at most one extra read per affected key and keeps no per-digest
state.

On Lambda, the process is frozen between invocations, so a refresh-ahead read
may only finish during a later invocation. The TTL still bounds staleness: an
entry past its TTL is never served, refresh or no refresh.

#### Immediate revocation

When revocation must take effect everywhere at once, turn off the valid-key
cache and keep the rest:

```rust
let keys = ApiKeyAuthenticator::new("pgn_", store)?.valid_ttl(Duration::ZERO);
```

Every request with a valid key reads the store, so with a strongly consistent
lookup a revoked key is refused on its next use on any instance (a request
whose read was already in flight when the revocation committed may still be
admitted). Refusal backoff, single flight and the lookup
limit stay on: a repeated forged key still costs one read per backoff window,
and a burst of concurrent requests with one key costs one read. Setting
`refusal_backoff(Duration::ZERO, Duration::ZERO)` as well caches nothing.

### Hooks

- `fence(|| ..)`: while it returns true, every key gets a sanitized 5xx
  (`AuthenticateResult::Unavailable`) with no read and no cache hit (Leyline's
  degraded credential replica). It is an outage, not a verdict on the key, so
  a valid key is never answered `invalid_token`. JWTs in the same chain are
  unaffected.
- `last_used(recorder)`: an `ApiKeyUsageRecorder` receives the digest and record
  of an authenticated key at most once per key per `last_used_resolution`
  (one hour; zero records every use). The throttle is marked before the call,
  so a failed write is not retried within the window. The call is awaited
  inline and returns nothing: it cannot fail the request. It is awaited for
  at most `last_used_timeout` (1 s; zero is a configuration error): a call
  still running then is dropped, so it must be safe to cancel, and counts as
  a failed write.
- `clock(..)` injects time for cache ages and expiry, for deterministic tests.

The authenticator never logs a credential or digest and never puts one in an
error or response.

### Testing

With `test-support`, `test_support::InMemoryApiKeyStore` is a cloneable
in-memory store: `insert(credential, record)` stores by digest and returns it,
`revoke`, `remove`, `set_unavailable(true)` simulates an outage, and `lookups()`
counts reads. See the
[HTTP test that chains it with a JWT authenticator](../e2e/weather-service/tests/auth_api_keys.rs).

## Refusals and authorization

`FrameworkError::Unauthenticated` and `FrameworkError::Forbidden` are typed
401/403 errors across all serving protocols. Bearer operations receive
`WWW-Authenticate: Bearer` on a missing credential and `invalid_token` on a
chain refusal, following
[RFC 6750 section 3](https://www.rfc-editor.org/rfc/rfc6750#section-3).
Authentication does not enforce groups; authorization interceptors can return
`FrameworkError::Forbidden`, which sends no challenge. Crucible has no OAuth
scopes: the model cannot declare them and no interceptor checks them. Permissions on resources (`requires
mailSend on Inbox`) are a separate gate, enforced by the runtime's
`Permissions` interceptor through a service `Authorizer` against generated
descriptions ([framework.md](framework.md#typed-permissions)); a permission is
never an OAuth scope, and a permission refusal sends no challenge.

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
`credential_headers([x-leyline-authorization, AUTHORIZATION])`:
CloudFront OAC overwrites `Authorization` with its own signature, so the router
moves the caller's token to `x-leyline-authorization`, and local calls still use
`Authorization`. Replace its API-key
authenticator with an `ApiKeyAuthenticator` over its DynamoDB table:

| Leyline feature | `ApiKeyAuthenticator` |
| --- | --- |
| `ley_` keys, Crockford base32 payload, CRC-32 checksum | `new("ley_", store)` and `well_formed(..)` with Leyline's validator |
| `x-leyline-authorization` before `Authorization`; bare keys | `credential_headers([x-leyline-authorization, AUTHORIZATION])`, `allow_bare(true)` |
| Key cache and revocation | `valid_ttl`, `refusal_backoff`, `forget` on its revoke path |
| `CREDENTIAL_REPLICA_DEGRADED` | `fence(..)` over its replica health. Fenced keys get a sanitized 5xx, where Leyline answers 403 today |
| Last-use writes | `last_used(..)`, `last_used_resolution(..)` |
| Display name, rate limits | the record's `attributes`, read back from `Principal::claims` |
| Organization | the record's tenant, `Principal::tenant` |

Rate limiting itself stays in Leyline. Its store's lookup should read with
`ConsistentRead` and its revoke path call `forget` after the write commits. Map the verified subject, tenant and key
attributes into Leyline's Caller and Charge attributes in an
authorization/identity interceptor. Preserve its modeled AccessDeniedException
through the refusal factory. Replace harness identity extensions with signed
test tokens and `InMemoryApiKeyStore`.

For Pigeon, replace the process-global once-only JWKS cache with an ID-token
authenticator, and its custom `ApiKeys` authenticator with
`ApiKeyAuthenticator::new("pgn_", store)`, where the store reads its DynamoDB
table by `api_key_digest` and maps the row's status, workspace and key ID into
an `ApiKeyRecord` (with `ConsistentRead`). Mint keys with `api_key_digest`
too. Pigeon states a revocation bound rather than promising immediate
revocation: a revoked key stops working within a minute (its requirement
SEC-13). It therefore sets `valid_ttl(Duration::from_secs(60))`, below the
5-minute default, and its delete path calls `forget` once the delete commits,
so the instance that revoked a key refuses it at once. This is the same
trade it makes with its 30-second permission-decision cache
([typed permissions](permissions.md#71-the-authorizer)). A service that must
apply revocation immediately uses the
[immediate revocation](#immediate-revocation) configuration
(`valid_ttl(Duration::ZERO)`) instead. Send `Authorization: Bearer TOKEN`
explicitly: Pigeon's old verifier accepted bare tokens, while both
authenticators require the scheme by default. Select a separately scoped
access-token authenticator only for operations deliberately accepting access
tokens. Map `Principal` into Pigeon's domain principal: an API key's workspace
is `Principal::tenant`; a token caller's workspace membership is still checked
in authorization.

This feature changed neither consumer repository. Pigeon has since adopted it
as described above; Leyline has not yet. To adopt, take the new crate,
regenerate server contracts with effective auth metadata, and update each
service's model and tests together.
