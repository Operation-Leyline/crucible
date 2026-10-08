//! Every operation, one file each.

use std::collections::BTreeMap;
use std::sync::Mutex;

mod create_graph;
mod get_graph;
mod health;

/// Implements every operation. A real service holds clients here; this one
/// keeps graphs in memory because the fixture only has to run a smoke test.
#[derive(Default)]
pub struct Leyline {
    pub(crate) graphs: Mutex<BTreeMap<String, String>>,
}
