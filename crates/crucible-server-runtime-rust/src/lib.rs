//! The Rust server runtime for Crucible Smithy-modelled APIs.
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
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod conformance;
pub mod context;
mod decoded_job;
pub mod error;
mod exception_translator;
mod fn_middleware;
mod generated_operations_builder;
mod hook;
pub mod host;
pub mod interceptor;
pub mod interceptors;
pub mod job;
#[cfg(feature = "logging")]
pub mod logging;
pub mod mcp;
pub mod middleware;
pub use auth_refusal::AuthRefusal;
pub mod middlewares;
mod operation_job;
mod operation_metadata;
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
pub(crate) use crucible_wire::cbor;
pub use crucible_wire::event_stream;
pub use crucible_wire::event_stream::{EventReceiver, EventStream, EventStreamError};
pub use crucible_wire::{blob, number, operation, time, validate};

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
pub use operation_metadata::OperationMetadata;
pub use operation_middleware::OperationAwareMiddleware;
pub use operations::{MissingOperations, Operations};
pub use outcome::Outcome;
pub use prepare::Prepare;
pub use protocol::{
    AwsJsonProtocol, InvokeProtocol, JsonProtocol, Protocol, ProtocolMatch, RestJsonProtocol,
    RpcV2CborProtocol, RpcV2JsonProtocol,
};
pub use resolution::Resolution;
pub use scoped::{InterceptorExt, OperationMiddlewareExt, ProtocolExt, Scoped};
pub use service::{BuildError, Builder, Crucible, Endpoint, RequestIdSource};

/// `#[crucible_server_runtime_rust::async_trait]` for implementing [`AsyncMiddleware`] and
/// `TypedInterceptor` with `async fn`.
pub use async_trait::async_trait;

/// What generated code names through `crucible_server_runtime_rust::__generated`, so a service
/// needs no direct dependency on serde, chrono or regex.
#[doc(hidden)]
pub mod __generated {
    #[cfg(any(test, feature = "test-support"))]
    pub use crate::conformance;
    pub use crate::generated_operations_builder::OperationsBuilder;
    pub use crate::{
        Context, Crucible, MissingOperations, ModelledError, Operation, OperationInfo, Operations,
        Result, ServiceInfo, WithContext, mcp, operations,
    };
    pub use __crucible_check_abi as check_abi;
    pub use crucible_wire::__generated::{error_info, member_info, operation_info, service_info};
    pub use crucible_wire::blob::Blob;
    pub use crucible_wire::event_stream::EventStream;
    pub use crucible_wire::number::{BigDecimal, BigInteger};
    pub use crucible_wire::stream::ByteStream;
    pub use crucible_wire::{
        cbor, codec, error, event_stream, number, open, operation, time, validate,
    };

    pub use chrono;
    pub use regex;
    pub use serde;
    pub use serde_json;
}

/// Includes the Rust server bindings generated for this crate's model.
/// Call at crate root; a Smithy `Weather` service declares `pub mod weather`
/// exposing its models, exceptions and operation traits directly.
#[macro_export]
macro_rules! include_service {
    () => {
        include!(concat!(env!("OUT_DIR"), "/crucible_service.rs"));
    };
}

/// Includes the `smithy.test` conformance tests generated for this crate's model
/// by `crucible_build::model(..).server().conformance()`. Declare it inside a
/// `#[cfg(test)]` module of the crate that called `include_service!()`, with
/// `crucible-server-runtime-rust/test-support` enabled for tests. The optional
/// argument configures the service the tests build, as the production builder
/// does (a `fn(Builder) -> Builder`, closures that capture nothing included); by default only
/// the framework defaults apply.
#[cfg(feature = "test-support")]
#[macro_export]
macro_rules! include_conformance {
    () => {
        $crate::include_conformance!(|builder: $crate::Builder| builder);
    };
    ($configure:expr) => {
        fn __conformance_configure(builder: $crate::Builder) -> $crate::Builder {
            // A function pointer, so a closure's parameter type is known.
            let configure: fn($crate::Builder) -> $crate::Builder = $configure;
            configure(builder)
        }
        include!(concat!(env!("OUT_DIR"), "/crucible_conformance.rs"));
    };
}

/// Owned streaming body and chunk stream used by generated payload members.
pub use crucible_wire::stream::{ByteStream, ChunkStream};

/// Exact numbers for Smithy arbitrary-precision numeric shapes.
pub use crucible_wire::number::{BigDecimal, BigInteger};
