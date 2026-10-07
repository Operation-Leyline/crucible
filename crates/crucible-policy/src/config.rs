use crate::{Compatibility, Exception, RULE_IDS, Rules};
use regex::Regex;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub compatibility: Compatibility,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
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
        let config: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        if config.schema_version != 1 {
            return Err(format!(
                "unsupported policy schema_version {}",
                config.schema_version
            ));
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
                Regex::new(pattern)
                    .map_err(|error| format!("invalid naming pattern {pattern:?}: {error}"))?;
            }
        }
        if let Some(rule) = &config.rules.paths {
            if let Some(prefix) = &rule.prefix {
                if !prefix.starts_with('/')
                    || prefix.contains(['?', '#', '{', '}'])
                    || (prefix.len() > 1 && prefix.ends_with('/'))
                {
                    return Err(
                        "paths.prefix must be an absolute literal path without a trailing slash"
                            .into(),
                    );
                }
            }
        }
        for exception in &config.exceptions {
            if !RULE_IDS.contains(&exception.rule.as_str()) {
                return Err(format!("unknown exception rule {:?}", exception.rule));
            }
            exception
                .shape
                .parse::<crucible_smithy::ShapeId>()
                .map_err(|error| error.to_string())?;
            if exception.reason.trim().is_empty() {
                return Err("exceptions require a nonempty reason".into());
            }
        }
        Ok(config)
    }
}
