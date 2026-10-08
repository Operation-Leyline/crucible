//! Smithy's official RPC v2 JSON protocol tests against the Rust server.
//!
//! The model is `crucible-smithy`'s fixture of `smithy-protocol-tests`; the
//! vectors that apply to a server run through the real pipeline with stub
//! operations. A vector the server fails is listed in `build.rs` with its
//! reason, never dropped.

crucible_server_runtime_rust::include_service!();

#[cfg(test)]
mod conformance {
    // The argument configures the service the tests build, as a real
    // service's builder would: here an explicit body limit.
    crucible_server_runtime_rust::include_conformance!(|builder| builder.body_limit(1 << 20));
}
