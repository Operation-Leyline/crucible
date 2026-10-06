//! Crucible: a service framework for Smithy-modelled APIs.
//!
//! A service depends on a model package; `crucible-build` generates an
//! interface per operation from it, and the service implements them. This
//! crate is everything else: the orchestrator and phased pipeline, the request
//! context, protocols, validation, errors, the service log and metrics
//! plumbing. See `docs/framework.md`.
//!
//! ```ignore
//! pub mod contract { crucible::include_service!(); }
//!
//! impl contract::GetCityOperation for Weather {
//!     async fn get_city(&self, input: contract::model::GetCityInput) -> crucible::Result<contract::model::GetCityOutput> {
//!         let id = self.context().request_id().to_owned();
//!         ...
//!     }
//! }
//!
//! let service = Crucible::builder(contract::operations(Arc::new(Weather::new(store))))
//!     .middleware(ServiceLog::errors_only())
//!     .exception_translator(ExceptionTranslator::new())
//!     .protocol(JsonProtocol)
//!     .build()?;
//! axum::serve(listener, service.into_router()).await?;
//! ```

mod async_interceptor;
mod async_middleware;
mod async_operation_middleware;
pub mod context;
mod decoded_job;
mod exception_translator;
mod fn_middleware;
mod hook;
pub mod host;
pub mod interceptor;
pub mod interceptors;
pub mod job;
#[cfg(feature = "logging")]
pub mod logging;
pub mod middleware;
pub mod middlewares;
mod operation_job;
pub mod operation_middleware;
pub mod operations;
mod outcome;
mod prepare;
pub mod protocol;
mod resolution;
mod scoped;
pub mod service;

// What the server shares with clients lives in crucible-wire, re-exported
// at the paths generated code names.
pub use crucible_wire::blob::Blob;
pub use crucible_wire::{blob, codec, error, number, open, operation, time, validate};

pub use async_interceptor::AsyncInterceptor;
pub use async_middleware::AsyncMiddleware;
pub use async_operation_middleware::AsyncOperationAwareMiddleware;
pub use context::{Context, RequestMeta, WithContext};
pub use decoded_job::DecodedJob;
pub use error::{Error, FrameworkError, MAX_SOURCE_DEPTH, ModelledError, Result};
pub use exception_translator::ExceptionTranslator;
pub use fn_middleware::{FnMiddleware, FnOperationMiddleware, after_fn, before_fn, raw_before_fn};
pub use hook::Hook;
pub use interceptor::Interceptor;
pub use interceptors::{TypedInterceptor, for_operation};
pub use job::Job;
pub use middleware::Middleware;
pub use operation::{Operation, OperationInfo, ServiceInfo};
pub use operation_job::OperationJob;
pub use operation_middleware::OperationAwareMiddleware;
pub use operations::{MissingOperations, Operations};
pub use outcome::Outcome;
pub use prepare::Prepare;
pub use protocol::{InvokeProtocol, JsonProtocol, Protocol, ProtocolMatch};
pub use resolution::Resolution;
pub use scoped::{InterceptorExt, OperationMiddlewareExt, ProtocolExt, Scoped};
pub use service::{BuildError, Builder, Crucible, Endpoint, RequestIdSource};

/// `#[crucible::async_trait]` for implementing [`AsyncMiddleware`] and
/// `TypedInterceptor` with `async fn`.
pub use async_trait::async_trait;

/// What generated code names through `crucible::__private`, so a service
/// needs no direct dependency on serde, chrono or regex.
#[doc(hidden)]
pub mod __private {
    pub use chrono;
    pub use regex;
    pub use serde;
    pub use serde_json;
}

/// Includes the code `crucible-build` generated for this crate's model.
#[macro_export]
macro_rules! include_service {
    () => {
        include!(concat!(env!("OUT_DIR"), "/crucible_service.rs"));
    };
}

/// Owned streaming body and chunk stream used by generated payload members.
pub use crucible_wire::stream::{ByteStream, ChunkStream};

/// Exact numbers for Smithy arbitrary-precision numeric shapes.
pub use crucible_wire::number::{BigDecimal, BigInteger};
