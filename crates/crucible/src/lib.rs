//! Crucible: a service framework for Smithy-modelled APIs.
//!
//! A service depends on a model package; `crucible-build` generates an
//! interface per operation from it, and the service implements them. This
//! crate is everything else: the orchestrator and handler chain, the request
//! context, protocols, validation, errors, the service log and metrics
//! plumbing. See `docs/framework.md`.
//!
//! ```ignore
//! mod api { crucible::include_service!(); }
//!
//! impl api::GetCityOperation for Weather {
//!     async fn get_city(&self, input: api::model::GetCityInput) -> crucible::Result<api::model::GetCityOutput> {
//!         let id = self.context().request_id().to_owned();
//!         ...
//!     }
//! }
//!
//! let service = Crucible::builder(api::operations(Arc::new(Weather::new(store))))
//!     .handler(ServiceLog::errors_only())
//!     .handler(ExceptionTranslator::new())
//!     .handler(JsonProtocol)
//!     .build()?;
//! axum::serve(listener, service.into_router()).await?;
//! ```

pub mod context;
pub mod handler;
pub mod handlers;
pub mod host;
pub mod operations;
pub mod protocol;
pub mod service;

// What the server shares with clients lives in crucible-wire, re-exported
// at the paths generated code names.
pub use crucible_wire::{error, operation, time, validate};

pub use context::{Context, RequestMeta, WithContext};
pub use error::{Error, FrameworkError, ModelledError, Result};
pub use handler::{AsyncHandler, Handler, Hook, Job, Outcome, Prepare, Resolution};
pub use handlers::HandlerExt;
pub use operation::{Operation, OperationInfo, ServiceInfo};
pub use operations::{MissingOperations, Operations};
pub use protocol::{InvokeProtocol, JsonProtocol, Protocol};
pub use service::{BuildError, Builder, Crucible, Endpoint, RequestIdSource};

/// `#[crucible::async_trait]` for implementing [`AsyncHandler`] and
/// `OperationHandler` with `async fn`.
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
