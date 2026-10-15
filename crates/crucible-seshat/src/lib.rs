//! A crucible service's request records and counters as Seshat log lines.
//!
//! Seshat ships a service's stdout to S3 for Athena; it reads one JSON object
//! per line in its log-line contract. This crate turns the runtime's two
//! completion records into that shape:
//!
//! - [`SeshatLog`], a [`LogSink`](crucible_server_runtime_rust::middlewares::LogSink)
//!   for `ServiceLog`: one `crucible.request` line per logged request, with
//!   the contract's names (`request_id`, `operation`, `error`) and the
//!   request's `status`, `outcome` and `latency_ms`.
//! - [`SeshatMetrics`], a [`Recorder`](crucible_server_runtime_rust::middlewares::Recorder)
//!   for `Metrics`: one counter increment per request, with `operation` and
//!   `class` (`success`, `error` or `fault`) as candidate dimensions.
//!
//! Both emit `tracing` events; a JSON subscriber that flattens event fields
//! (`seshat-tracing`, or `tracing-subscriber`'s JSON layer) prints them as
//! contract lines. Nothing here depends on Seshat's crates: the contract is
//! the interface, so this crate always matches the runtime it ships with and
//! never pins a Seshat version. See `docs/seshat.md`.

mod fields;
mod seshat_log;
mod seshat_metrics;

pub use seshat_log::SeshatLog;
pub use seshat_metrics::SeshatMetrics;

/// The Seshat log-line contract version these lines follow.
pub const CONTRACT_VERSION: u32 = 1;
/// The `event` of a request line; a service may route it to its own table.
pub const REQUEST_EVENT: &str = "crucible.request";
/// The `tracing` target of request lines.
pub const REQUEST_TARGET: &str = "crucible::request";
/// The `event` that makes a line a counter increment (Seshat's `seshat.metric`).
pub const METRIC_EVENT: &str = "seshat.metric";
/// The `tracing` target of counter lines. `seshat-tracing` always enables it,
/// so a quieter `RUST_LOG` cannot silence a declared counter; another
/// subscriber must enable it too.
pub const METRIC_TARGET: &str = "seshat::metric";
