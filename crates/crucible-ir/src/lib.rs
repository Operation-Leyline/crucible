//! The IR every Crucible emitter reads, and the release snapshot format.
//!
//! The `.cru` front end (`crucible-lang`) lowers a service to [`ir::Service`];
//! this crate owns that type, the checks that run on it and its on-disk form.

mod audience;
mod capabilities;
pub mod decimal;
mod diagnostics;
pub mod doc_links;
mod doc_notes;
mod extensions;
pub mod ir;
mod lower_examples;
mod names;
mod operation_intent;
mod pattern;
mod protocol;
mod recursion;
mod shape_id;
pub mod snapshot;
mod timestamp_default;
pub mod validate;
pub mod versions;

pub use audience::{Audience, client_view, project};
pub use capabilities::{Capabilities, Feature, FeatureUse};
pub use diagnostics::{Diagnostic, Diagnostics};
pub use protocol::{Protocol, ProtocolSettings};
pub use shape_id::{PRELUDE, ShapeId, ShapeIdError};
