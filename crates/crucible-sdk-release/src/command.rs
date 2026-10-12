//! The native tools a channel runs (`git`, `npm`): argument arrays, never a
//! shell, behind a trait so tests can record or fake them.

use std::fmt;
use std::io;
use std::process::{Command, Stdio};

/// One command: a program, its arguments and extra environment. Never
/// holds a credential, so it can always be printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl CommandLine {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }
}

/// The command as a POSIX shell would read it, for `publish`'s dry run.
impl fmt::Display for CommandLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&quote(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", quote(arg))?;
        }
        Ok(())
    }
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | '=' | '+' | ',')
        });
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What a finished command left behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    /// Standard error, trimmed, for an error message.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).trim().to_owned()
    }
}

/// Runs commands. [`SystemRunner`] runs them for real.
pub trait CommandRunner {
    /// Runs `command` to completion with no standard input. A command that
    /// ran and failed is `Ok` with `success: false`; `Err` means it could
    /// not be started.
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput>;
}

/// Runs commands with [`std::process::Command`].
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        let output = Command::new(&command.program)
            .args(&command.args)
            .envs(command.env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot run {}: {error}", command.program),
                )
            })?;
        Ok(CommandOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

impl<T: CommandRunner + ?Sized> CommandRunner for &T {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        (**self).run(command)
    }
}

impl<T: CommandRunner + ?Sized> CommandRunner for std::rc::Rc<T> {
    fn run(&self, command: &CommandLine) -> io::Result<CommandOutput> {
        (**self).run(command)
    }
}

#[cfg(test)]
mod tests {
    use super::CommandLine;

    #[test]
    fn commands_print_as_a_shell_would_read_them() {
        let command = CommandLine::new("npm")
            .args(["publish", "/tmp/a b.tgz", "--tag", "latest"])
            .arg("it's");
        assert_eq!(
            command.to_string(),
            r"npm publish '/tmp/a b.tgz' --tag latest 'it'\''s'"
        );
        assert_eq!(CommandLine::new("git").arg("").to_string(), "git ''");
    }
}
