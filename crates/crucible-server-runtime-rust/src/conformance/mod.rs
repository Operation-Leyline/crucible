//! Smithy protocol-test vectors, run against the real pipeline (`test-support`).
//!
//! `crucible_build::model(..).server().conformance()` generates one `#[test]` per
//! `smithy.test` vector the service's protocol shares with its clients, as a
//! table of the vector's data and a call into this module. Everything that
//! decides pass or fail lives here, so it is written and tested once:
//!
//! - [`request`]: sends the vector's request; the service must decode it to
//!   the vector's `params`.
//! - [`response`] and [`error`]: an operation returns `params`; the service
//!   must serialize them as the vector's status, headers and body.
//! - [`malformed`]: the service must refuse the request, before any operation
//!   runs, as the vector says.
//!
//! The operations are a [`Stub`], not the service's implementation: a
//! failure is the framework's, or the model's, never a handler's.

mod exchange;
mod expect;
mod param_shape;
mod stub;
mod vectors;

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Crucible;
use crate::error::Error;
use crate::operation::Operation;

pub use param_shape::{ParamMember, ParamShape};
pub use stub::Stub;
pub use vectors::{
    BodyAssertion, ExpectedResponse, HttpRequest, MalformedVector, Refusal, RequestVector,
    ResponseVector,
};

use stub::Reply;

/// Builds the service over a [`Stub`]: generated code registers every
/// operation with `Stub::call`, then applies the service's own configuration.
pub type Build = fn(Arc<Stub>) -> Crucible;

/// A request vector: the service decodes the request to the vector's `params`.
pub fn request<O: Operation>(build: Build, vector: &RequestVector) {
    let expected = typed::<O::Input>(vector.id, vector.params, vector.shape);
    let expected = judge(vector.id, serde_json::to_value(&expected).map_err(display));
    let stub = Stub::replying(None);
    let answered = judge(vector.id, exchange::send(build, &stub, &vector.request));
    let calls = stub.calls();
    let [(operation, decoded)] = calls.as_slice() else {
        fail(
            vector.id,
            format!(
                "the operation ran {} time(s), once expected; the service answered {}",
                calls.len(),
                answered.summary()
            ),
        );
    };
    if *operation != O::INFO.name {
        fail(
            vector.id,
            format!("{operation} ran, but {} was expected", O::INFO.name),
        );
    }
    if !expect::json_equal(&expected, decoded) {
        fail(
            vector.id,
            format!(
                "the decoded input differs from params\n  params:  {expected}\n  decoded: {decoded}"
            ),
        );
    }
}

/// A response vector: the operation returns `params` as its output.
pub fn response<O: Operation>(build: Build, vector: &ResponseVector)
where
    O::Output: DeserializeOwned,
{
    let output = typed::<O::Output>(vector.id, vector.params, vector.shape);
    answer::<O>(build, vector, Reply::Output(Box::new(output)));
}

/// A response vector for a modelled error: the operation fails with `params`
/// as the error `E`.
pub fn error<O: Operation, E>(build: Build, vector: &ResponseVector)
where
    E: DeserializeOwned + std::error::Error + Send + Sync + 'static,
{
    let error = typed::<E>(vector.id, vector.params, vector.shape);
    answer::<O>(build, vector, Reply::Error(Error::new(error)));
}

fn answer<O: Operation>(build: Build, vector: &ResponseVector, reply: Reply) {
    let stub = Stub::replying(Some(reply));
    let answered = judge(vector.id, exchange::send(build, &stub, &vector.seed));
    if stub.calls().is_empty() {
        fail(
            vector.id,
            format!(
                "the seed request never reached {}; the service answered {}",
                O::INFO.name,
                answered.summary()
            ),
        );
    }
    judge(vector.id, expect::check_response(&vector.expect, &answered));
}

/// A malformed-request vector: the service refuses the request, and no
/// operation runs.
pub fn malformed(build: Build, vector: &MalformedVector) {
    let stub = Stub::replying(None);
    let answered = judge(vector.id, exchange::send(build, &stub, &vector.request));
    if let Some((operation, _)) = stub.calls().first() {
        fail(
            vector.id,
            format!(
                "{operation} ran for a malformed request; the service answered {}",
                answered.summary()
            ),
        );
    }
    judge(vector.id, expect::check_refusal(&vector.refusal, &answered));
}

/// `params` as the type the generated model reads: translated by the shape,
/// then deserialized. A vector whose params the model cannot read is wrong
/// before it is a test.
fn typed<T: DeserializeOwned>(id: &str, params: &str, shape: &ParamShape) -> T {
    let params: Value = judge(
        id,
        serde_json::from_str(params).map_err(|error| format!("params are not JSON: {error}")),
    );
    let translated = judge(id, shape.translate(&params));
    // An operation without input or output is `()` where a structure is `{}`.
    if translated
        .as_object()
        .is_some_and(|members| members.is_empty())
        && let Ok(unit) = serde_json::from_value(Value::Null)
    {
        return unit;
    }
    judge(
        id,
        serde_json::from_value(translated.clone()).map_err(|error| {
            format!(
                "params do not read as {}: {error}\n  translated: {translated}",
                std::any::type_name::<T>()
            )
        }),
    )
}

fn judge<T>(id: &str, result: Result<T, String>) -> T {
    result.unwrap_or_else(|message| fail(id, message))
}

fn fail(id: &str, message: String) -> ! {
    panic!("conformance vector `{id}`: {message}")
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}
