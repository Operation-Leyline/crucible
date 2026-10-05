//! The runtime generated Rust clients use. A client package's `build.rs`
//! generates from the model package:
//!
//! ```ignore
//! fn main() { crucible_build::client("weather_model").generate(); }
//! ```
//!
//! and its code includes the result with `crucible_client::include_client!()`:
//! the model's types, its errors, an error enum per operation, and `Client`.
//!
//! ```ignore
//! let client = Client::new(Config::new("https://weather.example.com").token(Token::fixed(key)));
//! let city = client.get_city(GetCityInput { city_id: "sea".into() }).await?;
//! ```
//!
//! Requests travel as the service's `crucible.protocols#json` protocol
//! describes them. Throttling errors are retried; other transient failures
//! only when the operation is safe to repeat (`@readonly`, `@idempotent`,
//! or carrying an idempotency token, which the client fills when absent).

mod config;
mod paginate;
mod transport;

pub use config::{BoxFuture, Config, Interceptor, Retry, Token};
pub use crucible_wire::blob::Blob;
pub use crucible_wire::error::ModelledError;
pub use crucible_wire::field::Field;
pub use crucible_wire::open::UnknownUnion;
pub use crucible_wire::operation::{Operation, OperationInfo};
pub use crucible_wire::{blob, error, operation, time, validate};
pub use crucible_wire::{field, open};
pub use paginate::Paginator;
pub use transport::{Error, OperationErrors, Transport, UnknownError, by_status, decode_modelled};

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
