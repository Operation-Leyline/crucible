# SDK runtime policy

This is the cross-language contract for two client runtime behaviors: retry
modes (the retry quota and adaptive client-side rate limiting) and client
telemetry. Python is the reference implementation
(`crates/crucible-codegen-python/runtime/{policy,telemetry}.py`).
A runtime follows this document and passes the shared vectors in
`tests/conformance/` before it documents either behavior.

| Behavior | Vectors | Python |
|---|---|---|
| Retry quota | [`retry-quota.json`](../tests/conformance/retry-quota.json) | `retry_mode="standard"` |
| Adaptive rate limiting | [`adaptive-retry.json`](../tests/conformance/adaptive-retry.json) | `retry_mode="adaptive"` |
| Telemetry | [`telemetry.json`](../tests/conformance/telemetry.json) | `telemetry=` |

Every client-level vector runs against a client generated from
[`runtime-policy.json`](../crates/crucible-smithy/tests/fixtures/runtime-policy.json):
a service with sensitive members bound to a label, a header and a query
parameter, and modeled throttling and retryable errors.

## Configuration

Every runtime accepts the same three settings on its client configuration,
named by its language's conventions:

| Setting | Values | Default |
|---|---|---|
| Retry mode | `legacy`, `standard`, `adaptive` | `legacy` |
| Telemetry | a hook object, or none | none |
| Clock | an object with monotonic `now` and `sleep` | the system clock |

Recommended spellings: Python, Ruby and R `retry_mode`,
`telemetry`, `clock`; TypeScript and Dart `retryMode`;
Go and C# `RetryMode` properties with a `RetryMode` enum;
Java and Kotlin builder methods `retryMode(RetryMode.ADAPTIVE)`; Rust builder
methods taking `RetryMode::Adaptive`; Swift `retryMode: .adaptive`; PHP array
keys `retry_mode`; C `CRUCIBLE_RETRY_MODE_ADAPTIVE` constants; C++
`RetryMode::Adaptive`.

The clock is a test seam. It supplies the time used for the retry quota's rate
limiter, backoff and rate-limit sleeps, and telemetry
timestamps and latencies. A call's own deadline keeps using the platform's
monotonic clock, so a fake clock never shortens or extends a real timeout.

## Retry modes

The [HTTP retry contract](clients.md#http-retry-contract) decides which
failures are eligible for a retry in every mode: modeled throttling errors
always; explicitly retryable errors, HTTP 500/502/503/504 and transport
failures only for safe operations. The modes add limits on top of that
classification; they never make an ineligible failure eligible. (The AWS SDKs
also retry non-idempotent writes after a 5xx; Crucible deliberately does not.)

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

## Telemetry

A client accepts one telemetry hook. It receives four events, in this order
for every call:

```text
operationStarted
  (attemptStarted attemptFinished)*     one pair per attempt that reached the transport
operationFinished
```

Validation, serialization, credential and middleware failures before the first
send produce `operationStarted` and `operationFinished` with zero attempts.

| Event | Fields |
|---|---|
| `operationStarted` | invocation ID, service shape ID, operation name, time |
| `attemptStarted` | invocation ID, service, operation, attempt (1-based), method, endpoint, target, headers, request bytes, time |
| `attemptFinished` | invocation ID, service, operation, attempt, status, error, request ID, response bytes, latency, retry reason, retry delay, time |
| `operationFinished` | invocation ID, service, operation, attempts, status, error, latency, time |

* **Invocation ID**: a random ID shared by one call's events, for correlating.
* **Endpoint**: scheme, host, port and base path of the attempt's endpoint;
  never user information.
* **Target**: path and query as sent, with redactions.
* **Headers**: request headers as sent (after middleware), with redactions.
* **Request bytes**: the `Content-Length` sent, or none for a chunked body.
  **Response bytes**: the bytes read.
* **Status**: the HTTP status, or none when no response arrived. The operation
  status is the last attempt's status.
* **Error**: a modeled or wire error's name, or the runtime's exception type
  name; never an error message, which may echo request data.
* **Retry reason**: set only when another attempt follows: `throttling`,
  `transient`, `timeout` or `network`. **Retry delay**: the sleep
  before that attempt.
* **Latency**: seconds on the injected clock, from the attempt being sent (or
  the call starting) to its end.

### Redaction

Events never contain bodies, inputs, outputs or error messages. In headers and
targets the value (never the name) is replaced with `***`:

* the `Authorization`, `Proxy-Authorization`, `Cookie` and `Set-Cookie`
  headers;
* every header, query parameter and path label
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
* `telemetry.json`: event sequences compared field by field (only listed
  fields), plus a scan of every event field for the scenario's secrets.

## Status

| Runtime | Retry modes | Telemetry |
|---|---|---|
| Python | yes | yes |
| C# | yes (`RetryMode`) | yes (`TelemetryHook`, `ActivityTelemetryHook`) |
| Go | yes (`RetryMode`) | yes (`TelemetryHook`) |
| Others | not yet | not yet |
