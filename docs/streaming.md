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

Rust servers and TypeScript web/Node clients implement profile 1. Other clients
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

`bash tools/test-ts.sh` covers parser behavior and generated TypeScript-to-Rust
HTTP calls. `python tools/test-consumers.py` covers stable service consumers.
