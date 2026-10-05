use std::io::Write;

use serde::Serialize;

use crate::Error;

/// JSON emits a single document; JSON Lines can stream paginated responses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Json,
    Jsonl,
}

pub fn write_json(value: &impl Serialize) -> Result<(), Error> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value).map_err(Error::request)?;
    writeln!(lock).map_err(Error::request)
}
