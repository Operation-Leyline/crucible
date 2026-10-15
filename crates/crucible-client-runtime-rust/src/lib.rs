//! The runtime generated Rust clients use. A client package's `build.rs`
//! generates from the model package:
//!
//! ```ignore
//! fn main() { crucible_build::model("weather_model").client().generate(); }
//! ```
//!
//! and its code includes the result with `crucible_client_runtime_rust::include_client!()`:
//! the model's types, its errors, an error enum per operation, and its service-named client.
//!
//! ```ignore
//! let client = WeatherClient::new(Config::new("https://weather.example.com").token(Token::fixed(key)));
//! let city = client.get_city(GetCityInput::builder().city_id("sea").build()).await?;
//! ```
//!
//! Requests travel as `crucible.protocols#json` describes them. Throttling
//! errors are retried; other transient failures
//! only when the operation is safe to repeat (`readonly`, `idempotent`,
//! or carrying an idempotency token, which the client fills when absent).

include!(concat!(env!("OUT_DIR"), "/check_abi.rs"));

mod auth;
mod build_error;
mod cancellation;
mod config;
pub mod error;
mod paginate;
mod transport;

pub use build_error::BuildError;
pub use cancellation::CancellationToken;
pub use config::{BoxFuture, CallOptions, Config, Identity, Interceptor, Retry, Token};
pub use crucible_wire::blob::Blob;
pub use crucible_wire::error::ModelledError;
pub use crucible_wire::field::Field;
pub use crucible_wire::open::{Enum as OpenEnum, UnknownUnion};
pub use crucible_wire::operation::{Operation, OperationInfo};
pub use crucible_wire::{blob, number, operation, time, validate};

/// The permission registry a generated client publishes
/// (`permissions::PERMISSIONS`; docs/permissions.md §6.3).
pub mod permissions {
    pub use crucible_wire::permissions::{PermissionDescription, ResourcePermissions, Tier};
}
pub use paginate::Paginator;
pub use transport::{Error, ResponseMetadata, ServiceError, Transport, UnknownError};
#[doc(hidden)]
pub use transport::{OperationErrors, by_status, decode_modelled};

/// A fresh idempotency token, for an input member that carries one.
#[doc(hidden)]
pub fn idempotency_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Includes the client `crucible_build::model(..).client().generate()` wrote.
#[macro_export]
macro_rules! include_client {
    () => {
        include!(env!("CRUCIBLE_CLIENT_SOURCE"));
    };
}

/// For generated code only.
#[doc(hidden)]
pub mod __generated {
    pub use crate::{
        BuildError, CallOptions, Config, Error, Field, ModelledError, Operation, OperationErrors,
        OperationInfo, Paginator, Transport, UnknownUnion, by_status, decode_modelled,
        idempotency_token,
    };
    pub use __crucible_check_abi as check_abi;
    pub use crucible_wire::__generated::{error_info, member_info, operation_info, service_info};
    pub use crucible_wire::blob::Blob;
    pub use crucible_wire::number::{BigDecimal, BigInteger};
    pub use crucible_wire::{
        codec, error, field, number, object, open, operation, permissions, time, validate,
    };

    pub const fn build_error(shape: &'static str, member: &'static str) -> crate::BuildError {
        crate::BuildError { shape, member }
    }
    pub use chrono;
    pub use http;
    pub use regex;
    pub use serde;
    pub use serde_json;
}

/// Exact numbers for Smithy arbitrary-precision numeric shapes.
pub use crucible_wire::number::{BigDecimal, BigInteger};
