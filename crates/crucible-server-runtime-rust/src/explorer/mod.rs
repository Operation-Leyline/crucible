//! An opt-in local-development API explorer, served on every HTTP host.
pub(crate) mod description;
#[path = "explorer.rs"]
mod endpoint;

pub use description::Description;
pub use endpoint::Explorer;
