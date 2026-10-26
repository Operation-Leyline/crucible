//! An SSH boundary for local Git fixtures: stdin/stdout are the real Git protocol.

use std::path::PathBuf;
use std::process::{Command, exit};

fn main() {
    let command = std::env::args().next_back().expect("SSH command");
    std::fs::write(std::env::var_os("SDK_SSH_LOG").unwrap(), &command).unwrap();
    let path = command
        .strip_prefix("git-upload-pack '")
        .and_then(|path| path.strip_suffix('\''))
        .expect("Git upload-pack command");
    let root = PathBuf::from(std::env::var_os("SDK_SSH_ROOT").unwrap());
    // An SSH server resolves relative paths from the login user's home.
    // Keep the absolute-path repository distinct so a wrong URL fetches a
    // different package instead of passing by a string comparison.
    let repository = if let Some(path) = path.strip_prefix("~/") {
        root.join("home").join(path)
    } else if let Some(path) = path.strip_prefix('/') {
        root.join("absolute").join(path)
    } else {
        root.join("home").join(path)
    };
    let status = Command::new("git")
        .arg("upload-pack")
        .arg(repository)
        .status()
        .unwrap();
    exit(status.code().unwrap_or(1));
}
