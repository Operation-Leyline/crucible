# Seshat

[Seshat](https://github.com/Operation-Leyline/seshat) ships a service's log
lines to S3 for Athena instead of CloudWatch Logs. It reads one JSON object
per line on stdout, in its log-line contract. `crucible-seshat` writes the
runtime's two completion records in that shape:

- `SeshatLog`, a `LogSink` for `ServiceLog`: one `crucible.request` line per
  logged request.
- `SeshatMetrics`, a `Recorder` for `Metrics`: one counter increment per
  request.

It depends on nothing of Seshat's: the contract is the interface. Add it from
the same revision as your runtime, so the two always match:

```toml
crucible-server-runtime-rust = { git = "https://github.com/Operation-Leyline/crucible", rev = "<rev>" }
crucible-seshat = { git = "https://github.com/Operation-Leyline/crucible", rev = "<rev>" }
```

```rust
use crucible_seshat::{SeshatLog, SeshatMetrics};
use crucible_server_runtime_rust::middlewares::{Metrics, ServiceLog};

Crucible::builder(operations)
    .middleware(Metrics::new(SeshatMetrics::new("Requests")))
    .middleware(ServiceLog::errors_only().sink(SeshatLog))
```

Both emit `tracing` events. Print them with a JSON subscriber that flattens
event fields: `seshat_tracing::init()` does, and always enables the
`seshat::metric` target counters use.

## Request lines

| Key | Value |
|---|---|
| `event` | `crucible.request` |
| `level` | the level `ServiceLog`'s policy chose |
| `message` | `request completed` |
| `request_id`, `operation` | the request's id and operation name |
| `status`, `outcome`, `latency_ms` | the response status, `success`/`error`/`fault`, and the time from arrival through serializing the response |
| `error` | the modelled error's name, when there is one |
| `cause`, `input`, `output`, `error_fields`, `violations` | as the policy logs them; payloads are JSON text, already redacted |

`request_id`, `operation`, `level`, `message`, `event` and `error` are core
Seshat columns. Declare `status`, `outcome` and `latency_ms` as `logs`
columns in `seshat.json` to query them typed; undeclared, they are in
`attrs`. To give requests their own table instead, route the event:

```json
{
  "tables": [
    {
      "name": "requests",
      "events": ["crucible.request"],
      "columns": [
        { "name": "status", "type": "int" },
        { "name": "outcome", "type": "string" },
        { "name": "latency_ms", "type": "double" }
      ]
    }
  ]
}
```

## Counters

`SeshatMetrics::new(name)` counts each request once, carrying `operation`
(absent when no operation served the request), `class` (`success`, `error` or
`fault`) and `status`. Seshat publishes only declared counters, with only the
declared dimensions:

```json
{
  "metrics": {
    "namespace": "MyService",
    "counters": [{ "name": "Requests", "dimensions": ["operation", "class"] }]
  }
}
```

Each dimension combination is a CloudWatch metric; `status` is better left
in the request lines than declared as a dimension.

The counter sees what `Metrics` sees (`docs/framework.md`, "Metrics"): a
request answered before the transport middleware runs, such as an MCP
admission refusal, is not counted, and the class is the one the response had
when the record was taken, before a protocol's `decorate_response`. Count
those cases from the platform (the host's own request metrics), not here.
