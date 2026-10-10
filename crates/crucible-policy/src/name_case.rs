use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NameCase {
    PascalCase,
    #[serde(rename = "camelCase")]
    CamelCase,
    #[serde(rename = "snake_case")]
    SnakeCase,
    #[serde(rename = "SCREAMING_SNAKE_CASE")]
    ScreamingSnakeCase,
    #[serde(rename = "kebab-case")]
    KebabCase,
}

impl NameCase {
    pub const ALL: [Self; 5] = [
        Self::PascalCase,
        Self::CamelCase,
        Self::SnakeCase,
        Self::ScreamingSnakeCase,
        Self::KebabCase,
    ];

    /// How the config spells the case.
    pub fn name(self) -> &'static str {
        match self {
            Self::PascalCase => "PascalCase",
            Self::CamelCase => "camelCase",
            Self::SnakeCase => "snake_case",
            Self::ScreamingSnakeCase => "SCREAMING_SNAKE_CASE",
            Self::KebabCase => "kebab-case",
        }
    }

    /// `value` rewritten in this case, for a fix: words split at `_`, `-`
    /// and lower-to-upper boundaries (`createGraph`, `create_graph` and
    /// `CreateGraph` are the same two words).
    pub fn apply(self, value: &str) -> String {
        let mut words: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut previous: Option<char> = None;
        for c in value.chars() {
            if c == '_' || c == '-' {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            } else {
                if c.is_ascii_uppercase()
                    && previous.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit())
                    && !current.is_empty()
                {
                    words.push(std::mem::take(&mut current));
                }
                current.push(c);
            }
            previous = Some(c);
        }
        if !current.is_empty() {
            words.push(current);
        }
        let capitalized = |word: &str| {
            let lower = word.to_ascii_lowercase();
            let mut chars = lower.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        };
        match self {
            Self::PascalCase => words.iter().map(|w| capitalized(w)).collect(),
            Self::CamelCase => words
                .iter()
                .enumerate()
                .map(|(i, w)| {
                    if i == 0 {
                        w.to_ascii_lowercase()
                    } else {
                        capitalized(w)
                    }
                })
                .collect(),
            Self::SnakeCase => words
                .iter()
                .map(|w| w.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("_"),
            Self::ScreamingSnakeCase => words
                .iter()
                .map(|w| w.to_ascii_uppercase())
                .collect::<Vec<_>>()
                .join("_"),
            Self::KebabCase => words
                .iter()
                .map(|w| w.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join("-"),
        }
    }

    pub fn matches(self, value: &str) -> bool {
        let separated = |separator, uppercase: bool| {
            value.split(separator).all(|part: &str| {
                !part.is_empty()
                    && part.bytes().all(|byte| {
                        byte.is_ascii_digit()
                            || if uppercase {
                                byte.is_ascii_uppercase()
                            } else {
                                byte.is_ascii_lowercase()
                            }
                    })
            })
        };
        match self {
            Self::PascalCase => {
                value.starts_with(|c: char| c.is_ascii_uppercase())
                    && value.bytes().all(|b| b.is_ascii_alphanumeric())
            }
            Self::CamelCase => {
                value.starts_with(|c: char| c.is_ascii_lowercase())
                    && value.bytes().all(|b| b.is_ascii_alphanumeric())
            }
            Self::SnakeCase => separated('_', false),
            Self::ScreamingSnakeCase => separated('_', true),
            Self::KebabCase => separated('-', false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_rewritten_into_each_case() {
        for name in [
            "createGraph",
            "CreateGraph",
            "create_graph",
            "CREATE_GRAPH",
            "create-graph",
        ] {
            assert_eq!(NameCase::PascalCase.apply(name), "CreateGraph", "{name}");
            assert_eq!(NameCase::CamelCase.apply(name), "createGraph", "{name}");
            assert_eq!(NameCase::SnakeCase.apply(name), "create_graph", "{name}");
            assert_eq!(
                NameCase::ScreamingSnakeCase.apply(name),
                "CREATE_GRAPH",
                "{name}"
            );
            assert_eq!(NameCase::KebabCase.apply(name), "create-graph", "{name}");
        }
        assert_eq!(NameCase::PascalCase.apply("getV2Item"), "GetV2Item");
        for case in NameCase::ALL {
            assert!(
                case.matches(&case.apply("someName_here")),
                "{}",
                case.name()
            );
        }
    }
}
