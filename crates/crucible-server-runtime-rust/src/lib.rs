//! The Rust server runtime for APIs modelled in Crucible (`.cru`).
//!
//! A service depends on a model package; `crucible-build` generates an
//! interface per operation from it, and the service implements them. This
//! crate is everything else: the orchestrator and phased pipeline, the request
//! context, protocols, validation, errors, the service log and metrics
//! plumbing. See `docs/framework.md`.
//!
//! ```ignore
//! crucible_server_runtime_rust::include_service!();
//! use weather::{GetCityInput, GetCityOperation, GetCityOutput};
//! use crucible_server_runtime_rust::Result;
//!
//! impl GetCityOperation for Weather {
//!     async fn get_city(&self, input: GetCityInput) -> Result<GetCityOutput> {
//!         let id = self.context().request_id().to_owned();
//!         ...
//!     }
//! }
//!
//! let service = Crucible::builder(weather::operations(Arc::new(Weather::new(store))))
//!     .middleware(ServiceLog::errors_only())
//!     .exception_translator(ExceptionTranslator::new())
//!     .protocol(JsonProtocol)
//!     .build()?;
//! axum::serve(listener, service.into_router()).await?;
//! ```

include!(concat!(env!("OUT_DIR"), "/check_abi.rs"));

mod async_interceptor;
mod async_middleware;
mod async_operation_middleware;
mod auth_refusal;
#[cfg(any(feature = "page-tokens", feature = "idempotency"))]
mod caller_binding;
#[cfg(any(feature = "page-tokens", feature = "idempotency"))]
mod canonical_json;
pub mod context;
mod deadline_exceeded;
mod decoded_job;
mod deferred_stream;
pub mod endpoint;
pub mod error;
mod event_stream;
mod exception_translator;
pub use event_stream::EventStream;
mod streaming_output;
pub use streaming_output::StreamingOutput;
mod stream_completion;
mod stream_end;
mod stream_options;
pub use stream_completion::StreamCompletion;
pub use stream_end::StreamEnd;
pub use stream_options::StreamOptions;
#[cfg(feature = "explorer")]
pub mod explorer;
mod fn_middleware;
mod generated_operations_builder;
mod hook;
pub mod host;
#[cfg(feature = "idempotency")]
pub mod idempotency;
pub mod interceptor;
pub mod interceptors;
pub mod job;
#[cfg(feature = "logging")]
pub mod logging;
pub mod mcp;
pub mod middleware;
pub use auth_refusal::AuthRefusal;
pub mod middlewares;
mod opaque_error;
mod operation_job;
mod operation_metadata;
pub mod operation_middleware;
pub mod operations;
mod outcome;
#[cfg(feature = "page-tokens")]
pub mod page_tokens;
pub mod permissions;
mod prepare;
pub mod protocol;
mod resolution;
mod scoped;
pub mod service;
pub mod versioning;

// What the server shares with clients lives in crucible-wire, re-exported
// at the paths generated code names.
pub use crucible_wire::blob::Blob;
pub use crucible_wire::{blob, number, operation, time, validate};

pub use async_interceptor::AsyncInterceptor;
pub use async_middleware::AsyncMiddleware;
pub use async_operation_middleware::AsyncOperationAwareMiddleware;
pub use context::{Context, RequestMeta, WithContext};
pub use deadline_exceeded::{DEADLINE_RESERVE, DeadlineExceeded};
pub use decoded_job::DecodedJob;
pub use error::{Error, FrameworkError, MAX_SOURCE_DEPTH, ModelledError, Result};
pub use exception_translator::ExceptionTranslator;
pub use fn_middleware::{FnMiddleware, FnOperationMiddleware, after_fn, before_fn, raw_before_fn};
pub use hook::Hook;
pub use interceptor::Interceptor;
pub use interceptors::{TypedInterceptor, for_operation};
pub use job::Job;
pub use middleware::Middleware;
pub use opaque_error::OpaqueError;
pub use operation::{Operation, OperationInfo, ServiceInfo};
pub use operation_job::OperationJob;
pub use operation_metadata::OperationMetadata;
pub use operation_middleware::OperationAwareMiddleware;
pub use operations::{MissingOperations, Operations};
pub use outcome::Outcome;
pub use prepare::{Prepare, ServedOperation};
pub use protocol::{InvokeProtocol, JsonProtocol, Protocol, ProtocolMatch};
pub use resolution::Resolution;
pub use scoped::{InterceptorExt, OperationMiddlewareExt, ProtocolExt, Scoped};
pub use service::{BuildError, Builder, Crucible, Endpoint, RequestIdSource};
pub use versioning::{ApiVersion, Unversioned};

/// `#[crucible_server_runtime_rust::async_trait]` for implementing [`AsyncMiddleware`] and
/// `TypedInterceptor` with `async fn`.
pub use async_trait::async_trait;

/// What generated code names through `crucible_server_runtime_rust::__generated`, so a service
/// needs no direct dependency on serde, chrono or regex.
#[doc(hidden)]
pub mod __generated {
    #[cfg(feature = "explorer")]
    pub use crate::explorer::description::description as explorer_description;
    pub use crate::generated_operations_builder::OperationsBuilder;
    pub use crate::permissions::not_found as permission_not_found;
    pub use crate::{
        Context, Crucible, MissingOperations, ModelledError, Operation, OperationInfo, Operations,
        Result, ServiceInfo, WithContext, mcp, operations, versioning,
    };
    pub use __crucible_check_abi as check_abi;
    pub use crucible_wire::__generated::{error_info, member_info, operation_info, service_info};
    pub use crucible_wire::blob::Blob;
    pub use crucible_wire::number::{BigDecimal, BigInteger};
    pub use crucible_wire::{
        codec, error, field, number, open, operation, permissions, time, validate,
    };

    pub use chrono;
    pub use regex;
    pub use serde;
    pub use serde_json;
}

/// Includes the Rust server bindings generated for this crate's model.
/// Call at crate root; a `Weather` service declares `pub mod weather`
/// exposing its models, exceptions and operation traits directly.
#[macro_export]
macro_rules! include_service {
    () => {
        include!(concat!(env!("OUT_DIR"), "/crucible_service.rs"));
    };
}

/// Exact numbers for `bigInteger` and `bigDecimal` members.
pub use crucible_wire::number::{BigDecimal, BigInteger};
