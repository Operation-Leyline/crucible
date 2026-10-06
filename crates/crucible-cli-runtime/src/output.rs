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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Serialize, Serializer, ser::Error as SerError};

    struct FailingType;

    impl Serialize for FailingType {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("serialization failed"))
        }
    }

    #[test]
    fn test_write_json_serialization_error() {
        let result = write_json(&FailingType);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.to_string(), "serialization failed");
        assert_eq!(err.exit_code(), 1);
    }
}
