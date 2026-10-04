//! Pages of a `@paginated` operation, one request each.

use std::marker::PhantomData;

use crucible_wire::operation::Operation;
use serde::de::DeserializeOwned;

use crate::transport::{Error, OperationErrors, Transport};

/// Sends the operation again with each response's token until a response
/// has none. Generated `*_pages` methods build it.
pub struct Paginator<O: Operation, E> {
    transport: Transport,
    /// The next request; `None` once the last page is in.
    next: Option<O::Input>,
    token: fn(&O::Output) -> Option<&str>,
    input_token: fn(&O::Input) -> Option<&str>,
    set_token: fn(&mut O::Input, String),
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
            next: Some(input),
            token,
            input_token,
            set_token,
            errors: PhantomData,
        }
    }

    /// The next page, or `None` after the last. A failed page ends the
    /// paging: retries have already run inside the call.
    pub async fn next_page(&mut self) -> Option<Result<O::Output, Error<E>>> {
        let input = self.next.take()?;
        let output = match self.transport.send::<O, E>(&input).await {
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
