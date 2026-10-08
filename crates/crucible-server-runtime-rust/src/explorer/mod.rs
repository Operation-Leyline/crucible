//! An opt-in local-development API explorer, served on every HTTP host.
pub(crate) mod description;
mod explorer;

pub use description::Description;
pub use explorer::Explorer;
