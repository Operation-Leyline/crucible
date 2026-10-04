//! The operations a service implements, behind one type-erased table the
//! orchestrator calls. Generated code fills it: `operations(service)` for one
//! type implementing every operation, or `Operations::builder` for several.

use std::collections::BTreeMap;
use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::operation::{Input, Operation, OperationInfo, Output, ServiceInfo};
use crate::validate::{Rejection, Validator};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// How a registration calls an implementation: generated, one per operation.
pub type Call<S, I, O> = fn(Arc<S>, I) -> BoxFuture<Result<O>>;

pub(crate) type Invoke =
    Arc<dyn Fn(Box<dyn Input>) -> BoxFuture<Result<Box<dyn Output>>> + Send + Sync>;
type Decode = fn(Value) -> std::result::Result<Box<dyn Input>, Rejection>;

pub(crate) struct Entry {
    pub info: &'static OperationInfo,
    pub decode: Decode,
    pub invoke: Invoke,
}

pub struct Operations {
    pub(crate) service: &'static ServiceInfo,
    pub(crate) entries: BTreeMap<&'static str, Entry>,
}

impl Operations {
    pub fn builder(service: &'static ServiceInfo) -> OperationsBuilder {
        OperationsBuilder {
            operations: Operations {
                service,
                entries: BTreeMap::new(),
            },
        }
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    pub(crate) fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }
}

pub struct OperationsBuilder {
    operations: Operations,
}

impl OperationsBuilder {
    /// Registers `O`, served by `call` on `implementation`. Generated code
    /// calls this; a service registers through the generated setters.
    pub fn add<O, S>(mut self, implementation: Arc<S>, call: Call<S, O::Input, O::Output>) -> Self
    where
        O: Operation,
        S: Send + Sync + 'static,
    {
        let invoke: Invoke = Arc::new(move |input: Box<dyn Input>| {
            let implementation = implementation.clone();
            Box::pin(async move {
                let input = input.into_any().downcast::<O::Input>().map_err(|_| {
                    Error::internal(format!("{}: input of the wrong type", O::INFO.name))
                })?;
                let output = call(implementation, *input).await?;
                Ok(Box::new(output) as Box<dyn Output>)
            })
        });
        self.operations.entries.insert(
            O::INFO.name,
            Entry {
                info: O::INFO,
                decode: decode::<O>,
                invoke,
            },
        );
        self
    }

    /// The table, or the operations the service has not implemented.
    pub fn build(self) -> std::result::Result<Operations, MissingOperations> {
        let missing: Vec<&'static str> = self
            .operations
            .service
            .operations
            .iter()
            .map(|op| op.name)
            .filter(|name| !self.operations.entries.contains_key(name))
            .collect();
        if missing.is_empty() {
            Ok(self.operations)
        } else {
            Err(MissingOperations(missing))
        }
    }
}

fn decode<O: Operation>(value: Value) -> std::result::Result<Box<dyn Input>, Rejection> {
    let input: O::Input = serde_json::from_value(value)
        .map_err(|error| Rejection::malformed("", error.to_string()))?;
    Validator::check(&input)?;
    Ok(Box::new(input))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingOperations(pub Vec<&'static str>);

impl fmt::Display for MissingOperations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no implementation for: {}", self.0.join(", "))
    }
}

impl std::error::Error for MissingOperations {}
