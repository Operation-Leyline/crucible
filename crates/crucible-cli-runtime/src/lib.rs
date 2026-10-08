//! Shared behavior for generated service executables. HTTP encoding, retries,
//! authentication overrides, and pagination remain in the generated SDK.

mod command;
mod mcp;

#[doc(hidden)]
pub mod __generated {
    pub use crate::mcp::serve;
    pub mod mcp {
        pub use crucible_wire::mcp::{ToolDescriptor, ToolError};
    }
}
mod error;
mod input;
mod invocation;
mod output;
mod profile;
mod spec;

use std::ffi::OsString;

pub use error::{Error, report};
pub use invocation::Invocation;
pub use output::{Output, write_json};

/// Parse command-line input. `None` means an offline command (help, version,
/// completion, or input template) has already written its output.
pub fn parse(
    spec_json: &str,
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<Invocation>, Error> {
    let spec: spec::Spec = serde_json::from_str(spec_json).map_err(Error::input)?;
    let mut command = command::build(&spec)?;
    let matches = match command.clone().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            error.print().map_err(Error::request)?;
            return Ok(None);
        }
        Err(error) => return Err(Error::input(error)),
    };
    if let Some(shell) = matches.get_one::<String>("generate-completion") {
        let shell: clap_complete::Shell = shell.parse().map_err(Error::input)?;
        clap_complete::generate(shell, &mut command, &spec.name, &mut std::io::stdout());
        return Ok(None);
    }
    let Some((name, operation_matches)) = matches.subcommand() else {
        command.print_help().map_err(Error::request)?;
        return Ok(None);
    };
    if name == "mcp" {
        return Ok(Some(Invocation {
            operation: "mcp".into(),
            input: serde_json::Value::Null,
            config: profile::resolve(&spec, &matches)?,
            paginate: false,
            output: Output::Json,
        }));
    }
    let operation = spec
        .operations
        .iter()
        .find(|operation| operation.name == name)
        .ok_or_else(|| Error::input("unknown operation"))?;
    if operation_matches.get_flag("generate-input") {
        write_json(&input::skeleton(&spec, operation)?)?;
        return Ok(None);
    }
    let input = input::request(&spec, operation, operation_matches)?;
    let config = profile::resolve(&spec, &matches)?;
    let output = match matches.get_one::<String>("output").map(String::as_str) {
        Some("jsonl") => Output::Jsonl,
        _ => Output::Json,
    };
    Ok(Some(Invocation {
        operation: operation.operation.clone(),
        input,
        config,
        paginate: operation.paginated && !operation_matches.get_flag("no-paginate"),
        output,
    }))
}
