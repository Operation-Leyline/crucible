//! `crucible upgrade-guide` (docs/api-versions-clients.md, 5d): the
//! Markdown guide between two supported releases of a `.cru` package, as
//! `crucible_codegen_docs::upgrade_guide` writes it from the package's
//! release chain, lowered as a build lowers it.

use std::path::{Path, PathBuf};

use crucible_ir::ShapeId;

/// The command's arguments.
#[derive(clap::Args)]
pub(crate) struct Args {
    /// The package directory, holding crucible.toml with [model].
    package: PathBuf,
    /// The service, when the package declares several.
    #[arg(long)]
    service: Option<ShapeId>,
    /// The release a client moves from.
    #[arg(long, value_name = "RELEASE")]
    from: String,
    /// The release it moves to. Defaults to the current release.
    #[arg(long, value_name = "RELEASE")]
    to: Option<String>,
    /// An external guide leaves out the steps and elements of internal API.
    #[arg(long, value_enum, default_value = "external")]
    audience: crate::AudienceArg,
    /// Write the guide to this file instead of standard output.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub(crate) fn run(args: Args) -> Result<(), String> {
    let source = crate::Source {
        model: args.package,
        service: args.service,
        api_version: None,
    };
    // The chain as a build serves it: guarded, every release projected and
    // checked, then projected to the audience, release first.
    let full = crate::lower_cru(&source, true)?;
    let service = crucible_ir::project(&full, args.audience.into()).map_err(|e| e.to_string())?;
    let guide = crucible_codegen_docs::upgrade_guide(&service, &args.from, args.to.as_deref())
        .map_err(|diagnostics| diagnostics.to_string().trim_end().to_owned())?;
    match args.out {
        Some(path) => write(&path, &guide),
        None => {
            print!("{guide}");
            Ok(())
        }
    }
}

fn write(path: &Path, guide: &str) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, guide).map_err(|error| format!("cannot write {}: {error}", path.display()))
}
