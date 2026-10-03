use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use crucible_core::ir::{Service, Shape, Traits};
use crucible_core::{Model, ShapeId};

/// Generate servers and clients from a Smithy model.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load a model and report every construct crucible cannot generate from.
    Check {
        /// A Smithy JSON AST, built with the flattenAndRemoveMixins transform.
        model: PathBuf,
        /// The service to lower, when the model defines several.
        #[arg(long)]
        service: Option<ShapeId>,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Check { model, service } => check(&model, service.as_ref()),
    }
}

fn check(path: &PathBuf, service: Option<&ShapeId>) -> ExitCode {
    let lowered = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
        .and_then(|json| Model::from_json(&json).map_err(|error| error.to_string()))
        .and_then(|model| model.lower(service).map_err(|error| error.to_string()));
    match lowered {
        Ok(service) => {
            print!("{}", summary(&service));
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn summary(service: &Service) -> String {
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for shape in service.shapes.values() {
        let kind = match shape {
            Shape::Unit => "unit",
            Shape::Simple(_) => "simple",
            Shape::Enum(_) => "enum",
            Shape::List(_) => "list",
            Shape::Map(_) => "map",
            Shape::Structure(_) => "structure",
            Shape::Union(_) => "union",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    let kinds: Vec<String> = kinds
        .iter()
        .map(|(kind, n)| format!("{n} {kind}"))
        .collect();
    let paginated = service
        .operations
        .iter()
        .filter(|op| op.paginated.is_some())
        .count();
    let tokens = service
        .operations
        .iter()
        .filter(|op| op.idempotency_token.is_some())
        .count();

    let mut out = format!(
        "{} {}\n  {} operations ({paginated} paginated, {tokens} with idempotency tokens)\n  {} shapes: {}\n",
        service.id,
        service.version,
        service.operations.len(),
        service.shapes.len(),
        kinds.join(", "),
    );
    let custom = custom_traits(service);
    if !custom.is_empty() {
        out.push_str("  custom traits:\n");
        for (id, uses) in custom {
            out.push_str(&format!("    {id} ×{uses}\n"));
        }
    }
    out
}

/// Traits from the model's own namespaces, which only a project plugin reads.
fn custom_traits(service: &Service) -> BTreeMap<&ShapeId, usize> {
    let standard = |id: &ShapeId| id.is_prelude() || id.namespace().starts_with("aws.");
    let mut maps: Vec<&Traits> = service.operations.iter().map(|op| &op.traits).collect();
    for shape in service.shapes.values() {
        if let Shape::Structure(structure) | Shape::Union(structure) = shape {
            maps.push(&structure.traits);
            maps.extend(structure.members.iter().map(|member| &member.traits));
        }
    }
    let mut uses = BTreeMap::new();
    for id in maps
        .into_iter()
        .flat_map(Traits::keys)
        .filter(|id| !standard(id))
    {
        *uses.entry(id).or_default() += 1;
    }
    uses
}
