//! The front end of Crucible's modelling language (`docs/idl/reference.md`).
//!
//! [`parse`] turns one `.cru` file into a typed syntax tree. Lexing is
//! lossless: the returned tokens, whitespace and comments included, cover the
//! source exactly, which is what a formatter and a language server build on.
//! Parsing never stops at the first problem; every diagnostic is collected.
//! [`compile`] resolves a whole package, checks it and lowers one service to
//! `crucible_ir::ir`, the IR every generator reads.

pub mod ast;
pub mod compile;
pub mod diagnostic;
pub mod format;
pub mod guard;
pub mod lexer;
mod literal;
mod lower;
pub mod package;
mod parser;
pub mod release;
mod rules;
pub mod span;
mod templates;

pub use compile::{
    Compiled, FileDiagnostic, Location, Naming, Package, SourceFile, SourceMap, bundled_packages,
    compile, load_baseline,
};
pub use diagnostic::{Diagnostic, Severity};
pub use format::{FormatOptions, format, format_lines};
pub use guard::{Checked, Guard, Versions, check_in, guard, guard_in, versions, versions_in};
pub use lexer::{Token, TokenKind};
pub use package::{check_manifest, is_package, load_package, load_package_with_manifest};
pub use release::Releases;
pub use span::{LineIndex, Span};

/// Loads the package in `dir`, compiles it and returns its service, or the
/// rendered diagnostics. A convenience for tools and tests that only want
/// the lowered IR of a package on disk.
pub fn load_service(dir: &std::path::Path) -> Result<crucible_ir::ir::Service, String> {
    let package = load_package(dir)?;
    let compiled = compile(&package, None);
    match compiled.service.clone() {
        Some(service) => Ok(service),
        None => Err(compiled.render()),
    }
}

/// [`load_service`], with the older releases the service still serves in
/// its `versions`, as a build lowers it: the chain of migrations back from
/// its `version`, under the policy its `crucible.toml` declares beside
/// `[model]`, after its compatibility guard, each release projected and
/// checked. For tools and tests that want what the reference documents.
pub fn load_versioned_service(dir: &std::path::Path) -> Result<crucible_ir::ir::Service, String> {
    let package = load_package(dir)?;
    let compiled = compile(&package, None);
    let Some(mut service) = compiled.service.clone() else {
        return Err(compiled.render());
    };
    let manifest = dir.join("crucible.toml");
    let config = match std::fs::read_to_string(&manifest) {
        Ok(text) if crucible_policy::Config::declares_policy(&text) => {
            crucible_policy::Config::load(&manifest)?
        }
        _ => crucible_policy::Config::default(),
    };
    // The guard first, as a build runs it: a break no migration declares is
    // refused, not documented. One `Releases` serves it and the versions.
    let releases = Releases::default();
    let checked = check_in(&package, &compiled, package.files.len(), &config, &releases);
    if let Some(failure) = checked.failure(dir, &compiled) {
        return Err(failure);
    }
    service.versions = checked.versions.versions;
    Ok(service)
}

/// One parsed file: its syntax tree, every token, and every problem found.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub file: ast::File,
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses one source file. Always returns a tree; check `diagnostics`.
pub fn parse(source: &str) -> Parsed {
    if source.len() > u32::MAX as usize {
        return Parsed {
            file: ast::File::default(),
            tokens: Vec::new(),
            diagnostics: vec![Diagnostic::new(
                Span::default(),
                "a model file must be smaller than 4 GiB",
            )],
        };
    }
    let (tokens, mut diagnostics) = lexer::lex(source);
    let mut parser = parser::Parser::new(source, &tokens);
    let file = parser.file();
    diagnostics.append(&mut parser.diagnostics);
    diagnostics.sort_by_key(|diagnostic| (diagnostic.span.start, diagnostic.span.end));
    Parsed {
        file,
        tokens,
        diagnostics,
    }
}
