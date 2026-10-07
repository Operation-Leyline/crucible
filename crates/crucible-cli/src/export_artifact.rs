use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use clap::Args as ClapArgs;
use serde_json::Value;

/// Build a Cargo model package and export one of its named artifacts.
#[derive(ClapArgs)]
pub(crate) struct Args {
    /// The artifact name declared by the producer's build script.
    pub(crate) artifact: String,
    /// The producer workspace or package manifest.
    #[arg(long, default_value = "Cargo.toml")]
    pub(crate) manifest_path: PathBuf,
    /// The workspace package that publishes the artifact.
    #[arg(long)]
    pub(crate) package: String,
    /// Destination for the generated artifact directory.
    #[arg(long)]
    pub(crate) out: PathBuf,
}

pub(crate) fn run(args: &Args) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let metadata = Command::new(&cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(&args.manifest_path)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("could not run Cargo metadata: {error}"))?;
    if !metadata.status.success() {
        return Err(format!("Cargo metadata failed ({})", metadata.status));
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout)
        .map_err(|error| format!("could not read Cargo metadata: {error}"))?;
    let workspace = metadata["workspace_members"]
        .as_array()
        .ok_or("Cargo metadata omitted workspace members")?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata omitted packages")?;
    let matches: Vec<&str> = packages
        .iter()
        .filter(|package| package["name"].as_str() == Some(args.package.as_str()))
        .filter_map(|package| package["id"].as_str())
        .filter(|id| workspace.iter().any(|member| member.as_str() == Some(id)))
        .collect();
    let package_id = match matches.as_slice() {
        [id] => *id,
        [] => return Err(format!("workspace has no package named {:?}", args.package)),
        _ => {
            return Err(format!(
                "workspace package name {:?} is ambiguous",
                args.package
            ));
        }
    };

    // Cargo reports the actual OUT_DIR, including cached build-script results.
    // Searching target/ would risk exporting a stale or unrelated producer.
    let build = Command::new(&cargo)
        .args(["build", "--manifest-path"])
        .arg(&args.manifest_path)
        .args(["--package", package_id, "--message-format=json"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("could not run Cargo build: {error}"))?;
    let mut outputs = BTreeSet::new();
    for line in build.stdout.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if message["reason"].as_str() == Some("compiler-message") {
            if let Some(rendered) = message["message"]["rendered"].as_str() {
                eprint!("{rendered}");
            }
        }
        if message["reason"].as_str() == Some("build-script-executed")
            && message["package_id"].as_str() == Some(package_id)
        {
            if let Some(path) = message["out_dir"].as_str() {
                outputs.insert(PathBuf::from(path));
            }
        }
    }
    if !build.status.success() {
        return Err(format!(
            "Cargo build for {:?} failed ({}); output was not exported",
            args.package, build.status,
        ));
    }
    let out_dir = match outputs.len() {
        1 => outputs.pop_first().expect("one Cargo build-script output"),
        0 => {
            return Err(format!(
                "package {:?} did not publish artifacts; generate named outputs in its build.rs first",
                args.package,
            ));
        }
        _ => {
            return Err(format!(
                "Cargo reported several build-script outputs for {:?}; select a single build target in Cargo configuration",
                args.package,
            ));
        }
    };
    let index = out_dir.join("crucible-artifacts.json");
    if !index.is_file() {
        return Err(format!(
            "package {:?} has no artifact registry at {}; generate named outputs in its build.rs first",
            args.package,
            index.display(),
        ));
    }
    crucible_build::export_artifact(&index, &args.artifact, &args.out)
}
