//! Problems found while reading a model file. Every one is collected, so a
//! single run lists all of them.

use std::fmt::Write as _;

use crate::span::{LineIndex, Span};

/// Whether a diagnostic stops compilation. Warnings report something
/// probably unintended (decisions 33, 41, 47f) without withholding the IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub severity: Severity,
}

impl Diagnostic {
    /// An error: the package does not compile.
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            severity: Severity::Error,
        }
    }

    /// A warning: reported, but the package still compiles.
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            severity: Severity::Warning,
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// `path:line:column: message`, followed by the offending line and a
    /// caret under the span. Warnings say so.
    pub fn render(&self, path: &str, source: &str, lines: &LineIndex) -> String {
        let (line, column) = lines.line_col(source, self.span.start);
        let prefix = match self.severity {
            Severity::Error => "",
            Severity::Warning => "warning: ",
        };
        let mut out = format!("{path}:{line}:{column}: {prefix}{}\n", self.message);
        if let Some(text) = source.lines().nth(line - 1) {
            // Underline to the span's end or the line's end, whichever is first.
            let width = self
                .span
                .text(source)
                .chars()
                .take_while(|c| *c != '\n')
                .count()
                .max(1);
            let _ = writeln!(out, "  {text}");
            let _ = writeln!(out, "  {}{}", " ".repeat(column - 1), "^".repeat(width));
        }
        out
    }
}
