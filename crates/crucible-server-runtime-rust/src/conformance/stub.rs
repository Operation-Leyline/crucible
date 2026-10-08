use std::any::Any;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::operation::Operation;
use crate::operations::BoxFuture;

/// What the next operation call answers with.
pub(crate) enum Reply {
    Output(Box<dyn Any + Send>),
    Error(Error),
}

/// Every operation of the service under test. It records the input each call
/// decoded, then answers with the scripted reply. Nothing else, so a
/// conformance run exercises the framework and not an implementation.
#[derive(Default)]
pub struct Stub {
    calls: Mutex<Vec<(&'static str, Value)>>,
    reply: Mutex<Option<Reply>>,
}

impl Stub {
    pub(crate) fn replying(reply: Option<Reply>) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::default(),
            reply: Mutex::new(reply),
        })
    }

    /// The registration for `O`: generated code passes `Stub::call::<O>` to
    /// `OperationsBuilder::add`.
    pub fn call<O: Operation>(stub: Arc<Self>, input: O::Input) -> BoxFuture<Result<O::Output>> {
        Box::pin(async move {
            let recorded = serde_json::to_value(&input).map_err(|error| {
                Error::internal(format!(
                    "{}: the decoded input is not JSON: {error}",
                    O::INFO.name
                ))
            })?;
            stub.calls
                .lock()
                .expect("stub lock")
                .push((O::INFO.name, recorded));
            let reply = stub.reply.lock().expect("stub lock").take();
            match reply {
                Some(Reply::Output(output)) => output
                    .downcast::<O::Output>()
                    .map(|output| *output)
                    .map_err(|_| {
                        Error::internal(format!(
                            "{}: scripted output of the wrong type",
                            O::INFO.name
                        ))
                    }),
                Some(Reply::Error(error)) => Err(error),
                None => Err(Error::internal(format!(
                    "{}: reached the operation",
                    O::INFO.name
                ))),
            }
        })
    }

    pub(crate) fn calls(&self) -> Vec<(&'static str, Value)> {
        self.calls.lock().expect("stub lock").clone()
    }
}
