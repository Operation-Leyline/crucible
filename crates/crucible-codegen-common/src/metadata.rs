//! Package metadata every native client generator shares
//! (docs/sdk-packaging.md, "The common keys"). Each generator maps the
//! fields its ecosystem has a place for and drops the rest; none is invented.

/// Common package metadata. Unset fields keep what a generator emits today.
///
/// The README and license are carried as file contents, already read and
/// normalized to LF, so generators stay free of I/O and a package's content
/// digest does not depend on the checkout's line endings. Start from
/// [`PackageMetadata::new`] (or `Default`) and set the fields: the struct
/// gains fields as more ecosystems are mapped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Where the package is released, when that decides its manifest.
    pub registry: Option<Registry>,
}

/// Where a package is released, as far as its generated manifest says so.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Registry {
    /// An npm registry: `publishConfig.registry` and `.access`. Build it
    /// with [`Registry::npm`].
    #[non_exhaustive]
    Npm { url: String, access: Option<String> },
    /// Git tags, never a registry: the manifest refuses a registry publish
    /// (`publish = false`, npm `"private": true`).
    Git,
}

impl Registry {
    /// An npm registry, with its `access` (`public` or `restricted`).
    pub fn npm(url: impl Into<String>, access: Option<&str>) -> Self {
        Self::Npm {
            url: url.into(),
            access: access.map(str::to_owned),
        }
    }

    /// Whether this is a public registry's default, where a private or
    /// internal package must never go. An unreadable URL counts as public.
    pub fn is_public(&self) -> bool {
        match self {
            Self::Npm { url, .. } => is_public_npm_registry(url),
            Self::Git => false,
        }
    }
}

impl PackageMetadata {
    /// Empty metadata: every field unset.
    pub fn new() -> Self {
        Self::default()
    }

    /// Problems with these fields that hold in every ecosystem. Each is
    /// a complete sentence naming the field, and never repeats a credential.
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
        if let Some(Registry::Npm { url, access }) = &self.registry {
            if let Some(problem) = url_problem(url) {
                problems.push(format!("the npm registry {problem}"));
            }
            if let Some(access) = access
                && !matches!(access.as_str(), "public" | "restricted")
            {
                problems.push(format!(
                    "npm access {access:?} is not \"public\" or \"restricted\""
                ));
            }
        }
        if self.private && self.registry.as_ref().is_some_and(Registry::is_public) {
            problems.push("a private package never names a public registry".into());
        }
        if self.private
            && matches!(&self.registry, Some(Registry::Npm { access: Some(access), .. }) if access == "public")
        {
            problems.push("a private package is never published with npm access \"public\"".into());
        }
        problems
    }
}

/// Why `value` is not an absolute `http` or `https` URL without
/// credentials, if it is not. The message never repeats a credential.
pub fn url_problem(value: &str) -> Option<String> {
    let shown = redact(value);
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some(format!("{shown:?} is not an http or https URL"));
    }
    let Ok(url) = url::Url::parse(value) else {
        return Some(format!("{shown:?} is not an http or https URL"));
    };
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Some(format!("{shown:?} is not an http or https URL"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Some(format!(
            "{shown:?} carries a user name or password; a published URL never carries credentials"
        ));
    }
    None
}

/// Why a channel or runtime URL must not be written down: it carries a
/// password or token. A user name such as `git@` is fine over SSH; over
/// HTTP(S) a user name can only be a token, since credentials come from the
/// environment. The URL is read as Git and Cargo read it (`https:/u@h`,
/// `git+https://u@h` included); only a form no URL parser reads (scp-like
/// `user@host:path`) is split by hand. The message never repeats a
/// credential.
pub fn credential_problem(url: &str) -> Option<String> {
    let shown = redact(url);
    let password = format!(
        "{shown:?} carries a password or token; credentials come from the environment, never from configuration"
    );
    let user = format!(
        "{shown:?} carries a user name, which over HTTP(S) can only be a token; credentials come from the environment, never from configuration"
    );
    if let Ok(parsed) = url::Url::parse(url)
        && parsed.has_host()
    {
        if parsed.password().is_some() {
            return Some(password);
        }
        if !parsed.username().is_empty() && http_like(parsed.scheme()) {
            return Some(user);
        }
        return None;
    }
    // scp-like `user[:password]@host:path`, or a URL no parser reads.
    let (before, _) = url.rsplit_once('@')?;
    let scheme = before
        .split_once(':')
        .map(|(scheme, _)| scheme.trim_end_matches('/'));
    if before.contains(':') {
        return Some(password);
    }
    if before.contains("//") || scheme.is_some_and(http_like) {
        return Some(user);
    }
    None
}

/// Whether a scheme sends its user info over HTTP: `http`, `https`, and
/// Cargo's `git+https`-style prefixes.
fn http_like(scheme: &str) -> bool {
    let scheme = scheme.to_ascii_lowercase();
    let last = scheme.rsplit('+').next().unwrap_or(&scheme);
    matches!(last, "http" | "https")
}

/// `url` with any user name and password replaced by `***`, for messages.
/// A URL is read as a parser reads it; anything else loses everything
/// before its last `@` that could be a credential.
pub fn redact(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url)
        && parsed.has_host()
    {
        if parsed.password().is_none() && parsed.username().is_empty() {
            return url.into();
        }
        let _ = parsed.set_password(None);
        if parsed.set_username("***").is_ok() {
            return parsed.to_string();
        }
    }
    let Some((before, host)) = url.rsplit_once('@') else {
        return url.into();
    };
    match before.find("://") {
        Some(index) => format!("{}://***@{host}", &before[..index]),
        None if before.contains(':') || before.contains('/') => format!("***@{host}"),
        // A plain SSH user name, as in `git@host:path`.
        None => url.into(),
    }
}

/// A Git remote as provenance names it: the https URL a reader opens, like a
/// configured `repository`. SSH and scp-like forms (`git@host:org/repo.git`,
/// `ssh://git@host/org/repo`) become `https://host/org/repo`; credentials,
/// ports, a trailing `/` and `.git` are dropped. A local path, a `file:`
/// URL, or anything that does not come out as an https URL with a host
/// gives `None`: nothing is better than a wrong link.
pub fn repository_url(remote: &str) -> Option<String> {
    let (host, path) = match url::Url::parse(remote) {
        Ok(parsed) if parsed.has_host() => {
            let scheme = parsed.scheme().to_ascii_lowercase();
            let last = scheme.rsplit('+').next().unwrap_or(&scheme).to_owned();
            if !matches!(last.as_str(), "http" | "https" | "ssh" | "git") {
                return None;
            }
            (parsed.host_str()?.to_owned(), parsed.path().to_owned())
        }
        Ok(_) | Err(_) => {
            // scp-like `[user@]host:path`: a host of more than one
            // character (not a drive letter) and no separators.
            let (authority, path) = remote.split_once(':')?;
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            if host.len() < 2 || host.contains(['/', '\\']) || path.starts_with("//") {
                return None;
            }
            (
                host.to_owned(),
                format!("/{}", path.trim_start_matches('/')),
            )
        }
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let candidate = format!("https://{host}{path}");
    let parsed = url::Url::parse(&candidate).ok()?;
    (parsed.scheme() == "https"
        && parsed.has_host()
        && parsed.username().is_empty()
        && !candidate.contains('\\'))
    .then_some(candidate)
}

/// The host a URL names, as the network sees it: percent-decoded, IDNA
/// ASCII, lowercase, without a trailing dot.
pub fn canonical_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Whether `url` is the public npm registry, however it is spelled. A URL
/// that cannot be read counts as public, so it never passes a private guard.
pub fn is_public_npm_registry(url: &str) -> bool {
    canonical_host(url).is_none_or(|host| {
        matches!(
            host.as_str(),
            "registry.npmjs.org" | "registry.npmjs.com" | "registry.yarnpkg.com"
        )
    })
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
    use super::{
        PackageMetadata, Registry, cargo_keyword_problems, credential_problem,
        is_public_npm_registry, normalize_text, redact, url_problem,
    };

    const SECRET: &str = "sentinel-secret-7f3a";

    #[test]
    fn urls_are_absolute_http_without_credentials() {
        assert_eq!(url_problem("https://example.com/x"), None);
        assert_eq!(url_problem("http://example.com"), None);
        assert!(url_problem("example.com").is_some());
        assert!(url_problem("https://").is_some());
        assert!(url_problem("https://a b").is_some());
        assert!(url_problem("ftp://example.com").is_some());
        for url in [
            format!("https://user:{SECRET}@example.com/x"),
            format!("https://{SECRET}@example.com"),
        ] {
            let problem = url_problem(&url).unwrap();
            assert!(problem.contains("credentials"), "{problem}");
            assert!(!problem.contains(SECRET), "{problem}");
        }
    }

    #[test]
    fn credentials_are_refused_and_never_repeated() {
        for url in [
            format!("https://x:{SECRET}@github.com/a/b.git"),
            format!("https://{SECRET}@npm.pkg.github.com"),
            format!("user:{SECRET}@github.com:a/b.git"),
            format!("ssh://git:{SECRET}@github.com/a/b.git"),
        ] {
            let problem = credential_problem(&url).unwrap_or_else(|| panic!("{url}"));
            assert!(!problem.contains(SECRET), "{problem}");
        }
        assert_eq!(credential_problem("git@github.com:a/b.git"), None);
        assert_eq!(credential_problem("ssh://git@github.com/a/b.git"), None);
        assert_eq!(credential_problem("https://github.com/a/b.git"), None);
        assert_eq!(
            redact(&format!("https://u:{SECRET}@h.example/p?q")),
            "https://***@h.example/p?q"
        );
        assert_eq!(redact("git@github.com:a/b"), "git@github.com:a/b");
    }

    #[test]
    fn the_public_npm_registry_is_recognized_however_it_is_spelled() {
        for url in [
            "https://registry.npmjs.org",
            "https://REGISTRY.npmjs.org/",
            "https://%72egistry.npmjs.org",
            "https://registry.npmjs.org./",
            "https://registry.npmjs.org:443",
            "not a url",
        ] {
            assert!(is_public_npm_registry(url), "{url}");
        }
        assert!(!is_public_npm_registry("https://npm.pkg.github.com"));
        let mut metadata = PackageMetadata::new();
        metadata.private = true;
        metadata.registry = Some(Registry::npm("https://%72egistry.npmjs.org", None));
        assert!(
            metadata
                .problems()
                .iter()
                .any(|problem| problem.contains("public registry"))
        );
        metadata.registry = Some(Registry::Git);
        assert!(metadata.problems().is_empty());
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
        let mut metadata = PackageMetadata::new();
        metadata.repository = Some("github.com/x".into());
        metadata.homepage = Some(format!("https://u:{SECRET}@example.com"));
        metadata.license = Some("MIT".into());
        metadata.license_file = Some("text".into());
        metadata.authors = vec![" ".into()];
        let problems = metadata.problems();
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems[0].starts_with("`repository`"));
        assert!(problems[1].starts_with("`homepage`") && !problems[1].contains(SECRET));
        assert!(PackageMetadata::default().problems().is_empty());
    }

    #[test]
    fn copied_text_is_lf() {
        assert_eq!(normalize_text("a\r\nb\n"), "a\nb\n");
    }

    /// Spellings the URL parser reads as credentials, which splitting on
    /// `://` missed: refused, and never echoed.
    #[test]
    fn every_url_spelling_with_credentials_is_caught_and_redacted() {
        let problem = url_problem("https:/user:SECRETTOK@example.com/x").unwrap();
        assert!(!problem.contains("SECRETTOK"), "{problem}");
        for url in [
            "https:/SECRETTOK2@github.com/o/r.git",
            "http:/SECRETTOK5@github.com/o/r.git",
            "git+https://SECRETTOK6@github.com/o/r.git",
        ] {
            let problem = credential_problem(url).unwrap_or_else(|| panic!("{url} passes"));
            assert!(!problem.contains("SECRETTOK"), "{problem}");
            assert!(!redact(url).contains("SECRETTOK"), "{}", redact(url));
        }
        let problem = url_problem("https://u:SECRETTOK7/x@host").unwrap();
        assert!(!problem.contains("SECRETTOK7"), "{problem}");
    }

    #[test]
    fn remotes_become_https_urls_or_nothing() {
        for (remote, expected) in [
            (
                "https://x-access-token:secret@github.com/a/b.git",
                Some("https://github.com/a/b"),
            ),
            (
                "git@github.com:Operation-Pigeon/pigeon.git",
                Some("https://github.com/Operation-Pigeon/pigeon"),
            ),
            (
                "ssh://git@github.com:22/a/b.git",
                Some("https://github.com/a/b"),
            ),
            ("https://github.com/a/b/", Some("https://github.com/a/b")),
            ("https://[::1]:8080/a", Some("https://[::1]/a")),
            ("C:\\repos\\x", None),
            ("file:///home/u/r", None),
            ("/srv/repo.git", None),
            ("../sibling", None),
        ] {
            assert_eq!(
                super::repository_url(remote).as_deref(),
                expected,
                "{remote}"
            );
        }
    }

    #[test]
    fn a_private_package_is_never_public_on_npm() {
        let mut metadata = PackageMetadata::new();
        metadata.private = true;
        metadata.registry = Some(Registry::npm("https://npm.pkg.github.com", Some("public")));
        assert!(
            metadata
                .problems()
                .iter()
                .any(|problem| problem.contains("access \"public\""))
        );
        metadata.private = false;
        assert!(metadata.problems().is_empty());
    }
}
