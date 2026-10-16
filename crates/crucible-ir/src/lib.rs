//! The IR every Crucible emitter reads, and the release snapshot format.
//!
//! The `.cru` front end (`crucible-lang`) lowers a service to [`ir::Service`];
//! this crate owns that type, the checks that run on it and its on-disk form.

mod audience;
mod capabilities;
mod diagnostics;
pub mod doc_links;
mod doc_notes;
mod extensions;
pub mod ir;
mod lower_examples;
mod names;
mod pattern;
mod protocol;
mod recursion;
mod shape_id;
pub mod snapshot;
mod timestamp_default;
pub mod validate;

pub use audience::{Audience, project};
pub use capabilities::{Capabilities, Feature, FeatureUse};
pub use diagnostics::{Diagnostic, Diagnostics};
pub use protocol::{Protocol, ProtocolSettings};
pub use shape_id::{ShapeId, ShapeIdError};
