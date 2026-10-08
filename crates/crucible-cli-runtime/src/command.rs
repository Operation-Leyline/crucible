use std::collections::BTreeSet;

use clap::{Arg, ArgAction, Command};

use crate::Error;
use crate::spec::{Kind, Spec};

const RESERVED: &[&str] = &[
    "help",
    "endpoint-url",
    "profile",
    "config",
    "output",
    "generate-completion",
    "input-json",
    "generate-input",
    "no-paginate",
];

pub(crate) fn build(spec: &Spec) -> Result<Command, Error> {
    let mut command = Command::new(spec.name.clone())
        .version(spec.version.clone())
        .about(spec.description.clone())
        .arg(
            Arg::new("endpoint-url")
                .long("endpoint-url")
                .global(true)
                .help("Service endpoint URL"),
        )
        .arg(
            Arg::new("profile")
                .long("profile")
                .global(true)
                .help("Named configuration profile (default: default)"),
        )
        .arg(
            Arg::new("config")
                .long("config")
                .global(true)
                .help("Path to JSON profile configuration"),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .global(true)
                .default_value("json")
                .value_parser(["json", "jsonl"])
                .help("JSON document, or one response per line"),
        )
        .arg(
            Arg::new("generate-completion")
                .long("generate-completion")
                .global(true)
                .value_parser(["bash", "elvish", "fish", "powershell", "zsh"])
                .help("Print shell completion without making a request"),
        )
        .subcommand(Command::new("mcp").about("Serve local agents over MCP stdio"));
    let mut names = BTreeSet::new();
    for operation in &spec.operations {
        if !names.insert(&operation.name) || matches!(operation.name.as_str(), "help" | "mcp") {
            return Err(Error::input(format!(
                "duplicate or reserved command {}",
                operation.name
            )));
        }
        let mut subcommand = Command::new(operation.name.clone()).about(operation.description.clone())
            .arg(Arg::new("input-json").long("input-json").value_name("JSON|file://PATH|-").help("Request JSON, a UTF-8 file, or standard input; explicit flags override members"))
            .arg(Arg::new("generate-input").long("generate-input").action(ArgAction::SetTrue).help("Print a request template without making a request"))
            .arg(Arg::new("no-paginate").long("no-paginate").action(ArgAction::SetTrue).help("Return only the first page"));
        let mut flags = BTreeSet::new();
        for member in spec.members(operation)? {
            if RESERVED.contains(&member.flag.as_str()) || !flags.insert(&member.flag) {
                return Err(Error::input(format!(
                    "duplicate or reserved flag --{} in {}",
                    member.flag, operation.name
                )));
            }
            let kind = member.kind.unwrap_or(spec.shape(&member.target)?.kind);
            let mut arg = Arg::new(member.flag.clone())
                .long(member.flag.clone())
                .help(member.description.clone());
            arg = match kind {
                Kind::Boolean => arg
                    .num_args(0..=1)
                    .default_missing_value("true")
                    .value_name("BOOL"),
                Kind::Integer | Kind::Number => {
                    arg.allow_negative_numbers(true).value_name("NUMBER")
                }
                Kind::Structure | Kind::Union | Kind::List | Kind::Map | Kind::Document => {
                    arg.value_name("JSON")
                }
                Kind::Blob => arg.value_name("BASE64|fileb://PATH"),
                _ => arg.value_name("VALUE"),
            };
            subcommand = subcommand.arg(arg);
        }
        command = command.subcommand(subcommand);
    }
    Ok(command)
}
