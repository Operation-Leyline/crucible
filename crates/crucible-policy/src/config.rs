use crate::{Compatibility, Exception, RULE_IDS, Rules};
use regex::Regex;
use serde::Deserialize;
use std::fmt;
use std::ops::Range;
use std::path::Path;

/// API policy, read from `crucible.toml`. The file may also be a `.cru`
/// package's manifest: its `[model]` table belongs to the compiler, and
/// policy beside it applies to that package.
#[derive(Debug, Clone)]
pub struct Config {
    pub schema_version: u32,
    pub rules: Rules,
    pub compatibility: Compatibility,
    pub exceptions: Vec<Exception>,
}

/// The file as written: policy, and the package manifest it may share.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    /// Optional, so a policy written below a package's `[model]` table
    /// needs no top-level key that TOML would file under that table.
    #[serde(default = "first_version")]
    schema_version: u32,
    #[serde(default)]
    rules: Rules,
    #[serde(default)]
    compatibility: Compatibility,
    #[serde(default)]
    exceptions: Vec<Exception>,
    /// The package manifest, checked by `crucible-lang`.
    #[serde(default)]
    #[allow(dead_code)]
    model: serde::de::IgnoredAny,
}

fn first_version() -> u32 {
    1
}

/// The top-level keys that make a `crucible.toml` carry policy.
pub const POLICY_KEYS: &[&str] = &["schema_version", "rules", "compatibility", "exceptions"];

/// Why a policy did not load, and where in the file, when that is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub message: String,
    /// Byte range in the text the policy was parsed from.
    pub span: Option<Range<usize>>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            rules: Rules::default(),
            compatibility: Compatibility::default(),
            exceptions: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        Self::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        Self::parse_located(text).map_err(|error| error.message)
    }

    /// Whether `text`, a `crucible.toml`, carries policy at all. A package
    /// manifest without any is not a policy file.
    pub fn declares_policy(text: &str) -> bool {
        text.parse::<toml::Table>()
            .is_ok_and(|table| POLICY_KEYS.iter().any(|key| table.contains_key(*key)))
    }

    /// [`Config::parse`], with the location of the problem for an editor.
    pub fn parse_located(text: &str) -> Result<Self, ConfigError> {
        let document: Document = toml::from_str(text).map_err(|error| ConfigError {
            message: error.message().to_owned(),
            span: error.span(),
        })?;
        let config = Self {
            schema_version: document.schema_version,
            rules: document.rules,
            compatibility: document.compatibility,
            exceptions: document.exceptions,
        };
        // Problems found after reading point at the value they are about.
        let at = |value: &str, message: String| ConfigError {
            message,
            span: locate(text, value),
        };
        if config.schema_version != 1 {
            return Err(ConfigError {
                message: format!(
                    "unsupported policy schema_version {}; this version reads 1",
                    config.schema_version
                ),
                span: locate_key(text, "schema_version"),
            });
        }
        if let Some(rule) = &config.rules.naming {
            for pattern in [
                &rule.operations_pattern,
                &rule.shapes_pattern,
                &rule.members_pattern,
                &rule.enum_values_pattern,
            ]
            .into_iter()
            .flatten()
            {
                naming_pattern(&pattern.value).map_err(|error| {
                    at(
                        &pattern.value,
                        format!("invalid naming pattern {:?}: {error}", pattern.value),
                    )
                })?;
            }
        }
        if let Some(prefix) = config.rules.paths.as_ref().and_then(|r| r.prefix.as_ref()) {
            let prefix = &prefix.value;
            if !prefix.starts_with('/')
                || prefix.contains(['?', '#', '{', '}'])
                || (prefix.len() > 1 && prefix.ends_with('/'))
            {
                return Err(at(
                    prefix,
                    "paths.prefix must be an absolute literal path without a trailing slash".into(),
                ));
            }
        }
        for exception in &config.exceptions {
            if !RULE_IDS.contains(&exception.rule.as_str()) {
                return Err(at(
                    &exception.rule,
                    format!(
                        "unknown exception rule {:?}; the rules are {}",
                        exception.rule,
                        RULE_IDS.join(", ")
                    ),
                ));
            }
            exception
                .shape
                .parse::<crucible_smithy::ShapeId>()
                .map_err(|error| at(&exception.shape, error.to_string()))?;
            if exception.reason.trim().is_empty() {
                return Err(at(
                    &exception.reason,
                    "exceptions require a nonempty reason".into(),
                ));
            }
        }
        Ok(config)
    }
}

/// Where a string value is written, quoted either way TOML allows.
fn locate(text: &str, value: &str) -> Option<Range<usize>> {
    [format!("\"{value}\""), format!("'{value}'")]
        .iter()
        .filter_map(|quoted| text.find(quoted.as_str()).map(|at| at..at + quoted.len()))
        .min_by_key(|range| range.start)
}

/// Where a top-level key is written.
fn locate_key(text: &str, key: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with(key) && trimmed[key.len()..].trim_start().starts_with('=') {
            let start = offset + (line.len() - trimmed.len());
            return Some(start..start + key.len());
        }
        offset += line.len();
    }
    None
}

pub(crate) fn naming_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    // Validate the same whole-name expression used by the checker, including
    // patterns whose verbose-mode comments could consume the closing anchors.
    Regex::new(&format!("\\A(?:{pattern})\\z"))
}
