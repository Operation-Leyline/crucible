use std::sync::Arc;

use super::token::Sealer;
use crate::{Error, Result};

/// One request's page token state, left on the request context by
/// [`super::PageTokens`] for [`super::resume`] and [`super::issue`].
#[derive(Clone)]
pub(crate) struct PageScope {
    sealer: Arc<Sealer>,
    binding: [u8; 32],
    resumed: Option<Arc<[u8]>>,
}

impl PageScope {
    pub(crate) fn new(sealer: Arc<Sealer>, binding: [u8; 32], resumed: Option<Vec<u8>>) -> Self {
        Self {
            sealer,
            binding,
            resumed: resumed.map(Arc::from),
        }
    }

    pub(crate) fn resumed(&self) -> Option<&[u8]> {
        self.resumed.as_deref()
    }

    pub(crate) fn issue(&self, cursor: &[u8]) -> Result<String> {
        self.sealer
            .seal(&self.binding, cursor)
            .ok_or_else(|| Error::internal("a page token could not be sealed"))
    }
}
