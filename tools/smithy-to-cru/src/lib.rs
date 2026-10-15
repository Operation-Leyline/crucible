//! One-shot migration tool: a Smithy JSON AST to a Crucible language package,
//! and the proof that the package means the same thing.
//!
//! This tool exists for the move of Leyline (and Crucible's own Smithy test
//! fixtures) to `.cru`. It is removed together with Smithy support, so it is
//! not part of Crucible's public surface: no CLI subcommand, no published
//! crate.
//!
//! [`convert`] writes the package; [`check`] compiles it with `crucible-lang`,
//! lowers the Smithy model with `crucible-smithy`, and reports every
//! difference between the two IRs. Differences that are deliberate front-end
//! differences (decisions 20, 26, 34a, 63) are listed as expected; anything
//! else is a converter bug or a construct `.cru` cannot say.

pub mod check;
pub mod convert;
mod model;
mod text;

pub use check::{Difference, Report, check};
pub use convert::{Converted, Diagnostic, Options, Severity, convert};
