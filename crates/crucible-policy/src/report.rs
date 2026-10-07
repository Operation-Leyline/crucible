use std::fmt;

use serde::Serialize;

use crate::{Config, Finding, Severity};

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.severity == Severity::Error && finding.exception.is_none())
    }

    pub(crate) fn push(
        &mut self,
        rule: &str,
        shape: impl ToString,
        severity: Severity,
        message: impl Into<String>,
    ) {
        if severity != Severity::Off {
            self.findings.push(Finding {
                rule: rule.into(),
                shape: shape.to_string(),
                severity,
                message: message.into(),
                exception: None,
            });
        }
    }

    pub(crate) fn finish(mut self, config: &Config) -> Self {
        for finding in &mut self.findings {
            if let Some(exception) = config.exceptions.iter().find(|exception| {
                exception.rule == finding.rule && exception.shape == finding.shape
            }) {
                finding.exception = Some(exception.reason.clone());
            }
        }
        self.findings
            .sort_by(|a, b| (&a.shape, &a.rule, &a.message).cmp(&(&b.shape, &b.rule, &b.message)));
        self.findings
            .dedup_by(|a, b| a.shape == b.shape && a.rule == b.rule && a.message == b.message);
        self
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for finding in &self.findings {
            let severity = match finding.severity {
                Severity::Off => "off",
                Severity::Warning => "warning",
                Severity::Error => "error",
            };
            write!(
                f,
                "{severity}[{}] {}: {}",
                finding.rule, finding.shape, finding.message
            )?;
            if let Some(reason) = &finding.exception {
                write!(f, " (excepted: {reason})")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
