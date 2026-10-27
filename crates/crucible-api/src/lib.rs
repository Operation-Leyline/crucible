//! Opt-in entry points for Crucible's existing runtime crates.
//!
//! Enable `server`, `client`, or `auth` to expose the corresponding module.
//! No feature is enabled by default. These are crate re-exports: their types
//! remain identical to those used by generated bindings and direct dependencies.
//!
//! Build scripts continue to use `crucible-build` as a separate dependency.

/// Server pipeline, extension interfaces, protocols and hosts.
#[cfg(feature = "server")]
pub use crucible_server_runtime_rust as server;

/// Rust client transport, configuration and bearer identity providers.
#[cfg(feature = "client")]
pub use crucible_client_runtime_rust as client;

/// Explicit server authentication: JWT/OIDC, API keys and authenticator chains.
#[cfg(feature = "auth")]
pub use crucible_auth as auth;
