# Crucible Explorer

Crucible Explorer serves an API reference and try-it panel from your service for
local development. It lists the complete modeled API, including internal
operations by default, and sends calls through the service's normal HTTP routes.

## Enable the Explorer

Enable `explorer` in the service's runtime dependency. The feature is off by
default; keep the runtime and build dependency pinned to the same revision.

```toml
[dependencies]
crucible-server-runtime-rust = { version = "=0.6.1", features = ["explorer"] }

[build-dependencies]
crucible-build = "=0.6.1"
```

Generate the embedded description in `build.rs`:

```rust
fn main() {
    crucible_build::model("weather_model")
        .server()
        .explorer()
        .generate();
}
```

Register the endpoint while constructing the service:

```rust
use crucible_server_runtime_rust::{Crucible, explorer::Explorer};

let mut builder = Crucible::builder(weather::operations(handlers)).defaults();
if cfg!(debug_assertions) {
    builder = builder.explorer(Explorer::new(weather::DESCRIPTION));
}
let service = builder.build()?;
```

Open `/explorer/` on the running service. `/explorer` redirects to the trailing
slash so sibling assets resolve correctly. `crucible new` includes this debug
condition in the service template. A developer can deliberately enable it in a
release build; the framework permits that and emits a startup warning through
`tracing`. Install a subscriber to see warnings.

## Configure access and presentation

| Method | Behavior |
|---|---|
| `Explorer::new(DESCRIPTION)` | Serve the generated static description at `/explorer` with loopback admission |
| `.path("/dev/explorer")` | Change the owned path prefix; modeled route overlaps fail service construction |
| `.title("Weather development")` | Change the browser title and reference title |
| `.allow_remote()` | Permit remote and unknown peers; emit a startup warning |
| `.endpoint_guard(guard)` | Replace loopback admission with a custom endpoint guard |
| `.read_only(true)` | Disable browser calls for operations without `@readonly` |

Loopback admission uses the trusted socket peer, including IPv4 and IPv6. The
native HTTP host supplies it automatically. For an Axum-hosted router, use
`into_make_service_with_connect_info::<std::net::SocketAddr>()`. A request with
no known peer is denied, including Lambda HTTP events. Forwarded headers and
Lambda's reported source IP do not establish a trusted socket peer.

The guard runs on every request under the prefix, before transport middleware
or body parsing. It protects the page, assets, descriptions, redirects and
unknown paths. It receives `Parts` and returns `EndpointAdmission::Admit` or
`EndpointAdmission::Respond(response)` from
`crucible_server_runtime_rust::endpoint`. A synchronous closure is a guard;
implement `EndpointGuard` with `Hook::wait` for an asynchronous check.

`crucible_auth::EndpointAuthenticator::new(authenticator, scheme)` adapts any
`Authenticator` to this interface. Construction rejects unsupported schemes.
Missing or invalid credentials receive 401, with a bearer challenge when
applicable; verification outages receive 503 without private error details.
The adapter authenticates access to the endpoint. Normal operation middleware
continues to authenticate and authorize each API call separately.

```rust
use crucible_auth::EndpointAuthenticator;
use crucible_server_runtime_rust::operation::AuthScheme;

let guard = EndpointAuthenticator::new(authenticator, AuthScheme::Bearer)?;
let explorer = Explorer::new(weather::DESCRIPTION).endpoint_guard(guard);
```

For browser navigation, a guard generally needs an existing browser session or
another credential source the browser sends while loading assets. A bearer
token entered into the try-it panel authorizes API calls; it is not attached to
ordinary browser navigation or script requests.

## Call operations

Choose an operation and edit its schema-derived form or raw JSON. Inputs use
wire member names; blobs use base64. Available `@examples` prefill the form,
including path, query, header and payload members. Nested objects, maps, arrays,
unions and documents are edited as JSON fields. The server remains authoritative
about validation, constraints, defaults and required inputs.

Bearer tokens and modeled header/query API keys are saved only in
`sessionStorage`, scoped to the service and browser tab. **Clear credentials**
removes them. **Copy as curl** copies the URL, method, headers and body, including
credentials you entered. Blob payload commands refer to `payload.bin`.

**Send request** uses same-origin fetch relative to the page's service base,
preserving router mounts and API Gateway stage prefixes. The request builder
reads the bindings, protocol headers, wire names, timestamp formats and payload
metadata already resolved in `describe.json`. Calls pass through the ordinary
authentication, validation, middleware, interceptors and handlers. The response
viewer shows status, headers, body and the modeled error name when identifiable.
Paginated operations offer **Next page** when a successful response contains a
nonempty output token.

Crucible JSON, AWS REST JSON, AWS JSON 1.0/1.1 and Smithy RPC v2 JSON support
buffered calls. CBOR, XML-family and streaming operations retain documentation
and curl guidance, without a browser send button. Browser limitations such as
GET bodies, dot-only path labels and unsafe JavaScript integer precision require
curl or a native client. Explorer does not perform SigV4 signing or HTTP
challenge authentication. Browser-managed and forbidden headers follow Fetch's
rules.

`read_only(true)` changes the Explorer UI; it does not alter authorization or
disable write operations on the service's normal routes.

## Build and serving contract

`.server().explorer()` emits a static `DESCRIPTION` using
`crucible_codegen_common::describe`, with Internal audience by default.
`.audience(crucible_build::Audience::External)` projects only this description;
ordinary server registration keeps every operation. Audience selection works
before or after `.explorer()`.

The endpoint implements `Protocol`, owns its prefix and opts out of fallback
error encoding. It works with the native HTTP host, Axum integration and Lambda
HTTP events. It never claims direct Lambda invocations. At preparation, it
detects modeled route overlaps, including label and greedy-label matches, and
prepares title/read-only configuration once.

The runtime embeds the docs renderer's original `explorer.js` and `explorer.css`
and adds try-it through `registerOperationPanel`; it does not fork the renderer.
The build-generated `describe.json` and `describe.js` are served without
per-request model parsing or serialization. Code and style assets use `private, no-cache` headers so browsers revalidate
after a rebuild. The page, description and configuration use `no-store`
to expose rebuilds immediately.

Every endpoint response sets `X-Content-Type-Options: nosniff`,
`Referrer-Policy: no-referrer`, and this Content Security Policy:

```text
default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'
```

See [service API references](docs.md) for the shared describe/1 contract and
[the framework pipeline](framework.md#the-request-pipeline) for ordinary calls.

Explorer responses use a same-origin resource policy and do not inherit the
modeled API CORS policy. Ordinary API calls retain their modeled CORS behavior.
