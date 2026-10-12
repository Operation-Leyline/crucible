//! Package metadata every native client generator shares
//! (docs/sdk-packaging.md, "The common keys"). Each generator maps the
//! fields its ecosystem has a place for and drops the rest; none is invented.

/// Common package metadata. Unset fields keep what a generator emits today.
///
/// The README and license are carried as file contents, already read and
/// normalized to LF, so generators stay free of I/O and a package's content
/// digest does not depend on the checkout's line endings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageMetadata {
    /// One paragraph. Unset, each language keeps its generated description.
    pub description: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    /// An SPDX expression. Exclusive with `license_file`.
    pub license: Option<String>,
    /// Contents copied into the package as `LICENSE`.
    pub license_file: Option<String>,
    /// `Name <email>` strings; the first is the maintainer where one is required.
    pub authors: Vec<String>,
    /// Contents copied into the package as `README.md`.
    pub readme: Option<String>,
    pub keywords: Vec<String>,
    /// Never published to a public registry: the package carries its
    /// ecosystem's guard against an accidental publish.
    pub private: bool,
    /// The registry the package is published to, when it has one.
    pub registry: Option<Registry>,
}

/// A package registry a generated manifest names, so the native publish
/// command goes there and nowhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registry {
    /// An npm registry: `publishConfig.registry` and `.access`.
    Npm { url: String, access: Option<String> },
}

impl PackageMetadata {
    /// Problems with these fields that hold in every ecosystem. Each is
    /// a complete sentence naming the field.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for (key, value) in [
            ("repository", &self.repository),
            ("homepage", &self.homepage),
            ("documentation", &self.documentation),
        ] {
            if let Some(value) = value
                && let Some(problem) = url_problem(value)
            {
                problems.push(format!("`{key}` {problem}"));
            }
        }
        if self.license.is_some() && self.license_file.is_some() {
            problems.push("set `license` or `license_file`, not both".into());
        }
        for (key, value) in [
            ("description", &self.description),
            ("license", &self.license),
        ] {
            if value.as_ref().is_some_and(|value| value.trim().is_empty()) {
                problems.push(format!("`{key}` is empty"));
            }
        }
        if self.authors.iter().any(|author| author.trim().is_empty()) {
            problems.push("`authors` has an empty entry".into());
        }
        problems
    }
}

/// Why `value` is not an absolute `http` or `https` URL, if it is not.
pub fn url_problem(value: &str) -> Option<String> {
    let rest = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    match rest {
        Some(rest)
            if !rest.is_empty()
                && !rest.starts_with('/')
                && !value.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            None
        }
        _ => Some(format!("{value:?} is not an http or https URL")),
    }
}

/// What crates.io refuses in a package's keywords: at most 5, each at most
/// 20 ASCII characters (letters, digits, `_`, `-`, `+`), starting with a
/// letter.
pub fn cargo_keyword_problems(keywords: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    if keywords.len() > 5 {
        problems.push(format!(
            "crates.io takes at most 5 keywords, not {}",
            keywords.len()
        ));
    }
    for keyword in keywords {
        let valid = keyword.len() <= 20
            && keyword
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && keyword
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'+'));
        if !valid {
            problems.push(format!(
                "crates.io refuses the keyword {keyword:?}: at most 20 ASCII letters, digits, `_`, `-` or `+`, starting with a letter"
            ));
        }
    }
    problems
}

/// Text copied into a package (a README or license) as it is written and
/// hashed: LF line endings, whatever the checkout used.
pub fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::{PackageMetadata, cargo_keyword_problems, normalize_text, url_problem};

    #[test]
    fn urls_are_absolute_http() {
        assert_eq!(url_problem("https://example.com/x"), None);
        assert_eq!(url_problem("http://example.com"), None);
        assert!(url_problem("example.com").is_some());
        assert!(url_problem("https://").is_some());
        assert!(url_problem("https://a b").is_some());
        assert!(url_problem("ftp://example.com").is_some());
    }

    #[test]
    fn cargo_keywords_follow_crates_io() {
        assert!(cargo_keyword_problems(&["email".into(), "c++".into()]).is_empty());
        assert_eq!(
            cargo_keyword_problems(&["1email".into(), "a".repeat(21), "ok_ok".into()]).len(),
            2
        );
        assert_eq!(
            cargo_keyword_problems(&(0..6).map(|i| format!("k{i}")).collect::<Vec<_>>()).len(),
            1
        );
        assert_eq!(cargo_keyword_problems(&["e mail".into()]).len(), 1);
    }

    #[test]
    fn metadata_problems_name_their_field() {
        let metadata = PackageMetadata {
            repository: Some("github.com/x".into()),
            license: Some("MIT".into()),
            license_file: Some("text".into()),
            authors: vec![" ".into()],
            ..PackageMetadata::default()
        };
        let problems = metadata.problems();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("`repository`"));
        assert!(PackageMetadata::default().problems().is_empty());
    }

    #[test]
    fn copied_text_is_lf() {
        assert_eq!(normalize_text("a\r\nb\n"), "a\nb\n");
    }
}
