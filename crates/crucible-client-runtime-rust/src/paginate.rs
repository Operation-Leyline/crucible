//! Pages of a `@paginated` operation, one request each.

use std::marker::PhantomData;

use crucible_wire::operation::Operation;
use serde::de::DeserializeOwned;

use crate::transport::{Error, OperationErrors, Transport};
use crate::{CallOptions, OperationMetadata};

/// Sends the operation again with each response's token until a response
/// has none. Generated `*_pages` methods build it.
pub struct Paginator<O: Operation, E> {
    transport: Transport,
    metadata: Option<&'static OperationMetadata>,
    /// The next request; `None` once the last page is in.
    next: Option<O::Input>,
    token: fn(&O::Output) -> Option<&str>,
    input_token: fn(&O::Input) -> Option<&str>,
    set_token: fn(&mut O::Input, String),
    /// Fills the idempotency token afresh before each page, when the caller
    /// left it empty: each page is its own request.
    fresh_token: Option<fn(&mut O::Input)>,
    errors: PhantomData<fn() -> E>,
}

impl<O, E> Paginator<O, E>
where
    O: Operation,
    O::Input: Clone,
    O::Output: DeserializeOwned,
    E: OperationErrors,
{
    #[doc(hidden)]
    pub fn new(
        transport: Transport,
        input: O::Input,
        token: fn(&O::Output) -> Option<&str>,
        input_token: fn(&O::Input) -> Option<&str>,
        set_token: fn(&mut O::Input, String),
    ) -> Self {
        Self {
            transport,
            metadata: None,
            next: Some(input),
            token,
            input_token,
            set_token,
            fresh_token: None,
            errors: PhantomData,
        }
    }

    #[doc(hidden)]
    pub fn metadata(mut self, metadata: &'static OperationMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    #[doc(hidden)]
    pub fn fresh_token(mut self, fill: fn(&mut O::Input)) -> Self {
        self.fresh_token = Some(fill);
        self
    }

    /// The next page, or `None` after the last. A failed page ends the
    /// paging: retries have already run inside the call.
    pub async fn next_page(&mut self) -> Option<Result<O::Output, Error<E>>> {
        let mut input = self.next.take()?;
        if let Some(fill) = self.fresh_token {
            fill(&mut input);
        }
        let result = match self.metadata {
            Some(metadata) => {
                self.transport
                    .send_with_metadata::<O, E>(&input, &CallOptions::default(), metadata)
                    .await
            }
            None => self.transport.send::<O, E>(&input).await,
        };
        let output = match result {
            Ok(output) => output,
            Err(error) => return Some(Err(error)),
        };
        // A service that hands back the token it was sent would page forever.
        let token = (self.token)(&output)
            .filter(|token| !token.is_empty() && Some(*token) != (self.input_token)(&input))
            .map(str::to_owned);
        if let Some(token) = token {
            let mut next = input;
            (self.set_token)(&mut next, token);
            self.next = Some(next);
        }
        Some(Ok(output))
    }

    /// Every page, in order; stops at the first error.
    pub async fn collect(mut self) -> Result<Vec<O::Output>, Error<E>> {
        let mut pages = Vec::new();
        while let Some(page) = self.next_page().await {
            pages.push(page?);
        }
        Ok(pages)
    }
}
