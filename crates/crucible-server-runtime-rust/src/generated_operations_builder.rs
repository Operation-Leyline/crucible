use crate::OperationMetadata;
use crate::error::{Error, Result};
use crate::operation::{Input, Operation, Output, Segment};
use crate::operations::{
    Entry, Invoke, MissingOperations, Operations, StreamingOutput, decode, decode_bound,
    decode_cbor, decode_events, decode_json,
};
use std::sync::Arc;

pub struct OperationsBuilder {
    pub(crate) operations: Operations,
}

impl OperationsBuilder {
    /// Registers `O`, served by `call` on `implementation`. Generated code
    /// calls this; a service registers through the generated setters.
    ///
    /// `call` returns the implementation's future as it is; the framework
    /// boxes it once, together with turning its output into the response.
    pub fn add<O, S, Fut>(
        mut self,
        implementation: Arc<S>,
        call: impl Fn(Arc<S>, O::Input) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        O: Operation,
        S: Send + Sync + 'static,
        Fut: Future<Output = Result<O::Output>> + Send + 'static,
    {
        let invoke: Invoke = Arc::new(move |input: Box<dyn Input>| {
            let input = match input.into_any().downcast::<O::Input>() {
                Ok(input) => *input,
                Err(_) => {
                    let error =
                        Error::internal(format!("{}: input of the wrong type", O::INFO.name));
                    return Box::pin(std::future::ready(Err(error)));
                }
            };
            let output = call(implementation.clone(), input);
            Box::pin(
                async move { Ok(Box::new(StreamingOutput::<O>(output.await?)) as Box<dyn Output>) },
            )
        });
        self.operations.entries.insert(
            O::INFO.name,
            Entry {
                info: O::INFO,
                metadata: OperationMetadata::of::<O>(),
                decode: decode::<O>,
                decode_json: decode_json::<O>,
                decode_cbor: decode_cbor::<O>,
                decode_bound: decode_bound::<O>,
                decode_events: decode_events::<O>,
                invoke,
                body_limit: None,
            },
        );
        self
    }

    /// The table, or the operations the service has not implemented.
    pub fn build(mut self) -> std::result::Result<Operations, MissingOperations> {
        for entry in self.operations.entries.values() {
            let info = entry.info;
            if info
                .path
                .iter()
                .any(|s| matches!(s, Segment::GreedyLabel(_)))
            {
                self.operations.greedy_routes.push(info);
                continue;
            }
            let routes = &mut self.operations.routes;
            if routes.len() <= info.path.len() {
                routes.resize_with(info.path.len() + 1, Vec::new);
            }
            routes[info.path.len()].push(info);
        }
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
