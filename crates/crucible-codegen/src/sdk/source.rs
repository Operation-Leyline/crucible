//! `source` in a release's `crucible.json`: the commit that reproduces
//! every configured input. Only `crucible sdk build` records it; development
//! exports carry none.

use std::path::Path;
use std::process::Command;

use super::{Sdk, SdkFile};
use crate::provenance::Source;

/// The Git checkout holding the SDK file, for `sdk`'s release. Every
/// configured input (the model, the SDK file, a README or license file)
/// must be tracked and unmodified, so `commit` reproduces it; otherwise this
/// fails, unless `allow_dirty`, which records `dirty`.
pub fn source(file: &SdkFile, sdk: &Sdk, allow_dirty: bool) -> Result<Source, String> {
    let root = file.root();
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let git = |args: &[&str]| -> Result<String, String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|error| format!("cannot run git: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let commit = git(&["rev-parse", "HEAD"])
        .map_err(|error| format!("{} is not in a Git checkout: {error}", file.path.display()))?
        .trim()
        .to_owned();
    let name = file
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(super::FILE_NAME)
        .to_owned();
    let mut inputs = vec![name, file.model.clone()];
    inputs.extend(sdk.metadata.readme.clone());
    inputs.extend(sdk.metadata.agents.clone());
    inputs.extend(sdk.metadata.license_file.clone());
    // One problem per input: untracked, or tracked but different from HEAD.
    let mut problems = Vec::new();
    for input in &inputs {
        if git(&["ls-files", "--error-unmatch", "--", input]).is_err() {
            problems.push(format!("{input} is not tracked"));
        } else if !git(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--",
            input,
        ])?
        .trim()
        .is_empty()
        {
            problems.push(format!("{input} is modified"));
        }
    }
    if !problems.is_empty() && !allow_dirty {
        return Err(format!(
            "sdk.{}: its source commit would not reproduce it: {}; commit them, or pass --allow-dirty",
            sdk.name,
            problems.join("; ")
        ));
    }
    let repository = git(&["remote", "get-url", "origin"])
        .ok()
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
        .and_then(|url| crucible_codegen_common::metadata::repository_url(&url));
    Ok(Source {
        repository,
        commit,
        dirty: !problems.is_empty(),
    })
}
