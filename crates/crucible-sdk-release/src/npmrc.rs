//! npm registry credentials, found the way `npm` finds them.
//!
//! - **Layers.** The project's `.npmrc`, the user config
//!   (`NPM_CONFIG_USERCONFIG`, which `actions/setup-node` points at the file
//!   it writes, or `~/.npmrc`) and the global config
//!   (`NPM_CONFIG_GLOBALCONFIG`, when set) are merged key by key, the
//!   project's winning; within a file the last line wins.
//! - **Scope.** Only per-registry keys count
//!   (`//npm.pkg.github.com/:_authToken=…`). The merged keys are matched
//!   against the registry URL without its scheme, then each parent path,
//!   so the longest matching scope wins whichever file it came from. Host
//!   and port compare without case; the path is compared exactly, as npm
//!   does, since a registry may serve separate tenants at `/Team` and
//!   `/team`.
//! - **Expansion.** `${NAME}` is the environment variable, so setup-node's
//!   `${NODE_AUTH_TOKEN}` works. As in npm, an unset `${NAME}` stays as
//!   written and `${NAME?}` becomes empty; either way the credential is
//!   unusable, so it is skipped and reads go without credentials, with a
//!   note naming the variable (a fork's pull request has no secrets, and a
//!   public package must still plan).
//!
//! A bare `NODE_AUTH_TOKEN` is never sent by itself: nothing would say
//! which registry it was issued for, and a GitHub token must not reach
//! registry.npmjs.org. Nothing here ever prints a credential's value.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use url::Url;

use crate::http::Credential;

/// Looks `name` up in the environment.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// What the configuration says about one registry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lookup {
    pub credential: Option<Credential>,
    /// Why a configured credential was not used, for an error message.
    /// Never holds the credential.
    pub note: Option<String>,
}

/// The credential for `registry` from the merged `.npmrc` layers.
/// `project` is the directory `npm publish` runs in.
pub fn credential(registry: &Url, project: Option<&Path>, env: Env<'_>) -> Result<Lookup, String> {
    let mut merged = BTreeMap::new();
    for file in files(project, env) {
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot read {}: {error}", file.display())),
        };
        // Earlier layers win, key by key.
        for (key, value) in entries(&text) {
            merged.entry(key).or_insert(value);
        }
    }
    Ok(resolve(registry, &merged, env))
}

/// The credential one `.npmrc`'s text holds for `registry`.
pub fn parse(registry: &Url, text: &str, env: Env<'_>) -> Lookup {
    resolve(registry, &entries(text), env)
}

/// The `.npmrc` files npm reads, highest precedence first.
fn files(project: Option<&Path>, env: Env<'_>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = project.map(|dir| dir.join(".npmrc")).into_iter().collect();
    let user = env("NPM_CONFIG_USERCONFIG")
        .or_else(|| env("npm_config_userconfig"))
        .map(PathBuf::from)
        .or_else(|| {
            env("HOME")
                .or_else(|| env("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".npmrc"))
        });
    files.extend(user);
    files.extend(
        env("NPM_CONFIG_GLOBALCONFIG")
            .or_else(|| env("npm_config_globalconfig"))
            .map(PathBuf::from),
    );
    files
}

/// A file's per-registry keys, normalized, the last line winning.
fn entries(text: &str) -> BTreeMap<String, String> {
    let mut entries = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';', '[']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some((scope, field)) = key.trim().rsplit_once(':') else {
            continue;
        };
        if !scope.starts_with("//") {
            continue;
        }
        entries.insert(
            format!("{}:{field}", normalize(scope)),
            unquote(value.trim()).to_owned(),
        );
    }
    entries
}

fn resolve(registry: &Url, entries: &BTreeMap<String, String>, env: Env<'_>) -> Lookup {
    for nerf in nerf_darts(registry) {
        let find = |field: &str| entries.get(&format!("{nerf}:{field}")).map(String::as_str);
        let unusable = |what: &str, problem: String| Lookup {
            credential: None,
            note: Some(format!("the .npmrc {what} for {nerf} {problem}")),
        };
        if let Some(token) = find("_authToken") {
            return match expand(token, env) {
                Ok(token) => Lookup {
                    credential: Credential::bearer(registry, &token),
                    note: None,
                },
                Err(problem) => unusable("_authToken", problem),
            };
        }
        if let Some(auth) = find("_auth") {
            return match expand(auth, env) {
                Ok(auth) => Lookup {
                    credential: Credential::basic(registry, &auth),
                    note: None,
                },
                Err(problem) => unusable("_auth", problem),
            };
        }
        if let (Some(user), Some(password)) = (find("username"), find("_password")) {
            let (user, password) = match (expand(user, env), expand(password, env)) {
                (Ok(user), Ok(password)) => (user, password),
                (Err(problem), _) | (_, Err(problem)) => {
                    return unusable("username/_password", problem);
                }
            };
            let Ok(password) = base64::engine::general_purpose::STANDARD.decode(password) else {
                return unusable("_password", "is not base64".into());
            };
            let mut pair = user.into_bytes();
            pair.push(b':');
            pair.extend(password);
            let encoded = base64::engine::general_purpose::STANDARD.encode(pair);
            return Lookup {
                credential: Credential::basic(registry, &encoded),
                note: None,
            };
        }
    }
    Lookup::default()
}

/// `//host[:port]/path/`, then each parent path, as npm tries them.
fn nerf_darts(registry: &Url) -> Vec<String> {
    let Some(host) = registry.host_str() else {
        return Vec::new();
    };
    let authority = match registry.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    }
    .to_ascii_lowercase();
    let segments: Vec<&str> = registry
        .path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    (0..=segments.len())
        .rev()
        .map(|depth| {
            let mut nerf = format!("//{authority}/");
            for segment in &segments[..depth] {
                nerf.push_str(segment);
                nerf.push('/');
            }
            nerf
        })
        .collect()
}

/// A key's registry part: the authority without case, the path exactly,
/// with the trailing slash npm also accepts missing.
fn normalize(scope: &str) -> String {
    let rest = scope.trim_start_matches('/');
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut normalized = format!("//{}/{path}", authority.to_ascii_lowercase());
    if !normalized.ends_with('/') {
        normalized.push('/');
    }
    normalized
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(value)
}

/// Expands `${NAME}` and `${NAME?}` as npm does, but refuses a result npm
/// would send uselessly: one with an unset `${NAME}` left in it, or an
/// empty one. The error names variables, never the value.
fn expand(value: &str, env: Env<'_>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    let mut missing = Vec::new();
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err("has an unterminated ${".into());
        };
        let name = &after[..end];
        let (name, optional) = match name.strip_suffix('?') {
            Some(name) => (name, true),
            None => (name, false),
        };
        match env(name) {
            Some(resolved) => out.push_str(&resolved),
            None if optional => {}
            None => missing.push(format!("${{{name}}}")),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    if !missing.is_empty() {
        return Err(format!("names {}, which is not set", missing.join(", ")));
    }
    if out.is_empty() {
        return Err("is empty".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Lookup, credential, parse};
    use std::fs;
    use url::Url;

    fn registry(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn env(name: &str) -> Option<String> {
        match name {
            "NODE_AUTH_TOKEN" => Some("ghp_token".into()),
            "EMPTY" => Some(String::new()),
            _ => None,
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn authorization(lookup: Lookup) -> Option<String> {
        // The header is private; a fake transport shows what would be sent.
        use crate::http::{HttpRequest, HttpResponse, HttpTransport, get};
        use std::cell::RefCell;
        struct Capture(RefCell<Option<String>>);
        impl HttpTransport for Capture {
            fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
                *self.0.borrow_mut() = request.authorization.map(str::to_owned);
                Ok(HttpResponse {
                    status: 200,
                    location: None,
                    body: Vec::new(),
                })
            }
        }
        let credential = lookup.credential?;
        let capture = Capture(RefCell::default());
        let url = registry("https://npm.pkg.github.com/x");
        get(&capture, &url, "*/*", Some(&credential), 1).unwrap();
        capture.0.into_inner()
    }

    #[test]
    fn reads_what_setup_node_writes() {
        let text = "//npm.pkg.github.com/:_authToken=${NODE_AUTH_TOKEN}\n@operation-pigeon:registry=https://npm.pkg.github.com/\nalways-auth=true\n";
        let found = parse(&registry("https://npm.pkg.github.com"), text, &env);
        assert_eq!(authorization(found).as_deref(), Some("Bearer ghp_token"));
    }

    #[test]
    fn matches_the_registry_path_and_its_parents_only() {
        let text = "; comment\n# comment\n//other.example/:_authToken=nope\n//NPM.pkg.github.com/:_authToken = \"quoted\"\n";
        let found = parse(&registry("https://npm.pkg.github.com/owner/"), text, &env);
        // The host compares without case.
        assert_eq!(authorization(found).as_deref(), Some("Bearer quoted"));
        // The most specific path wins.
        let text = "//r.example/:_authToken=root\n//r.example/npm/repo/:_authToken=deep\n";
        let deep = parse(&registry("https://r.example/npm/repo/"), text, &env);
        let root = parse(&registry("https://r.example/other/"), text, &env);
        assert_ne!(deep, root);
        assert!(deep.credential.is_some() && root.credential.is_some());
        // Another host or port matches nothing.
        assert_eq!(
            parse(&registry("https://r.example:8443/"), text, &env),
            Lookup::default()
        );
        assert_eq!(
            parse(&registry("https://npm.pkg.github.com/"), text, &env),
            Lookup::default()
        );
    }

    /// A registry may serve tenants at `/Internal` and `/internal`: a
    /// token for one never goes to the other.
    #[test]
    fn paths_compare_exactly() {
        let text = "//r.example/Internal/:_authToken=secret\n";
        assert_eq!(
            parse(&registry("https://r.example/internal/"), text, &env),
            Lookup::default()
        );
        assert!(
            parse(&registry("https://r.example/Internal/"), text, &env)
                .credential
                .is_some()
        );
    }

    #[test]
    fn basic_auth_forms() {
        let found = parse(
            &registry("https://npm.pkg.github.com/"),
            "//npm.pkg.github.com/:_auth=dXNlcjpwYXNz\n",
            &env,
        );
        assert_eq!(authorization(found).as_deref(), Some("Basic dXNlcjpwYXNz"));
        // username + base64 _password make the same header.
        let found = parse(
            &registry("https://npm.pkg.github.com/"),
            "//npm.pkg.github.com/:username=user\n//npm.pkg.github.com/:_password=cGFzcw==\n",
            &env,
        );
        assert_eq!(authorization(found).as_deref(), Some("Basic dXNlcjpwYXNz"));
    }

    /// As in npm, an unset variable leaves the credential unusable; it is
    /// skipped with a note, never an error, and the note never shows the
    /// value around it.
    #[test]
    fn an_unset_variable_skips_the_credential_and_says_why() {
        for (value, says) in [
            ("ghp_secret${MISSING}", "${MISSING}, which is not set"),
            ("${MISSING?}", "is empty"),
            ("${EMPTY}", "is empty"),
            ("ghp_secret${UNTERMINATED", "unterminated"),
        ] {
            let text = format!("//npm.pkg.github.com/:_authToken={value}\n");
            let found = parse(&registry("https://npm.pkg.github.com/"), &text, &env);
            assert_eq!(found.credential, None, "{value}");
            let note = found.note.unwrap();
            assert!(note.contains(says), "{note}");
            assert!(!note.contains("ghp_secret"), "{note}");
        }
        // An optional variable that is set expands.
        let text = "//npm.pkg.github.com/:_authToken=${NODE_AUTH_TOKEN?}\n";
        let found = parse(&registry("https://npm.pkg.github.com/"), text, &env);
        assert_eq!(authorization(found).as_deref(), Some("Bearer ghp_token"));
    }

    #[test]
    fn a_bare_node_auth_token_is_not_a_credential() {
        let dir = tempfile::tempdir().unwrap();
        let none = |name: &str| (name == "NODE_AUTH_TOKEN").then(|| "ghp_token".to_owned());
        assert_eq!(
            credential(
                &registry("https://registry.npmjs.org/"),
                Some(dir.path()),
                &none
            )
            .unwrap(),
            Lookup::default()
        );
    }

    /// Layers merge key by key, and then the longest scope wins, whichever
    /// layer it came from.
    #[test]
    fn layers_merge_before_the_longest_scope_is_chosen() {
        let project = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(".npmrc"),
            "//r.example/:_authToken=project-host\n//r.example/npm/:_authToken=project-npm\n",
        )
        .unwrap();
        let userconfig = user.path().join("npmrc");
        fs::write(
            &userconfig,
            "//r.example/npm/team/:_authToken=user-team\n//r.example/npm/:_authToken=user-npm\n",
        )
        .unwrap();
        let path = userconfig.to_string_lossy().into_owned();
        let env = move |name: &str| (name == "NPM_CONFIG_USERCONFIG").then(|| path.clone());
        let lookup = |url: &str| {
            credential(&registry(url), Some(project.path()), &env)
                .unwrap()
                .credential
        };
        let token = |url: &str, token: &str| {
            parse(
                &registry(url),
                &format!("//r.example/:_authToken={token}\n"),
                &no_env,
            )
            .credential
        };
        // The user's team scope is longer than anything in the project.
        assert_eq!(
            lookup("https://r.example/npm/team/"),
            token("https://r.example/", "user-team")
        );
        // The same key in both layers: the project's wins.
        assert_eq!(
            lookup("https://r.example/npm/other/"),
            token("https://r.example/", "project-npm")
        );
        assert_eq!(
            lookup("https://r.example/"),
            token("https://r.example/", "project-host")
        );
    }
}
