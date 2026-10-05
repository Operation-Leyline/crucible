//! The runtime generated Rust clients use. A client package's `build.rs`
//! generates from the model package:
//!
//! ```ignore
//! fn main() { crucible_build::client("weather_model").generate(); }
//! ```
//!
//! and its code includes the result with `crucible_client::include_client!()`:
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

mod auth;
mod build_error;
mod cancellation;
pub mod checksum;
mod config;
mod credentials;
mod endpoint;
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
pub use crucible_wire::open::UnknownUnion;
pub use crucible_wire::operation::{Checksum, ChecksumAlgorithm, Protocol};
pub use crucible_wire::operation::{Operation, OperationInfo};
pub use crucible_wire::{blob, codec, error, number, operation, time, validate};
pub use crucible_wire::{field, open};
pub use metadata::{OperationMetadata, ServiceMetadata};
pub use paginate::Paginator;
pub use transport::{
    Error, OperationErrors, ResponseMetadata, ServiceError, Transport, UnknownError, by_status,
    decode_modelled, decode_modelled_at,
};
pub use waiter::{WaiterError, WaiterFailure, WaiterOptions, WaiterResult};

/// A fresh idempotency token, for an input member that carries one.
pub fn idempotency_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Includes the client `crucible_build::client(..).generate()` wrote.
#[macro_export]
macro_rules! include_client {
    () => {
        include!(concat!(env!("OUT_DIR"), "/crucible_client.rs"));
    };
}

/// For generated code only.
#[doc(hidden)]
pub mod __private {
    pub use chrono;
    pub use http;
    pub use regex;
    pub use serde;
    pub use serde_json;
}

/// Owned streaming body and chunk stream used by generated payload members.
pub use crucible_wire::stream::{ByteStream, ChunkStream};

/// Exact numbers for Smithy arbitrary-precision numeric shapes.
pub use crucible_wire::number::{BigDecimal, BigInteger};
