//! The front end of Crucible's modelling language (`docs/idl/reference.md`).
//!
//! [`parse`] turns one `.cru` file into a typed syntax tree. Lexing is
//! lossless: the returned tokens, whitespace and comments included, cover the
//! source exactly, which is what a formatter and a language server build on.
//! Parsing never stops at the first problem; every diagnostic is collected.
//! [`compile`] resolves a whole package, checks it and lowers one service to
//! `crucible_smithy::ir`, the IR every generator reads.

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

pub use compile::{
    Compiled, FileDiagnostic, Location, Naming, Package, SourceFile, SourceMap, bundled_packages,
    compile, load_baseline,
};
pub use diagnostic::{Diagnostic, Severity};
pub use format::{FormatOptions, format, format_lines};
pub use guard::{Guard, guard, versions};
pub use lexer::{Token, TokenKind};
pub use package::{check_manifest, is_package, load_package, load_package_with_manifest};
pub use span::{LineIndex, Span};

/// Loads the package in `dir`, compiles it and returns its service, or the
/// rendered diagnostics. A convenience for tools and tests that only want
/// the lowered IR of a package on disk.
pub fn load_service(dir: &std::path::Path) -> Result<crucible_smithy::ir::Service, String> {
    let package = load_package(dir)?;
    let compiled = compile(&package, None);
    match compiled.service.clone() {
        Some(service) => Ok(service),
        None => Err(compiled.render()),
    }
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
