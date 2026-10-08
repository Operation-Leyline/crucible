# Serving MCP from a Crucible service

Expose the service's public operations as Model Context Protocol tools on a URL.
Calls use the same middleware, validation, interceptors, handlers, and exception
translation as other Crucible protocols. MCP is disabled until you enable it.

## Enabling the endpoint

Generate descriptors in the service's build script:

```rust
fn main() {
    crucible_build::rust_service_from_smithy("smithy").mcp().generate();
}
```

For a model dependency, use `crucible_build::rust_service("weather_model").mcp()`.
The generated service module exports `MCP_TOOLS` and `MCP_EXCLUSIONS`. Generation
prints each streaming exclusion as a Cargo build warning.

Register MCP on the runtime builder. This example uses the Weather service;
substitute your service module and implementation:

```rust
use crucible_server_runtime_rust::{Crucible, mcp::McpProtocol};

let service = Crucible::builder(weather::operations(implementation))
    .defaults()
    .mcp(McpProtocol::new(weather::MCP_TOOLS)
        .server_info("Weather", "1.0")
        .allowed_origins(["https://agent.example.com"]))
    .build()?;
```

`Builder::mcp` registers MCP before the ordinary wire protocols. The default path
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
`McpProtocol::endpoint_guard` to authenticate the endpoint itself: initialization,
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
use crucible_server_runtime_rust::mcp::EndpointAdmission;

let protocol = McpProtocol::new(weather::MCP_TOOLS)
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

This feature does not publish OAuth Protected Resource Metadata, add discovery
challenges, or implement an authorization server. Applications supply credential
verification and client credential configuration.

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
exactly or receives HTTP 403 before parsing. Service `@cors` settings, including
`*`, do not authorize MCP origins. `.allow_any_origin()` is a separate explicit
opt-in. Authentication still applies to requests without Origin.

POST requires `Content-Type: application/json`. GET and DELETE return HTTP 405.
With service `@cors`, allowed OPTIONS preflights use the existing CORS policy,
augmented with POST and `MCP-Protocol-Version`. Endpoint guards also run for
preflights; a browser deployment's guard must admit valid OPTIONS requests
without requiring the credentials that browsers send only on the later POST. JSON-RPC batches return `-32600`. Service body limits
apply to the entire JSON-RPC envelope; operation-specific limits also apply
after tool identification. There is no SSE, resumability, session state,
server-to-client request, resource/prompt capability, or asynchronous task API.
Streaming blobs and event streams stay on the ordinary HTTP API and receive
collected `mcp.streaming_excluded` build notes. Tool names exceeding 128
characters receive collected `mcp.invalid_tool_name` diagnostics.

`ToolDescriptor` and `ToolRegistry` are reusable independently of HTTP. A later
stdio transport can resolve a tool to its operation, prepare arguments, invoke
the normal service pipeline, and project/validate its output with this API.
This feature does not add a generated CLI subcommand.
