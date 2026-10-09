//! Problems found while reading a model file. Every one is collected, so a
//! single run lists all of them.

use std::fmt::Write as _;

use crate::span::{LineIndex, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }

    /// `path:line:column: message`, followed by the offending line and a
    /// caret under the span.
    pub fn render(&self, path: &str, source: &str, lines: &LineIndex) -> String {
        let (line, column) = lines.line_col(source, self.span.start);
        let mut out = format!("{path}:{line}:{column}: {}\n", self.message);
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
