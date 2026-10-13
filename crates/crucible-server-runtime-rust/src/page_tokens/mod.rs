//! Sealed pagination tokens.
//!
//! A `@paginated` operation hands its caller a token that says where the
//! listing resumes. Whatever the service puts there (a database's last key,
//! an offset) is its own business, and a token that carries it in the clear
//! lets a caller read it, edit it, replay it against another listing, or keep
//! it forever. [`PageTokens`] seals the service's cursor instead:
//!
//! - **Opaque and tamper-proof.** XChaCha20-Poly1305: a caller can neither
//!   read the cursor nor change a byte of the token without it being refused.
//! - **Bound to the request.** The seal covers a digest of the service, the
//!   operation, the caller and every input member except the token and the
//!   page size, so a token opens only for the same caller, listing and
//!   filters. Nothing about the binding travels in the token.
//! - **Expiring.** The expiry is sealed inside (24 hours by default).
//! - **Versioned and rotatable.** The token names its format and its key, and
//!   [`PageKeys`] can accept retired keys while sealing with the current one.
//!
//! Nothing is stored: a token is checked by opening it. A token that does not
//! open is the service's validation error naming the token member, unless
//! [`PageTokens::refusal`] says otherwise; the caller's remedy is always to
//! start the listing again.
//!
//! An operation reads the cursor it was asked to resume from with [`resume`]
//! and seals the next one with [`issue`]:
//!
//! ```ignore
//! let start = page_tokens::resume()?;          // None on the first page
//! let (items, last) = store.page(start.as_deref(), size).await?;
//! let next_token = last.map(|key| page_tokens::issue(&key)).transpose()?;
//! ```

mod page_key_error;
mod page_keys;
#[allow(clippy::module_inception)]
mod page_tokens;
mod scope;
mod token;

pub use page_key_error::PageKeyError;
pub use page_keys::PageKeys;
pub use page_tokens::PageTokens;

use crate::{Context, Error, Result};
use scope::PageScope;

/// The cursor the request's token was sealed around, or `None` when the
/// request carries no token (the first page).
///
/// The token was opened, and refused if it did not open, before the
/// operation ran. An error means [`PageTokens`] is not installed or the
/// operation is not `@paginated`: a wiring bug, not a bad request.
pub fn resume() -> Result<Option<Vec<u8>>> {
    Ok(current()?.resumed().map(<[u8]>::to_vec))
}

/// Seals `cursor` as the token for the next page of this listing, bound to
/// this request's caller, operation and filters.
pub fn issue(cursor: &[u8]) -> Result<String> {
    current()?.issue(cursor)
}

fn current() -> Result<PageScope> {
    Context::try_current()
        .and_then(|context| context.attribute::<PageScope>())
        .ok_or_else(|| {
            Error::internal(
                "no page token scope on the request: install PageTokens, and call this from a @paginated operation",
            )
        })
}
