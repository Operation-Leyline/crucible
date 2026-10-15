# SDK runtime policy

This is the cross-language contract for three client runtime behaviors: retry
modes (the retry quota and adaptive client-side rate limiting), endpoint
discovery, and client telemetry. Python is the reference implementation
(`crates/crucible-codegen-python/runtime/{policy,discovery,telemetry}.py`).
A runtime follows this document and passes the shared vectors in
`tests/conformance/` before it documents any of these behaviors, or declares
`Feature::EndpointDiscovery`.

| Behavior | Vectors | Python |
|---|---|---|
| Retry quota | [`retry-quota.json`](../tests/conformance/retry-quota.json) | `retry_mode="standard"` |
| Adaptive rate limiting | [`adaptive-retry.json`](../tests/conformance/adaptive-retry.json) | `retry_mode="adaptive"` |
| Endpoint discovery | [`endpoint-discovery.json`](../tests/conformance/endpoint-discovery.json) | `endpoint_discovery=` |
| Telemetry | [`telemetry.json`](../tests/conformance/telemetry.json) | `telemetry=` |

Every client-level vector runs against a client generated from
[`runtime-policy.json`](../crates/crucible-smithy/tests/fixtures/runtime-policy.json):
a service with an API key, a discovery operation, a required and an optional
discovered operation, sensitive members bound to a label, a header and a query
parameter, and modeled throttling and retryable errors.

## Configuration

Every runtime accepts the same four settings on its client configuration,
named by its language's conventions:

| Setting | Values | Default |
|---|---|---|
| Retry mode | `legacy`, `standard`, `adaptive` | `legacy` |
| Endpoint discovery | unset, enabled, disabled | unset |
| Telemetry | a hook object, or none | none |
| Clock | an object with monotonic `now` and `sleep` | the system clock |

Recommended spellings: Python, Ruby and R `retry_mode`, `endpoint_discovery`,
`telemetry`, `clock`; TypeScript and Dart `retryMode`, `endpointDiscovery`;
Go and C# `RetryMode`, `EndpointDiscovery` properties with a `RetryMode` enum;
Java and Kotlin builder methods `retryMode(RetryMode.ADAPTIVE)`; Rust builder
methods taking `RetryMode::Adaptive`; Swift `retryMode: .adaptive`; PHP array
keys `retry_mode`; C `CRUCIBLE_RETRY_MODE_ADAPTIVE` constants; C++
`RetryMode::Adaptive`. The endpoint discovery setting is tri-state: Python uses
`None`/`True`/`False`, a language without a natural tri-state uses an enum
(`auto`, `enabled`, `disabled`).

The clock is a test seam. It supplies the time used for the retry quota's rate
limiter, discovery cache expiry, backoff and rate-limit sleeps, and telemetry
timestamps and latencies. A call's own deadline keeps using the platform's
monotonic clock, so a fake clock never shortens or extends a real timeout.

## Retry modes

The [HTTP retry contract](clients.md#http-retry-contract) decides which
failures are eligible for a retry in every mode: modeled throttling errors
always; explicitly retryable errors, HTTP 500/502/503/504 and transport
failures only for safe operations. The modes add limits on top of that
classification; they never make an ineligible failure eligible. (The AWS SDKs
also retry non-idempotent writes after a 5xx; Crucible deliberately does not.)
Endpoint discovery adds one eligible failure, described [below](#invalidation).

Backoff is the same in every mode: full jitter, `random() * min(max_delay,
base_delay * 2^(attempt - 1))` after the given 1-based failed attempt, unless a
valid `Retry-After` header supplies the delay. Python's defaults are 0.1 s and
20 s; the AWS SDKs use a 1 s base, which `base_delay=1` reproduces.

**Why `legacy` is the default.** The behavior shipped before this document is
not standard mode: it has the same eligibility and backoff but no retry quota,
so a client keeps retrying through an outage. Standard mode only differs once
a client has spent its quota, which is exactly the situation in which changing
behavior silently would surprise a caller. The name follows botocore, whose
default is also called `legacy`. Callers opt in to `standard` or `adaptive`.

### Retry quota (`standard` and `adaptive`)

One quota belongs to one client and is shared by all of its calls and threads.

| Constant | Value |
|---|---|
| Initial and maximum tokens | 500 |
| Retry cost | 5 |
| Retry cost after a transport timeout | 10 |
| Increment for a call that succeeds without retrying | 1 |

1. Before every retry, after deciding the failure is eligible and the attempt
   limit allows another attempt, acquire the cost. If fewer tokens are
   available than the cost, do not retry: the call fails with the failure it
   has, and the tokens are unchanged.
2. When a call succeeds (a 2xx response decoded into its output), release the
   cost of that call's **last** retry acquisition, or 1 if it never retried.
   The quota never exceeds 500.
3. A call that fails releases nothing; its retries stay spent.

A transport timeout is a timeout of the HTTP exchange itself (connect, read,
write). The call's total deadline is not a timeout failure; it ends the call.
An endpoint invalidation retry costs 5.

### Adaptive rate limiting (`adaptive`)

Adaptive mode is standard mode plus a client-side send-rate limiter, shared by
all calls of the client. The limiter follows the AWS SDKs' adaptive retry mode;
the CUBIC and rate-update vectors reproduce the values the AWS SDKs publish.

| Constant | Value |
|---|---|
| Minimum fill rate | 0.5 tokens/s |
| Minimum bucket capacity | 1 token |
| Measurement smoothing | 0.8 |
| Measurement bucket | 0.5 s |
| CUBIC beta | 0.7 |
| CUBIC scale constant | 0.4 |
| Rate ceiling | 2 x the measured rate |

State: a send token bucket (`capacity` 0, `fillRate`, `maxCapacity`, last refill
time unset, disabled), the measured rate (0, with the last measurement bucket
`floor(start)` and a count of 0), and CUBIC (`lastMaxRate` 0, `lastThrottle`
the start time).

```text
refill(now):        if last is unset: last = now; return
                    capacity = min(maxCapacity, capacity + (now - last) * fillRate); last = now
beforeSend(now):    if disabled: return 0
                    refill(now); capacity -= 1
                    return capacity >= 0 ? 0 : -capacity / fillRate     # sleep this long, then send
afterAttempt(throttled, now):
    bucket = floor(now * 2) / 2; count += 1
    if bucket > lastBucket:
        measured = (count / (bucket - lastBucket)) * 0.8 + measured * 0.2
        count = 0; lastBucket = bucket
    if throttled:
        rate = enabled ? min(measured, fillRate) : measured
        lastMaxRate = rate; k = cbrt(lastMaxRate * (1 - 0.7) / 0.4); lastThrottle = now
        newRate = rate * 0.7; enabled = true
    else:
        newRate = 0.4 * (now - lastThrottle - k)^3 + lastMaxRate
    refill(now)
    fillRate = max(min(newRate, 2 * measured), 0.5)
    maxCapacity = max(min(newRate, 2 * measured), 1)
    capacity = min(capacity, maxCapacity)
```

`beforeSend` runs before every attempt, including the first. Taking the token
before sleeping (capacity may go negative) is equivalent to the AWS
formulation and gives concurrent callers first-come order. `afterAttempt` runs
once per attempt that reached the transport: a decoded response, an HTTP error,
a transport failure, or an error raised while reading the response. Only a
modeled throttling error counts as throttled, the same classification the
retry contract uses; HTTP 429 without a modeled throttling error does not.

A synchronous client whose send delay would pass the call's deadline fails
with a timeout before sleeping and returns the token. An asynchronous client
sleeps under its deadline and returns the token if cancelled while waiting.

## Endpoint discovery

A service with `@aws.api#clientEndpointDiscovery` names a discovery operation
and optionally an error that means "this endpoint is no longer valid".
`crucible-smithy` validates the contract and lowers it once:
`Service.endpoint_discovery` (operation, error, the input's `Operation` and
`Identifiers` members, and the output's `Endpoints`/`Address`/
`CachePeriodInMinutes` members) and, per operation with
`@aws.api#clientDiscoveredEndpoint`, `Operation.discovered_endpoint` (required,
and the input members marked `@aws.api#clientEndpointDiscoveryId`, in source
order). The common runtime schema carries both as `endpointDiscovery` and
`operations.<name>.discoveredEndpoint`. Every construct the lowering refuses is
a diagnostic.

### When a call discovers

| Setting | Required operation | Optional operation | Other operations |
|---|---|---|---|
| unset (default) | discovers | configured endpoint | configured endpoint |
| enabled | discovers | discovers | configured endpoint |
| disabled | fails before sending | configured endpoint | configured endpoint |

The discovery operation itself never discovers.

### The request and the cache key

The discovery request sends `Operation` = the operation's name when the
discovery input has an `Operation` member, and `Identifiers` = a map from each
identifier member's model name to its string value when the discovery input
has an `Identifiers` member and the operation has identifiers. What is sent is
exactly the cache key: (operation name or none, identifiers sorted by name).
Python encodes it as the JSON `[operation, [[name, value], ...]]`. Credentials
are not part of the key; a cache belongs to one client.

The discovery call is an ordinary call of the discovery operation on the
configured (or rules-resolved) endpoint: it is authenticated, retried under its
own operation's eligibility (it is normally readonly), observed by telemetry as
its own operation, and bounded by the remaining deadline of the call that
started it (at most the client's default timeout).

### Using a response

The first entry of `Endpoints` whose `Address` is a nonempty string and whose
`CachePeriodInMinutes` is an integer is used. An address without a scheme gets
the configured endpoint's scheme (normally `https`); the result must be an
absolute HTTP(S) URL without user information, query or fragment, or the entry
is skipped. The entry expires at `now + max(0, CachePeriodInMinutes) * 60` s on
the injected clock; at or after that instant it is not used. A period of zero
serves only the calls that waited for that discovery. No usable entry is a
discovery failure.

The discovered URL replaces the operation's endpoint for the attempt: the
request path and query are appended to it. Host prefixes are not applied to a
discovered address; endpoint-rule headers and auth-scheme properties still are.

### Single flight

Concurrent calls that need the same key while no valid entry exists share one
discovery. The others wait for it (a synchronous waiter is bounded by its own
deadline) and receive its endpoint or its failure. In an asynchronous runtime
the shared discovery runs as its own task: cancelling the call that started it
does not cancel it for the others.

### Failure

A required operation fails with the discovery failure: the discovery call's
own error (for example a modeled error, a network error or a timeout), or the
runtime's endpoint discovery error when the response has no usable endpoint.
An optional operation uses the configured endpoint instead and keeps doing so
for that key for 60 s without retrying discovery. Cancellation is never
swallowed.

### Invalidation

When an attempt sent to a discovered endpoint fails with HTTP 421 or with the
service's modeled discovery error, the cache entry that attempt used is evicted
(unless another call already replaced it). The failure is then eligible for a
retry even for an operation that is not otherwise safe, because the request
was misdirected and not processed; the attempt limit, the retry quota and
one-shot streaming bodies still apply. The next attempt rediscovers. Retry
reason: `endpoint`.

## Telemetry

A client accepts one telemetry hook. It receives four events, in this order
for every call:

```text
operationStarted
  (attemptStarted attemptFinished)*     one pair per attempt that reached the transport
operationFinished
```

A discovery call is its own operation and its events appear, complete, between
the outer call's `operationStarted` and that attempt's `attemptStarted`.
Validation, serialization, credential and middleware failures before the first
send produce `operationStarted` and `operationFinished` with zero attempts.

| Event | Fields |
|---|---|
| `operationStarted` | invocation ID, service shape ID, operation name, time |
| `attemptStarted` | invocation ID, service, operation, attempt (1-based), method, endpoint, endpoint source, target, headers, request bytes, time |
| `attemptFinished` | invocation ID, service, operation, attempt, status, error, request ID, response bytes, latency, retry reason, retry delay, time |
| `operationFinished` | invocation ID, service, operation, attempts, status, error, latency, time |

* **Invocation ID**: a random ID shared by one call's events, for correlating.
* **Endpoint**: scheme, host, port and base path of the attempt's endpoint;
  never user information. **Endpoint source**: `configured`, `rules` or
  `discovered`.
* **Target**: path and query as sent, with redactions.
* **Headers**: request headers as sent (after middleware, checksums and
  signing), with redactions.
* **Request bytes**: the `Content-Length` sent, or none for a chunked stream.
  **Response bytes**: the bytes read; for a streamed response, its
  `Content-Length` if known.
* **Status**: the HTTP status, or none when no response arrived. The operation
  status is the last attempt's status.
* **Error**: a modeled or wire error's name, or the runtime's exception type
  name; never an error message, which may echo request data.
* **Retry reason**: set only when another attempt follows: `throttling`,
  `transient`, `timeout`, `network` or `endpoint`. **Retry delay**: the sleep
  before that attempt.
* **Latency**: seconds on the injected clock, from the attempt being sent (or
  the call starting) to its end.

### Redaction

Events never contain bodies, inputs, outputs or error messages. In headers and
targets the value (never the name) is replaced with `***`:

* the `Authorization`, `Proxy-Authorization`, `Cookie`, `Set-Cookie` and
  `X-Amz-Security-Token` headers;
* the `X-Amz-Signature`, `X-Amz-Credential` and `X-Amz-Security-Token` query
  parameters;
* the header or query parameter named by every `@httpApiKeyAuth` scheme;
* every header, prefix header, query parameter, query-map entry and path label
  bound to a member that is `@sensitive` (on the member or its target), as the
  common schema marks it with `"sensitive": true`.

Header and query names compare case-insensitively.

### Hook failures

A hook that raises is logged by the runtime and otherwise ignored; telemetry
never changes a call's result, its retries or its timing.

### OpenTelemetry

No runtime depends on OpenTelemetry. A runtime may ship an adapter that maps
events onto any tracer with the OpenTelemetry shape: one span per operation
named `<Service>.<Operation>` with `rpc.system=smithy`, `rpc.service`,
`rpc.method`; per attempt `server.address`, `http.request.method`,
`http.request.resend_count` (from the second attempt) and a
`crucible.attempt` span event carrying `http.response.status_code`,
`error.type`, `crucible.retry_reason` and `crucible.request_id`; at the end
`crucible.attempts`, `http.response.status_code`, `error.type` and an error
status when the call failed. Python's `OpenTelemetryHook(tracer)` does this by
duck typing and only imports `opentelemetry.trace` to set an error status when
the package happens to be installed.

## Running the vectors

* `retry-quota.json`: `quota` steps drive the token bucket directly; `calls`
  run each scenario on a fresh client, counting the requests each step sends
  and reading the quota afterwards (a test seam may expose it).
* `adaptive-retry.json`: unit vectors for CUBIC, the rate measurement and the
  send bucket, on a clock starting at 0. Compare rates within 1e-6.
* `endpoint-discovery.json`: a fresh client per scenario with an injected
  clock; every request must match the next scripted exchange, and every
  exchange must be used.
* `telemetry.json`: event sequences compared field by field (only listed
  fields), plus a scan of every event field for the scenario's secrets.

Single-flight discovery is concurrency behavior and is tested natively (Python:
eight threads or tasks, one discovery, including a cancelled leader).

## Status

| Runtime | Retry modes | Endpoint discovery | Telemetry |
|---|---|---|---|
| Python | yes | yes (`Feature::EndpointDiscovery` declared) | yes |
| C# | yes (`RetryMode`) | yes (`Feature::EndpointDiscovery` declared) | yes (`TelemetryHook`, `ActivityTelemetryHook`) |
| Go | yes (`RetryMode`) | yes (`Feature::EndpointDiscovery` declared) | yes (`TelemetryHook`) |
| Others | not yet | not declared | not yet |
