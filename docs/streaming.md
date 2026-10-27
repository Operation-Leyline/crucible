# Response streams

`output stream Event` returns a typed sequence through the usual authentication,
input validation, permissions, exception rules, and middleware. The model
records a sequence independently of its transport. The first transport is JSON
over SSE, consumed through Fetch.

## Model and server

```cru
public operation Watch {
    http GET "/events"
    output stream Progress
}

union Progress {
    update: Update
    ready: string
}

structure Update {
    text: string
}
```

List `Watch` in the service's operations as usual. POST can combine a normal
`input { ... }` with a streamed output. The item root must currently be a named
structure or union. Error declarations describe terminal failures and cannot
be item roots. Request streams, pagination on a stream, HEAD, and
success statuses 204/205 are diagnosed. Application event kinds belong in a
union; transport controls stay out of the model.

The generated Rust operation trait returns `Result<EventStream<Progress>>`.
Use `EventStream::from_try_stream(source)`, where `source` implements
`futures_core::Stream<Item = Result<Progress>> + Send + 'static`; `Sync` is
unnecessary. Return an error before opening for an ordinary HTTP failure;
yield an error after opening for a terminal stream failure. Generated item
validators run before each item is disclosed.

Discharge handler permission obligations before returning `EventStream`.
Opening checks run before the lazy producer is polled; deferring them until
completion could disclose unauthorized items. Per-item authorization belongs
in service-owned checks inside the producer.

Crucible pulls values as the body is consumed, without collecting the sequence
or creating an unbounded queue. A service bridging a push producer must choose
its own bounded channel and overflow policy. Dropping the response drops its
source, even if never consumed. Detached tasks must arrange their own
cancellation through ownership or channel closure.

## TypeScript client

```ts
const events = await client.watch({ signal: abortController.signal });
try {
  for await (const event of events) {
    if ("update" in event) render(event.update.text);
  }
} catch (error) {
  // Generated modeled errors can be thrown after items have arrived.
}
```

Opening returns `ResponseStream<Progress>` with HTTP `metadata`; completion is
established during iteration. `break`, `events.cancel()`, abort, deadlines,
and errors release the reader. Each stream has one consumer and refuses
concurrent reads. When consuming manually or storing an opened stream, use
`try { /* consume */ } finally { await events.cancel(); }` to cover abandoning
it before iteration.

Fetch supports bearer credentials, POST inputs, and normal request middleware.
Eligible opening failures use the existing retry policy. After successful
opening, Crucible never reconnects or replays the operation. Resume cursors,
replay storage, acknowledgments, and exactly-once business effects remain
service contracts.

`timeoutMs` covers credentials, opening, and the entire body. Each event has a
default 1 MiB limit; override `maxStreamEventBytes` on the client or call
(minimum 256 bytes). Malformed framing or values and truncation throw
`StreamProtocolError`. Unknown application union variants use the normal
generated union representation.

## Rust client

```rust,ignore
let mut events = client.watch().await?;
println!("HTTP {}", events.metadata().status);
while let Some(event) = events.next().await {
    let event = event?;
    // Handle the generated item or its preserved unknown union variant.
}
```

Generated streaming methods return `ResponseStream<Item, OperationError>`.
It implements `futures_core::Stream` and has an inherent async `next`, so a
consumer can iterate without importing an extension trait. Late failures use
the same generated operation error enum as opening failures. Their fields are
decoded as document values, including members normally bound to headers or a
payload.

Drop the owned stream or call its synchronous, idempotent `cancel()` to stop.
Breaking a loop while retaining the stream keeps it alive until cancellation
or drop. `CallOptions::cancellation` and the total client/call timeout remain
active even before the first poll. They also take precedence over buffered
items. `Config::max_stream_event_bytes` changes the default 1 MiB frame limit;
values below 256 are refused before opening. The same limit bounds buffered
opening-error bodies.

The client reads on consumer demand through bounded channels. It never
reconnects after successful headers, including malformed profiles or a later
connection failure. Success requires `crucible.complete` and body EOF; extra
bytes after a terminal are a decoding failure.

## Wire contract

Successful responses use `Content-Type: text/event-stream; charset=utf-8;
crucible-stream=1` and `Cache-Control: no-store`. The media parameter identifies
profile 1 without requiring a browser to expose a custom response header.
Normal service CORS configuration still applies.

| SSE event | JSON data | Meaning |
|---|---|---|
| `message` (also the default) | One item in normal Crucible JSON | Application value |
| `crucible.complete` | `{}` | Successful termination |
| `crucible.error` | `{"type":"WorkFailed","status":503,"error":{"message":"step failed"}}` | Terminal failure |

Exactly one terminal event ends the sequence, followed by body EOF. EOF without
a terminal is interruption. Duplicate terminals, data after completion, and
unknown controls fail explicitly. Comments are heartbeats. `id` and `retry`
do not enable reconnection. Standard [SSE framing](https://html.spec.whatwg.org/multipage/server-sent-events.html)
supports UTF-8, LF/CRLF/CR, multiline data, and arbitrary chunk boundaries.

Declared errors retain their generated identity and fields. Unknown failures
and producer panics become sanitized internal failures. A late error cannot
change the opening HTTP status; its envelope carries the modeled failure
status. Error fields are document values: header and payload bindings do not
move fields out of the terminal envelope. Normal output logging never collects
the event sequence.

## Lifetime and hosting

`Builder::stream_options(StreamOptions { ... })` controls server event size
(default 1 MiB), heartbeat interval (15 seconds), and application idle timeout
(disabled). Intervals must be positive. The total request deadline remains
active after opening; heartbeats reset neither it nor application idle time.

Existing `before`/`after` and `complete` hooks observe opening.
`stream_complete` observes termination or drop exactly once, with
`StreamCompletion`: `Completed`, `Failed`, or `Cancelled`, item count, and byte
count. It runs synchronously under the request context and tracing scope and
cannot rewrite headers or await during drop. Counts describe production by
Crucible, not receipt by the peer. Metrics and service logs record terminal
outcome and duration; typed and async convenience adapters forward this hook.

Custom protocols opt in with `supports_server_stream`, open their response
using `Job::stream_body`, and encode items, terminals, and optional heartbeats
through the `Protocol` hooks. They share the runtime's ownership and lifetime
handling; a protocol does not take over authentication or operation dispatch.

HTTP streams directly. For Lambda select `Host::LambdaStreaming`,
`host::lambda_streaming(service)`, or set
`CRUCIBLE_LAMBDA_RESPONSE_MODE=streaming` with `host::run`. Also configure the
AWS integration for streaming. This uses the AWS Rust runtime's response
encoder and retains Crucible's hardened HTTP event normalization. Function
URLs and API Gateway REST integrations are supported; HTTP API, ALB, and
direct-invoke envelopes are refused. Buffered Lambda refuses streaming
operations before calling the handler. Local
`host::lambda_streaming_event_json_with` tests do not establish deployed AWS
delivery or proxy flush behavior.

Configure proxy buffering and integration timeouts for progressive delivery.
Middleware that collects an entire body defeats streaming; opening middleware
cannot consume the deferred source before the pipeline finishes. The standard
idempotency interceptor refuses tokened streams because its replay store holds
single responses. Keep transactional business deduplication in the service.

## Current support

Rust servers, Rust clients and TypeScript web/Node clients implement profile 1. Other clients
diagnose `server-streaming` instead of emitting a unary API. OpenAPI includes
SSE and `x-crucible-stream` item/control metadata. Static references, explorer,
MCP, historical server adapters, and direct invocation do not yet serve
streams; unsupported generators or protocols refuse them. Changing response
cardinality is a breaking change in the compatibility guard.

New operations also appear in older projected releases. Adding a stream to a
versioned service therefore needs historical adapters even when the frozen
release predates that operation; this initial implementation diagnoses it.

[Backlog #389](https://github.com/Operation-Leyline/crucible/issues/389) tracks
client parity and the remaining tooling and host qualification.

`python tools/test-rust-streaming.py` builds an independent generated Cargo SDK consumer over native HTTP and shared profile vectors.

`bash tools/test-ts.sh` covers parser behavior and generated TypeScript-to-Rust
HTTP calls. `python tools/test-consumers.py` covers stable service consumers.

## Browser qualification

The optional browser harness generates and compiles the TypeScript SDK as ESM,
then uses native browser Fetch against the Rust HTTP fixture. It checks actual
cross-origin preflight and exposed request IDs, progressive GET/POST delivery,
typed errors, cancellation, producer drop, total deadlines, and unary calls.
A local raw HTTP fixture checks malformed framing, UTF-8 and JSON, terminal
rules, unknown application variants, byte bounds, and absence of replay.
Neither path substitutes `fetch` or disables browser web security.

Install Node/npm, Rust, Python 3.11 or later, and the optional browser dependency:

```sh
python -m pip install -r tools/requirements-browser.txt
python -m playwright install chromium
python tools/test-streaming-browser.py --report target/browser-streaming.json
```

Use `--channel chrome` or `--channel msedge` to test an already installed
browser in an ephemeral profile. Repeat the flag to test multiple builds;
Chrome and Edge are both Chromium, so this does not establish cross-engine
coverage. The report records each actual browser version and completed checks.
CI runs bundled Chromium when the TypeScript qualification gate is selected.

The harness builds its CLI and Rust fixture by default. `CRUCIBLE_BIN` and
`CRUCIBLE_STREAMING_BIN` can select already built binaries; `TSC` can select a
TypeScript compiler executable instead of the pinned npm invocation. Set a
private `CARGO_TARGET_DIR` and a low `CARGO_BUILD_JOBS` on a shared machine.
The Rust fixture enables its single-origin CORS middleware and observation
endpoint only when `CRUCIBLE_BROWSER_ORIGIN` is set by this harness. The model's
operations are public: the custom-header check proves request middleware
delivery, not modeled authentication. These local checks do not establish
deployed AWS flushing or support in the other generated client languages.
