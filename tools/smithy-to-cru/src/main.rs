//! `smithy-to-cru <model.json> --out <dir>` converts and checks;
//! `smithy-to-cru <model.json> --check <package>` only checks an existing
//! package against the model. Removed with Smithy support.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use smithy_to_cru::{Options, Severity, check, convert};

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool, String> {
    let mut model: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut check_dir: Option<PathBuf> = None;
    let mut service: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--out" => out = Some(value("--out")?.into()),
            "--check" => check_dir = Some(value("--check")?.into()),
            "--service" => service = Some(value("--service")?),
            "-h" | "--help" => {
                println!(
                    "usage: smithy-to-cru <model.json> (--out <dir> | --check <package>) [--service <id>]"
                );
                return Ok(true);
            }
            other if model.is_none() && !other.starts_with("--") => model = Some(other.into()),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let model =
        model.ok_or("usage: smithy-to-cru <model.json> (--out <dir> | --check <package>)")?;
    let json = std::fs::read_to_string(&model)
        .map_err(|e| format!("cannot read {}: {e}", model.display()))?;
    let mut conversion_errors = 0;
    let package: PathBuf = match (&out, &check_dir) {
        (Some(out), None) => {
            let converted = convert(
                &json,
                &Options {
                    service: service.clone(),
                },
            )?;
            write(out, &converted.files)?;
            println!("wrote {} files to {}", converted.files.len(), out.display());
            let mut errors = 0;
            for d in &converted.diagnostics {
                let tag = match d.severity {
                    Severity::Error => {
                        errors += 1;
                        "error"
                    }
                    Severity::Warning => "warning",
                    Severity::Note => "note",
                };
                println!("{tag}: {}: {}", d.shape, d.message);
            }
            println!("{errors} unrepresentable construct(s)");
            conversion_errors = errors;
            out.clone()
        }
        (None, Some(dir)) => dir.clone(),
        _ => return Err("give exactly one of --out and --check".into()),
    };
    let report = check(&json, &package, service.as_deref())?;
    print!("{}", report.render());
    for w in &report.warnings {
        println!("{w}");
    }
    Ok(report.is_clean() && conversion_errors == 0)
}

fn write(out: &Path, files: &std::collections::BTreeMap<String, String>) -> Result<(), String> {
    for (path, text) in files {
        let target = out.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    Ok(())
}
