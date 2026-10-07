use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
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
    pub(crate) fn matches(self, value: &str) -> bool {
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
