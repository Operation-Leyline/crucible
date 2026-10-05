use std::fmt::{self, Display};
use std::process::ExitCode;

/// A command failure. Exit 2 is invalid input/configuration; exit 1 is a
/// request or output failure. No failure is printed to standard output.
#[derive(Debug)]
pub struct Error {
    message: String,
    code: u8,
}

impl Error {
    pub fn input(message: impl Display) -> Self {
        Self {
            message: message.to_string(),
            code: 2,
        }
    }

    pub fn request(message: impl Display) -> Self {
        Self {
            message: message.to_string(),
            code: 1,
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.code
    }
}

impl Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub fn report(error: Error) -> ExitCode {
    eprintln!("{error}");
    ExitCode::from(error.exit_code())
}
