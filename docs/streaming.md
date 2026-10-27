# Response streams

`output stream Event` returns a typed sequence through the usual authentication,
input validation, permissions, exception rules, and middleware. The model
records a sequence independently of its transport. The first transport is JSON
over SSE, consumed through native SDK iterators or Fetch.

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

## Python clients

The synchronous method opens a `ResponseStream[Progress]`; the asynchronous
method opens an `AsyncResponseStream[Progress]`. Close either stream even when
abandoning it before iteration:

```python
with client.watch() as events:
    print(events.metadata.status)
    for event in events:
        print(event)
```

```python
async with await async_client.watch() as events:
    async for event in events:
        print(event)
```

Use `close()` or `await aclose()` for explicit cleanup. Pass
`cancellation=threading.Event()` to a synchronous call or
`cancellation=asyncio.Event()` to an asynchronous call. Task cancellation also
interrupts an asynchronous read. The total `timeout` starts at call entry and
covers transport setup, credentials, retries, decoding, and idle time before or
between reads. Expiry and cancellation take precedence over buffered items.

Synchronous streams own a demand-driven async HTTPX transport and worker thread.
When supplying a caller-owned synchronous `http=`, also supply a
`stream_http_factory` that returns a fresh owned `httpx.AsyncClient`. Cleanup
joins the worker and closes that transport; it does not close the caller's
unary pool. A synchronous factory cannot be forcibly interrupted: if its budget
expires, the returned transport is closed before credentials or HTTP begin.
Asynchronous streams close only their response, retaining the caller's pool.

`max_stream_event_bytes` bounds frames and opening-error bodies (default 1 MiB,
minimum 256). Python requests identity encoding and explicitly refuses encoded
success responses; bounded gzip/deflate decoding remains available for opening
errors. Malformed profiles, invalid values, and JSON depth rejection raise
`StreamProtocolError`. Modeled late errors keep their generated exception types
and document-valued fields. Unknown union variants keep the normal `_unknown`
representation. See the [Python client guide](clients/python.md#response-streams).

## Go client

Generated methods return `(*ResponseStream[*Progress], error)`. Keep the caller
context alive through iteration and always defer `Close`, including before the
first `Next`. Within a function returning `error`:

```go
events, err := client.Watch(ctx, sdk.Unit{})
if err != nil {
    return err
}
defer events.Close()
for events.Next() {
    fmt.Println(events.Value())
}
return events.Err()
```

`Metadata` retains opening HTTP metadata. `Next`, `Value`, and `Err` consume one
item at a time; `Close` is idempotent and can interrupt a read from another
goroutine. It releases the response without closing the caller's HTTP client.
Context cancellation and `Config.Timeout` remain active during credentials,
opening, decoding, and idle time between reads, and take precedence over
buffered items. `Config.MaxStreamEventBytes` bounds frames and opening-error
bodies (default 1 MiB, minimum 256).

Malformed profiles and invalid values produce `*StreamProtocolError` through
`Err`. Modeled late errors retain their generated types and `errors.As` support;
header and payload members decode as document fields. Unknown application union
variants keep `UnknownVariant`. See the [Go client guide](clients/go.md#response-streams).

Both Python and Go reuse normal authentication, middleware, version headers, and
eligible opening retries. They never reconnect or replay after successful
headers, and require exactly one terminal followed by body EOF. Each stream has
one consumer. A loop `break` retains the stream until context-manager exit or
explicit closure.

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

### Opt-in deployed AWS qualification

`tools/test-streaming-cloud.py` creates a temporary Lambda function, an
`AWS_IAM` Function URL in `RESPONSE_STREAM` mode, and a Regional REST API with
IAM authorization. Its Lambda proxy integrations use `responseTransferMode:
STREAM` and the `2021-11-15/.../response-streaming-invocations` URI. HTTP APIs
are not a replacement for this REST integration. See AWS's
[Lambda streaming configuration](https://docs.aws.amazon.com/lambda/latest/dg/config-rs-invoke-furls.html)
and [REST streaming integration configuration](https://docs.aws.amazon.com/apigateway/latest/developerguide/response-streaming-lambda-configure.html).

This test supports the commercial `aws` partition and rejects other partitions
before creating resources. Running it requires Docker, `boto3` in your Python
environment, and authorization to create temporary AWS resources. Use an
artifact directory outside the checkout and an explicit profile, region,
account, and principal:

```sh
python tools/build-streaming-cloud.py --artifacts /absolute/path/cloud-smoke
python tools/test-streaming-cloud.py \
  --profile YOUR_PROFILE --region us-east-1 \
  --account YOUR_ACCOUNT_ID --principal arn:aws:iam::YOUR_ACCOUNT_ID:user/YOUR_USER \
  --bootstrap-zip /absolute/path/cloud-smoke/bootstrap.zip \
  --report-dir /absolute/path/cloud-smoke
```

The build uses a pinned official Linux Rust image, one Cargo job, isolated
container-local Cargo caches, and a musl executable with an executable zip
entry. It installs no host toolchains.
The container uses the invoking UID/GID on Linux to preserve artifact ownership;
Windows Docker Desktop uses its host ACL mapping. Build metadata's
`partial_source_sha256` covers only the fixture, lockfile, and host source; it is
a focused identity receipt, not a complete compiler-input manifest.
Older build records used `source_sha256` for the same partial set. Preserve
those records as written and interpret that legacy field as partial too.
The fixture uses the ordinary generated operation traits and
`host::lambda_streaming`; no service-specific behavior is
added to the framework. Its execution role can write only its own log group.
It has no business-data permissions.

Both endpoints test GET and POST, an immediate first item and delayed second
item arriving before the terminal and EOF, zero items, a modeled late error,
profile and request-id headers,
deadline failure, and closing the actual client socket after the first item.
A unary request verifies ordinary output through the same streaming host.
Requests are signed with botocore SigV4. The receipt records timings and
nonce-correlated producer drop and terminal logs without credentials or signed
headers. Unique tagged resources are recorded before and after creation,
cleaned in `finally`, and checked for absence. Recovery retries reads for up to
35 seconds after a creation attempt so one stale read cannot silently skip a
resource. Each intended resource must be deleted with verified absence or
explicitly verified absent. The final receipt is `passed` only after successful
HTTP, lifecycle-log, and resource cleanup checks; all checks also run under
optimized Python. If interrupted outside normal
Python cleanup, use the receipt's names and ownership tags to recover resources.
Never run this opt-in cloud test in ordinary credential-free CI.

The fixture-specific frame and payload oracle has credential-free regression
coverage in `python -m unittest discover -s tools -p test_streaming_cloud.py`.
These tests execute the runner's actual request parser with a fake socket and
clock; they do not import the AWS SDK or create an AWS session. The existing
Rust CI gate's Python unittest discovery includes them.

Client cancellation is a transport observation, not proof that remote work has
stopped. AWS documents that
[a disconnected streaming client does not stop Lambda](https://docs.aws.amazon.com/lambda/latest/dg/configuration-response-streaming.html).
The fixture therefore enforces a five-second Crucible request budget inside a
fifteen-second Lambda timeout, and checks producer drop and terminal logs before
cleanup. The receipt's `disconnect_propagated_before_budget` field is a
conservative early-drop classifier: it is true only for a producer drop below
four seconds. That threshold is distinct from the five-second deadline; false
can include a drop between four and five seconds and does not prove the absence
of cancellation. Consult the actual drop time and lifecycle outcome.
Set application deadlines below the Lambda hard timeout even when
clients cancel promptly.

## Current support

Rust servers, Rust clients, TypeScript web/Node clients, Python sync/async clients,
and Go clients implement profile 1. Other clients diagnose `server-streaming`
instead of emitting a unary API. OpenAPI includes
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

`python tools/test-native-streaming.py python` qualifies separately installed
Python wheels with and without error documentation. The `go` variant qualifies
separate consumer modules. Both exercise native TCP profiles and the actual Rust
HTTP fixture, including held LF/CRLF/CR delivery, GET/POST, opening retries,
typed late errors, cancellation, deadlines, and producer release before fixture
cleanup. These local checks do not establish deployed AWS or proxy behavior.

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
