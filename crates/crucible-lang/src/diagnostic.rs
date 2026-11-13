//! Problems found while reading a model file. Every one is collected, so a
//! single run lists all of them.

use std::fmt::Write as _;

use crate::span::{LineIndex, Span};

/// Whether a diagnostic stops compilation. Warnings report something
/// probably unintended (decisions 33, 41, 47f) without withholding the IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Severity {
    Error,
    Warning,
    /// Something worth deciding that the model is silent about, such as an
    /// identifier no requirement covers (docs/permissions.md, check 18).
    Advice,
}

/// The code that starts the warning for `public`, the deprecated spelling of
/// `anonymous` on an operation and in a rule condition. Its span is the word
/// itself, so an editor's quick fix replaces the span with `anonymous`.
pub const DEPRECATED_PUBLIC: &str = "deprecated.public";

/// The warning for the deprecated `public` at `span`; `place` says where it
/// was written, such as "on an operation".
pub(crate) fn deprecated_public(span: Span, place: &str) -> Diagnostic {
    Diagnostic::warning(
        span,
        format!(
            "{DEPRECATED_PUBLIC}: `public` {place} is deprecated: write `anonymous`, which says what it means (the operation takes no credential); `public` is accepted until the next minor release"
        ),
    )
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

    /// Advice: a question for the author, never a failure.
    pub fn advice(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            severity: Severity::Advice,
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
            Severity::Advice => "advice: ",
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
