//! `crucible sdk`: the SDKs a service declares in `crucible-sdk.toml`
//! (docs/sdk-packaging.md). `config` prints their effective configuration
//! and `generate` writes the development exports. Both run only at the
//! Crucible revision the service's `Cargo.lock` pins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use crucible_codegen::sdk::{self, Channel, SdkFile};

#[derive(Args)]
pub(crate) struct SdkArgs {
    /// The SDK file. Defaults to crucible-sdk.toml in this directory or its
    /// nearest parent.
    #[arg(long, global = true)]
    file: Option<PathBuf>,
    #[command(subcommand)]
    command: SdkCommand,
}

#[derive(Subcommand)]
enum SdkCommand {
    /// Print each declared SDK's effective configuration.
    Config {
        /// Only this SDK; repeat for more. Defaults to every declared SDK.
        #[arg(long = "sdk", value_name = "NAME")]
        sdks: Vec<String>,
        /// Print JSON: the configuration and its digest per SDK.
        #[arg(long)]
        json: bool,
    },
    /// Write each declared SDK to its `out`, at the development version.
    Generate {
        /// Only this SDK; repeat for more. Defaults to every declared SDK.
        #[arg(long = "sdk", value_name = "NAME")]
        sdks: Vec<String>,
    },
}

pub(crate) fn run(args: SdkArgs) -> Result<(), String> {
    let path = match args.file {
        Some(path) => path,
        None => {
            let here = std::env::current_dir().map_err(|error| error.to_string())?;
            sdk::find(&here).ok_or_else(|| {
                format!(
                    "no {} in {} or its parents up to the repository root; --file names one",
                    sdk::FILE_NAME,
                    here.display()
                )
            })?
        }
    };
    let path = std::path::absolute(&path).map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let root = path.parent().unwrap_or(Path::new("."));
    // A checkout with uncommitted changes has no verified revision.
    if sdk::check_revision(root, crucible_revision::verified_revision())?.is_none() {
        eprintln!(
            "warning: no Cargo.lock in {} or its parents up to the repository root, so the Crucible revision is not checked",
            root.display()
        );
    }
    // The whole file is checked; only the selected SDKs' environment is read.
    let mut file = sdk::load(&path).map_err(|errors| errors.to_string())?;
    let names = match &args.command {
        SdkCommand::Config { sdks, .. } | SdkCommand::Generate { sdks } => sdks.clone(),
    };
    file.select(&names)?;
    file.resolve_endpoints(&names, |name| std::env::var(name).ok())
        .map_err(|errors| errors.to_string())?;
    match args.command {
        SdkCommand::Config { sdks, json } => config(&file, &sdks, json),
        SdkCommand::Generate { sdks } => generate(&file, &sdks),
    }
}

fn config(file: &SdkFile, names: &[String], json: bool) -> Result<(), String> {
    let selected = file.select(names)?;
    if json {
        let mut value = file.to_json();
        if !names.is_empty() {
            let sdks = value["sdks"]
                .as_object_mut()
                .expect("the configuration lists its SDKs");
            sdks.retain(|name, _| names.contains(name));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&value).expect("JSON serializes")
        );
        return Ok(());
    }
    println!("{}", file.path.display());
    println!("  model {}", file.model);
    if let Some(service) = &file.service {
        println!("  service {service}");
    }
    for sdk in selected {
        let effective = sdk.effective(file);
        println!();
        println!("[sdk.{}] {}", sdk.name, sdk.language.display());
        let mut rows: BTreeMap<&str, String> = BTreeMap::new();
        for (key, value) in effective.as_object().expect("an object") {
            if matches!(
                key.as_str(),
                "name" | "language" | "model" | "service" | "channel"
            ) {
                continue;
            }
            rows.insert(
                key,
                match value {
                    serde_json::Value::String(text) => text.clone(),
                    other => other.to_string(),
                },
            );
        }
        if let Some(channel) = &sdk.channel {
            rows.insert(
                "channel",
                match channel {
                    Channel::Npm { url, .. } => format!("npm {url}"),
                    Channel::Git { url, tag, .. } => format!("git {url} ({tag})"),
                },
            );
        }
        rows.insert("config_sha256", sdk.config_sha256(file));
        let width = rows.keys().map(|key| key.len()).max().unwrap_or(0);
        for (key, value) in rows {
            println!("  {key:width$}  {value}");
        }
    }
    Ok(())
}

fn generate(file: &SdkFile, names: &[String]) -> Result<(), String> {
    let selected = file.select(names)?;
    let source = crate::Source {
        model: file.join(&file.model),
        service: file
            .service
            .as_ref()
            .map(|id| id.parse().expect("the resolver checked the service ID")),
        protocol: None,
    };
    // Lowered once with every capability; each SDK then sees its audience
    // and checks its own language's capabilities.
    let all = crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
    let full = crate::lower_with_capabilities(&source, &all)?;
    for sdk in selected {
        if !sdk.takes_metadata() && has_common_keys(sdk) {
            eprintln!(
                "warning: sdk.{}: {} packages take the common metadata keys in a later release; they are recorded in its configuration only",
                sdk.name,
                sdk.language.display()
            );
        }
        let client = sdk.client(file, None)?;
        let stamp = crucible_codegen::provenance::Stamp {
            config_sha256: Some(sdk.config_sha256(file)),
            source: None,
        };
        let files = client
            .emit_with_provenance(&full, sdk.audience, &stamp)
            .map_err(|diagnostics| format!("cannot generate sdk.{}:\n{diagnostics}", sdk.name))?;
        let out = file.join(&sdk.out);
        crucible_codegen::write(&out, &files)
            .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
        println!(
            "sdk.{}: wrote {} files to {}",
            sdk.name,
            files.len(),
            sdk.out
        );
    }
    Ok(())
}

/// Whether the SDK sets a common key beyond `private` and `provenance`,
/// which every language honors through its channel rules.
fn has_common_keys(sdk: &sdk::Sdk) -> bool {
    let metadata = &sdk.metadata;
    metadata.description.is_some()
        || metadata.repository.is_some()
        || metadata.homepage.is_some()
        || metadata.documentation.is_some()
        || metadata.license.is_some()
        || metadata.license_file.is_some()
        || !metadata.authors.is_empty()
        || metadata.readme.is_some()
        || !metadata.keywords.is_empty()
}
