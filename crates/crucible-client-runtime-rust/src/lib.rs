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
//! Requests travel as the service's protocol describes them:
//! `crucible.protocols#json`, or an AWS JSON protocol chosen at generation. Throttling errors are retried; other transient failures
//! only when the operation is safe to repeat (`@readonly`, `@idempotent`,
//! or carrying an idempotency token, which the client fills when absent).

include!(concat!(env!("OUT_DIR"), "/check_abi.rs"));

mod auth;
mod build_error;
mod cancellation;
pub mod checksum;
mod config;
mod credentials;
mod endpoint;
pub mod error;
mod events;
mod metadata;
mod paginate;
mod query;
mod signing;
mod transport;
mod waiter;

pub use build_error::BuildError;
pub use cancellation::CancellationToken;
pub use checksum::ChecksumPolicy;
pub use config::{
    BasicCredentials, BoxFuture, CallOptions, ChecksumValidated, Config, Identity, Interceptor,
    Retry, Token,
};
pub use credentials::AwsCredentials;
pub use crucible_wire::blob::Blob;
pub use crucible_wire::error::ModelledError;
pub use crucible_wire::field::Field;
pub use crucible_wire::open::{Enum as OpenEnum, UnknownUnion};
pub use crucible_wire::operation::{Checksum, ChecksumAlgorithm, Protocol};
pub use crucible_wire::operation::{Operation, OperationInfo};
pub use crucible_wire::{blob, number, operation, time, validate};

/// The permission registry a generated client publishes
/// (`permissions::PERMISSIONS`; docs/permissions.md §6.3).
pub mod permissions {
    pub use crucible_wire::permissions::{PermissionDescription, ResourcePermissions, Tier};
}
#[doc(hidden)]
pub use metadata::{OperationMetadata, ServiceMetadata};
pub use paginate::Paginator;
pub use transport::{Error, ResponseMetadata, ServiceError, Transport, UnknownError};
#[doc(hidden)]
pub use transport::{OperationErrors, by_status, decode_modelled, decode_modelled_at};
pub use waiter::{WaiterError, WaiterFailure, WaiterOptions, WaiterResult};

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
        OperationInfo, OperationMetadata, Paginator, ServiceMetadata, Transport, UnknownUnion,
        WaiterError, WaiterOptions, WaiterResult, by_status, decode_modelled_at, idempotency_token,
    };
    pub use __crucible_check_abi as check_abi;
    pub use crucible_wire::__generated::{error_info, member_info, operation_info, service_info};
    pub use crucible_wire::blob::Blob;
    pub use crucible_wire::event_stream::EventStream;
    pub use crucible_wire::number::{BigDecimal, BigInteger};
    pub use crucible_wire::stream::ByteStream;
    pub use crucible_wire::{
        cbor, codec, error, event_stream, field, number, open, operation, permissions, time,
        validate,
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

pub use crucible_wire::event_stream;
/// Event stream members: events to send, events received, and why a
/// stream ended early.
pub use crucible_wire::event_stream::{EventReceiver, EventStream, EventStreamError};

/// Owned streaming body and chunk stream used by generated payload members.
pub use crucible_wire::stream::{ByteStream, ChunkStream};

/// Exact numbers for Smithy arbitrary-precision numeric shapes.
pub use crucible_wire::number::{BigDecimal, BigInteger};
