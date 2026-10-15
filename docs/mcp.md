# Serving MCP from a Crucible service

Expose the service's public operations as Model Context Protocol tools on a URL.
Calls use the same middleware, validation, interceptors, handlers, and exception
translation as other Crucible protocols. MCP is disabled until you enable it.

## Enabling the endpoint

Generate descriptors in the service's build script:

```rust
fn main() {
    crucible_build::model("smithy").server().mcp().generate();
}
```

For a model dependency, use `crucible_build::model("weather_model").server().mcp()`.
The generated `SERVICE.mcp_tools` carries the descriptors, so runtime wiring
does not pass `MCP_TOOLS` by hand. The module also exports `MCP_TOOLS` for inspection.

Register MCP on the runtime builder. This example uses the Weather service;
substitute your service module and implementation:

```rust
use crucible_server_runtime_rust::{Crucible, mcp::McpConfig};

let service = Crucible::builder(weather::operations(implementation))
    .defaults()
    .mcp(McpConfig::new()
        .server_info("Weather", "1.0")
        .allowed_origins(["https://agent.example.com"]))
    .build()?;
```

`Builder::mcp` reads the generated descriptors from its service metadata and
registers MCP before the ordinary wire protocols. Requesting MCP without build
script `.mcp()` returns a service build error. The default path
is `/mcp`; `.path("/agents/mcp")` changes it. The same service router and HTTP
hosts serve it. Configure the public endpoint URL in your client or deployment;
MCP never constructs a resource URL from the request's `Host` header.

## Connecting an agent

Configure the service URL in your agent's MCP settings. For Claude Code:

```sh
claude mcp add --transport http weather https://api.example.com/mcp
```

The endpoint supports `initialize`, `notifications/initialized`, `ping`,
`tools/list`, and `tools/call`. Tool names are the Smithy operation names,
including their case. A call takes a JSON object containing the operation's
whole input, including members ordinarily bound to labels, queries, or headers.
Blobs use base64 and timestamps use the generated model's document encoding.

For a paginated operation, pass the output token into the modeled input token
parameter on the next call. Weather's `ListCities` takes `nextToken` and
`maxResults`. Tool discovery returns the whole list; it has no cursor paging.

## Authentication and endpoint admission

Reuse the service's bearer/API-key middleware for tool calls. Configure
`McpConfig::endpoint_guard` to authenticate the endpoint itself: initialization,
discovery, notifications, and ping have no operation, so operation-aware
middleware cannot protect them. The guard runs before transport middleware and
JSON-RPC parsing, after Origin validation, on every request to the MCP path.

The hook receives HTTP request parts and returns `EndpointAdmission::Admit` or
`EndpointAdmission::Respond(Response)`. Closures support synchronous guards;
implement `EndpointGuard` and return `Hook::wait(...)` for asynchronous checks.
The current `Context` is available during the hook, so a guard can store the
authenticated principal with `Context::current().set_attribute(principal)`.

```rust
use axum::http::{StatusCode, header, request::Parts};
use axum::response::IntoResponse;
use crucible_server_runtime_rust::endpoint::EndpointAdmission;
use crucible_server_runtime_rust::mcp::McpConfig;

let config = McpConfig::new()
    .endpoint_guard(|parts: &Parts| {
        // Replace this fixed comparison with your application's verifier.
        if parts.headers.get("authorization").is_some_and(|v| v == "Bearer good") {
            EndpointAdmission::Admit
        } else {
            EndpointAdmission::Respond((StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Bearer")]).into_response())
        }
    });
```

Middleware can respond directly, or return `AuthRefusal::unauthorized(headers)`
or `AuthRefusal::forbidden(headers)` as a framework error. These refusals retain
HTTP 401/403 and `WWW-Authenticate`; MCP never turns them into tool results.
Modeled operation errors produce `isError: true` with their name and message in
a text block. Invalid arguments produce JSON-RPC `-32602`. Internal failures and
outputs that violate the advertised schema produce `-32603` without output data.

### OAuth discovery and access tokens

Install `crucible_auth::McpAuthenticator` through `McpConfig::authorization` to
derive discovery and authenticate every MCP request, including initialization.
The JWT verifiers supply the trusted issuers; the configured resource supplies
the public HTTPS MCP URL. The URL path must match `McpConfig::path` (default
`/mcp`). Neither `Host` nor forwarded headers select a resource or issuer.

Crucible serves unauthenticated GET/HEAD at
`/.well-known/oauth-protected-resource/mcp`, following
[RFC 9728 section 3.1](https://www.rfc-editor.org/rfc/rfc9728.html#section-3.1).
For `/agents/mcp`, the location is
`/.well-known/oauth-protected-resource/agents/mcp`. Only this path-suffixed
document is registered, so other resources can publish their own metadata.
It contains `resource`, sorted and deduplicated `authorization_servers`,
`scopes_supported` from the external tools' effective model scopes plus the
endpoint's required scope, and `bearer_methods_supported: ["header"]`.
Metadata is prepared once at construction, without reading raw model traits.

An unauthenticated request gets HTTP 401 before body parsing, with:

```http
WWW-Authenticate: Bearer scope="weather/mcp", resource_metadata="https://api.example.com/.well-known/oauth-protected-resource/mcp"
```

Invalid tokens get 401 with `error="invalid_token"`; missing scopes get 403
with `error="insufficient_scope"`. Tool scope refusals retain their operation
scopes and include the same metadata URL. Install `ScopeEnforcer::strict()` to
enforce the model scopes on calls; the endpoint guard always checks its own
required scope and stores the verified `Principal` in `Context`.
Provider outages return 503, rather than labeling credentials invalid.
`endpoint_guard` and `authorization` cannot be combined on one MCP endpoint.

This discovery and challenge behavior follows the fetched
[MCP authorization specification, 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
and its [authorization server discovery requirements](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization/authorization-server-discovery),
also present in [2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization).
The supported JSON-RPC transport versions remain those listed below. Crucible
does not implement an authorization server or client registration; for an
identity provider without either, see
[Identity providers without dynamic client registration](#identity-providers-without-dynamic-client-registration-cognito). Clients
obtain tokens from the advertised issuer and send them in `Authorization` on
each request. Query tokens are ignored; duplicate credential headers are refused.
Tokens must be access tokens, with an allowed `client_id`, and by default their
`aud` must exactly equal the configured resource URL, as a string or a
one-member array (RFC 7519 allows either for one audience). A token for several
audiences is refused: any of them could replay it here.

### Configuring Cognito

Use a Cognito user pool with a domain, permission to create its resource server
and app client, and an MCP client that supports pre-registered OAuth clients.
The following walkthrough uses `https://api.example.com/mcp` as the public MCP
resource and `weather` as this service's unique resource-server identifier.

1. Create a resource server in the user pool, with identifier `weather` and a
   scope named `mcp`. This produces `weather/mcp`; only this service should
   define and accept it. Add scopes for the model's operation policies, such as
   `weather/read`, `weather/write`, and `weather/admin`. See
   [Cognito resource servers and scopes](https://docs.aws.amazon.com/cognito/latest/developerguide/cognito-user-pools-define-resource-servers.html).
2. Create a dedicated app client. Enable authorization code grant and the
   custom scopes it needs, configure the MCP client's exact callback URL, and
   use PKCE with S256. For a public client, create the app client without a
   secret. Pre-register its client ID with the MCP client; Cognito discovery
   does not provide arbitrary dynamic registration for MCP clients.
3. Request `weather/mcp` and the needed model scopes. Include
   `resource=https://api.example.com/mcp` in the authorization request and the
   token request as required by MCP. Cognito now documents resource binding in
   its [authorization endpoint](https://docs.aws.amazon.com/cognito/latest/developerguide/authorization-endpoint.html);
   a bound [access token](https://docs.aws.amazon.com/cognito/latest/developerguide/amazon-cognito-user-pools-using-the-access-token.html)
   carries that resource in `aud`. Verify the issued token has the expected
   `aud` before using the default policy. Send the access token, not the ID token.
4. Configure the service's verifier with the user pool's issuer and the dedicated
   app-client allowlist. Keep it local to the MCP endpoint:

```rust
use crucible_auth::{IssuerConfig, JwtAuthenticator, McpAuthenticator, ScopeEnforcer};
use crucible_server_runtime_rust::{Crucible, mcp::McpConfig};

let verifier = JwtAuthenticator::new(IssuerConfig::cognito_access(
    "us-east-1", "us-east-1_EXAMPLE", vec!["YOUR_MCP_APP_CLIENT_ID".into()],
))?;
let authorization = McpAuthenticator::new(
    "https://api.example.com/mcp", vec![verifier], "weather/mcp",
)?;
let service = Crucible::builder(weather::operations(implementation))
    .defaults()
    .mcp(McpConfig::new().authorization(authorization))
    .interceptor(ScopeEnforcer::strict())
    .build()?;
```

For pools or token flows without resource binding, explicitly opt into
`authorization.allow_unbound_cognito_tokens()`. This compatibility mode accepts
absent `aud` only with all three substitute controls: access-token verification
on the MCP endpoint, an allowed `client_id`, and the required service-owned
resource-server scope. A present `aud` must still be the MCP URL alone. This mode
is not RFC 8707 audience binding and does not claim full MCP authorization
conformance. Prefer bound tokens where the deployment supports them.

The MCP guard does not install an ordinary API authenticator. Configure the
API's ID-token or other policy independently, and exclude MCP calls from an
ID-only `AuthenticatorChain` so it does not replace or refuse the MCP principal.
Never pass an MCP bearer token to an upstream service. Obtain a separate token
for that upstream; discovery and admission do not implement token passthrough.

## Identity providers without dynamic client registration (Cognito)

The MCP authorization spec makes the client choose how it gets a client id:
pre-registered credentials first, then Client ID Metadata Documents if the
authorization server advertises `client_id_metadata_document_supported`, then
Dynamic Client Registration if it advertises a `registration_endpoint`
([Client Registration](https://modelcontextprotocol.io/specification/draft/basic/authorization/client-registration)).
Cognito supports neither CIMD nor DCR, so each client uses the pre-registered
route. Crucible deliberately adds no registration endpoint: every target client
below accepts a configured client id, and a registration endpoint that returned
one shared Cognito client could not give a client a redirect URI Cognito has not
already registered.

One thing does need code. Claude Code and claude.ai refuse an authorization
server whose metadata does not list `S256` in `code_challenge_methods_supported`,
and Cognito's `openid-configuration` is reported to omit it ([AWS re:Post](https://repost.aws/questions/QUWgnfYNLDS6WIHsXxEDyFdg/missing-code-challenge-methods-supported-field-in-amazon-cognito-openid-connect-configuration),
[claude-code#13275](https://github.com/anthropics/claude-code/issues/13275):
`does not support S256 code challenge method required by MCP specification`).
Check your pool, and skip the next step if it now lists `S256`:

```sh
curl -s https://cognito-idp.REGION.amazonaws.com/POOL_ID/.well-known/openid-configuration \
  | jq .code_challenge_methods_supported
```

### Serving RFC 8414 metadata for Cognito

`AuthorizationServerMetadata` is a static RFC 8414 document the service serves
itself. It names the service as the authorization server, advertises `S256`, and
points `authorization_endpoint`, `token_endpoint` and `revocation_endpoint` at
the Cognito user pool domain. Cognito still authenticates users and issues
tokens; nothing is fetched or proxied at request time.

```rust
use crucible_server_runtime_rust::mcp::{AuthorizationServerMetadata, McpConfig};

// `authorization` is the McpAuthenticator from "Configuring Cognito" above.
let config = McpConfig::new()
    .authorization(authorization)
    .authorization_server_metadata(
        // The service's public origin, then the user pool domain (host name only).
        AuthorizationServerMetadata::cognito("https://api.example.com", "auth.example.com"),
    );
```

The document is served on `GET` at `/.well-known/oauth-authorization-server`
followed by the issuer's path, if any (`https://api.example.com/tenant` is served
at `/.well-known/oauth-authorization-server/tenant`). It is public: neither
`allowed_origins` nor `endpoint_guard` applies, because clients read it before
they hold a token. Construction fails if a URL is not `https` or the path
collides with the MCP path. Use `AuthorizationServerMetadata::new` for another
provider with the same gap.

When `McpConfig::authorization` derives protected-resource metadata, a configured
metadata issuer replaces the token issuers in its `authorization_servers`, so
clients discover this document instead of Cognito's. Without `authorization`,
publish protected resource metadata that lists this issuer first (Claude uses
only the first entry). Clients validate that the metadata `issuer` equals the URL
they discovered it from and do not compare it with the `iss` of the token
Cognito issues, which the verifier still checks.

### Cognito app client settings and client setup

[Configuring Cognito](#configuring-cognito) creates the resource server, scopes
and verifier. For the app client these clients use, set:

| Setting | Value |
|---|---|
| Application type / client secret | Public client, no secret. Claude treats a blank secret as a public client; Cursor and VS Code accept a client id alone. Use a secret only if you want a confidential client. |
| OAuth grant types | Authorization code grant only. Do not enable implicit or client credentials. |
| OpenID Connect and custom scopes | The service-owned `weather/mcp` and model scopes from "Configuring Cognito"; add `openid` only if clients need it. |
| Allowed callback URLs | Exactly the URLs below. Cognito matches them exactly, requires HTTPS except `http://localhost`, and has no wildcard or port-agnostic match. |

PKCE needs no setting: Cognito accepts `code_challenge_method=S256` only, which is
what these clients send. Resource binding (`resource`, RFC 8707) needs the
Essentials or Plus feature plan.

| Client | Callback URL to register | Pass the client id with |
|---|---|---|
| claude.ai, Claude Desktop, mobile | `https://claude.ai/api/mcp/auth_callback` | Add custom connector, then Advanced settings: use your own OAuth client, client id, secret blank ([Claude connector authentication](https://claude.com/docs/connectors/building/authentication)). |
| Claude Code | `http://localhost:PORT/callback`, one port you choose | `claude mcp add --transport http --client-id CLIENT_ID --callback-port PORT NAME URL` ([Claude Code MCP](https://code.claude.com/docs/en/mcp)). Claude Code 2.1.229 sent `127.0.0.1`, which Cognito rejects over HTTP; use 2.1.231 or later. |
| Cursor | `http://localhost:8787/callback` (desktop), `https://www.cursor.com/agents/mcp/oauth/callback` (web and cloud agents) | `"auth": {"CLIENT_ID": "...", "scopes": ["..."]}` in the server's `mcp.json` entry ([Cursor MCP](https://cursor.com/docs/context/mcp)). |
| VS Code | Not verified; see below | `"oauth": {"clientId": "..."}` in the server's `mcp.json` entry ([VS Code MCP configuration](https://code.visualstudio.com/docs/agents/reference/mcp-configuration)). |

Claude Code and claude.ai first try CIMD, then DCR, and use the id you supply
when you give one; Cognito advertises neither. A client that cannot fix its
callback to a registered URL (random-port loopback) cannot work with Cognito.
VS Code's documentation does not list its redirect URIs, and it may use
`http://127.0.0.1`, which Cognito cannot register; test it against your pool
before promising VS Code support. Claude connections originate from Anthropic's
published address range, so the service and the Cognito domain must be reachable
from it.
`EndpointGuard` and `EndpointAdmission` live in the shared `endpoint` module;
their original MCP re-exports remain available. Use
`crucible_auth::EndpointAuthenticator::new(authenticator, scheme)` to adapt an
existing authenticator for initialization and discovery as well as tool calls.


## Audience and schemas

Descriptors use Crucible's External audience projection. Internal operations are
unavailable through MCP, and internal members are removed from both schemas and
the JSON passed into or returned from tool calls. Servers still retain their
full ordinary API. External projection diagnostics fail descriptor generation;
there is no Internal audience option for MCP.

JSON Schema emission is shared with OpenAPI. MCP embeds only reachable
definitions under `$defs` instead of OpenAPI's `components/schemas`. Inputs use
Smithy client nullability: `@input` and `@clientOptional` members may be omitted
or null in the description, while the normal server validator remains
authoritative about whether the handler can receive them. Success returns an
object as `structuredContent` and its serialized JSON as a text content block.
Empty outputs become `{}`. Validators compile once at service construction;
every published output is checked against its `outputSchema`.

Annotations describe `@readonly`, `@idempotent`, and idempotency tokens.
`destructiveHint` is true for operations that are not readonly. Agents should
apply their own confirmation policy for writes. MCP does not generate
idempotency tokens: supply the modeled token when replaying a logical write.

## Transport and limits

The implementation follows MCP's [Streamable HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
and [tool contract](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)
with stateless, single-JSON responses. It supports protocol versions
`2025-03-26`, `2025-06-18`, and `2025-11-25`. An absent `MCP-Protocol-Version`
header uses the compatibility version `2025-03-26`; an unsupported header gets
HTTP 400. Initialization negotiates a supported version without storing a
session. Valid notifications return HTTP 202 with an empty body.

Absent Origin is allowed. A present Origin must match `.allowed_origins(...)`
exactly or receives HTTP 403 before parsing. CORS configured for the rest of
the service, including `*`, does not authorize MCP origins. `.allow_any_origin()` is a separate explicit
opt-in. Authentication still applies to requests without Origin.

POST requires `Content-Type: application/json`. GET and DELETE return HTTP 405.
A browser deployment configures CORS outside the model (a Tower layer, for
example) and allows POST and `MCP-Protocol-Version`. Endpoint guards also run for
preflights; a browser deployment's guard must admit valid OPTIONS requests
without requiring the credentials that browsers send only on the later POST. JSON-RPC batches return `-32600`. Service body limits
apply to the entire JSON-RPC envelope; operation-specific limits also apply
after tool identification. There is no SSE, resumability, session state,
server-to-client request, resource/prompt capability, or asynchronous task API.
Tool names exceeding 128
characters receive collected `mcp.invalid_tool_name` diagnostics.

`ToolDescriptor` and `ToolRegistry` are reusable independently of HTTP. A later
stdio transport can resolve a tool to its operation, prepare arguments, invoke
the normal service pipeline, and project/validate its output with this API.
This feature does not add a generated CLI subcommand.
