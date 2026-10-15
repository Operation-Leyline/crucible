use crate::{Compatibility, Exception, RULE_IDS, Rules};
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
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
        // Problems found after reading point at the field they are about.
        let at = |span: Option<Range<usize>>, message: String| ConfigError { message, span };
        let locations = || toml::from_str::<Locations>(text).unwrap_or_default();
        if config.schema_version != 1 {
            return Err(at(
                locations().schema_version.map(|value| value.span()),
                format!(
                    "unsupported policy schema_version {}; this version reads 1",
                    config.schema_version
                ),
            ));
        }
        if let Some(rule) = &config.rules.naming {
            for (key, pattern) in [
                ("operations_pattern", &rule.operations_pattern),
                ("shapes_pattern", &rule.shapes_pattern),
                ("members_pattern", &rule.members_pattern),
                ("enum_values_pattern", &rule.enum_values_pattern),
            ] {
                let Some(pattern) = pattern else { continue };
                naming_pattern(&pattern.value).map_err(|error| {
                    at(
                        locations().rules.naming.get(key).map(|value| value.span()),
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
                    locations()
                        .rules
                        .paths
                        .get("prefix")
                        .map(|value| value.span()),
                    "paths.prefix must be an absolute literal path without a trailing slash".into(),
                ));
            }
        }
        for (index, exception) in config.exceptions.iter().enumerate() {
            let field = |pick: fn(&ExceptionLocations) -> &Option<toml::Spanned<toml::Value>>| {
                locations()
                    .exceptions
                    .get(index)
                    .and_then(|located| pick(located).as_ref().map(toml::Spanned::span))
            };
            if !RULE_IDS.contains(&exception.rule.as_str()) {
                return Err(at(
                    field(|e| &e.rule),
                    format!(
                        "unknown exception rule {:?}; the rules are {}",
                        exception.rule,
                        RULE_IDS.join(", ")
                    ),
                ));
            }
            exception
                .shape
                .parse::<crucible_ir::ShapeId>()
                .map_err(|error| at(field(|e| &e.shape), error.to_string()))?;
            if exception.reason.trim().is_empty() {
                return Err(at(
                    field(|e| &e.reason),
                    "exceptions require a nonempty reason".into(),
                ));
            }
        }
        Ok(config)
    }
}

/// Where the fields checked after reading are written. A second, lenient
/// read keeps each field's own span, so an error points at that field, not
/// at an equal string in a comment or another entry.
#[derive(Deserialize, Default)]
struct Locations {
    #[serde(default)]
    schema_version: Option<toml::Spanned<toml::Value>>,
    #[serde(default)]
    rules: RuleLocations,
    #[serde(default)]
    exceptions: Vec<ExceptionLocations>,
}

#[derive(Deserialize, Default)]
struct RuleLocations {
    #[serde(default)]
    naming: BTreeMap<String, toml::Spanned<toml::Value>>,
    #[serde(default)]
    paths: BTreeMap<String, toml::Spanned<toml::Value>>,
}

#[derive(Deserialize, Default)]
struct ExceptionLocations {
    #[serde(default)]
    rule: Option<toml::Spanned<toml::Value>>,
    #[serde(default)]
    shape: Option<toml::Spanned<toml::Value>>,
    #[serde(default)]
    reason: Option<toml::Spanned<toml::Value>>,
}

pub(crate) fn naming_pattern(pattern: &str) -> Result<Regex, regex::Error> {
    // Validate the same whole-name expression used by the checker, including
    // patterns whose verbose-mode comments could consume the closing anchors.
    Regex::new(&format!("\\A(?:{pattern})\\z"))
}
