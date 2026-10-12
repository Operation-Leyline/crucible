//! npm registry credentials, found where `npm` itself finds them: the
//! project's `.npmrc`, then the user config (`NPM_CONFIG_USERCONFIG`, which
//! `actions/setup-node` points at the file it writes, or `~/.npmrc`).
//!
//! Only per-registry keys count (`//npm.pkg.github.com/:_authToken=…`),
//! matched the way npm matches them: the registry URL without its scheme,
//! then each parent path. `${NAME}` in a value is the environment variable,
//! so setup-node's `${NODE_AUTH_TOKEN}` works. A bare `NODE_AUTH_TOKEN` is
//! never sent by itself: nothing would say which registry it was issued
//! for, and a GitHub token must not reach registry.npmjs.org.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use url::Url;

use crate::http::Credential;

/// Looks `name` up in the environment.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The credential for `registry`, from the first `.npmrc` that has one.
/// `project` is the directory `npm publish` runs in.
pub fn credential(
    registry: &Url,
    project: Option<&Path>,
    env: Env<'_>,
) -> Result<Option<Credential>, String> {
    for file in files(project, env) {
        let text = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot read {}: {error}", file.display())),
        };
        if let Some(credential) =
            parse(registry, &text, env).map_err(|error| format!("{}: {error}", file.display()))?
        {
            return Ok(Some(credential));
        }
    }
    Ok(None)
}

/// The `.npmrc` files npm reads, most specific first.
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
    files
}

/// The credential one `.npmrc`'s text holds for `registry`.
pub fn parse(registry: &Url, text: &str, env: Env<'_>) -> Result<Option<Credential>, String> {
    let entries: Vec<(&str, &str)> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';', '[']))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim(), unquote(value.trim())))
        .collect();
    for nerf in nerf_darts(registry) {
        let find = |name: &str| {
            entries.iter().rev().find_map(|(key, value)| {
                let (scope, field) = key.rsplit_once(':')?;
                (field == name && normalize(scope) == nerf).then_some(*value)
            })
        };
        if let Some(token) = find("_authToken") {
            return Ok(Credential::bearer(registry, &expand(token, env)?));
        }
        if let Some(auth) = find("_auth") {
            return Ok(Credential::basic(registry, &expand(auth, env)?));
        }
        if let (Some(user), Some(password)) = (find("username"), find("_password")) {
            let password = base64::engine::general_purpose::STANDARD
                .decode(expand(password, env)?)
                .map_err(|_| format!("{nerf}:_password is not base64"))?;
            let mut pair = expand(user, env)?.into_bytes();
            pair.push(b':');
            pair.extend(password);
            let encoded = base64::engine::general_purpose::STANDARD.encode(pair);
            return Ok(Credential::basic(registry, &encoded));
        }
    }
    Ok(None)
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

/// A key's registry part, with the trailing slash npm also accepts missing.
fn normalize(scope: &str) -> String {
    let lower = scope.to_ascii_lowercase();
    if lower.ends_with('/') {
        lower
    } else {
        format!("{lower}/")
    }
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

/// Replaces `${NAME}` with the environment's value. An unset name is an
/// error, as it is for npm: an empty token would read as "no credentials".
fn expand(value: &str, env: Env<'_>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unterminated ${{ in {value:?}"))?;
        let name = &after[..end];
        let resolved = env(name)
            .filter(|resolved| !resolved.is_empty())
            .ok_or_else(|| format!("${{{name}}} is not set"))?;
        out.push_str(&resolved);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{credential, parse};
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

    fn authorization(credential: Option<crate::http::Credential>) -> Option<String> {
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
        let credential = credential?;
        let capture = Capture(RefCell::default());
        let url = registry("https://npm.pkg.github.com/x");
        get(&capture, &url, "*/*", Some(&credential), 1).unwrap();
        capture.0.into_inner()
    }

    #[test]
    fn reads_what_setup_node_writes() {
        let text = "//npm.pkg.github.com/:_authToken=${NODE_AUTH_TOKEN}\n@operation-pigeon:registry=https://npm.pkg.github.com/\nalways-auth=true\n";
        let found = parse(&registry("https://npm.pkg.github.com"), text, &env).unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Bearer ghp_token"));
    }

    #[test]
    fn matches_the_registry_path_and_its_parents_only() {
        let text = "; comment\n# comment\n//other.example/:_authToken=nope\n//npm.pkg.github.com/:_authToken = \"quoted\"\n";
        let found = parse(&registry("https://npm.pkg.github.com/owner/"), text, &env).unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Bearer quoted"));
        // The most specific path wins.
        let text = "//r.example/:_authToken=root\n//r.example/npm/repo/:_authToken=deep\n";
        let deep = parse(&registry("https://r.example/npm/repo/"), text, &env).unwrap();
        let root = parse(&registry("https://r.example/other/"), text, &env).unwrap();
        assert_ne!(deep, root);
        assert!(deep.is_some() && root.is_some());
        // Another host or port matches nothing.
        assert!(
            parse(&registry("https://r.example:8443/"), text, &env)
                .unwrap()
                .is_none()
        );
        assert!(
            parse(&registry("https://npm.pkg.github.com/"), text, &env)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn basic_auth_forms() {
        let found = parse(
            &registry("https://npm.pkg.github.com/"),
            "//npm.pkg.github.com/:_auth=dXNlcjpwYXNz\n",
            &env,
        )
        .unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Basic dXNlcjpwYXNz"));
        // username + base64 _password make the same header.
        let found = parse(
            &registry("https://npm.pkg.github.com/"),
            "//npm.pkg.github.com/:username=user\n//npm.pkg.github.com/:_password=cGFzcw==\n",
            &env,
        )
        .unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Basic dXNlcjpwYXNz"));
    }

    #[test]
    fn an_unset_variable_is_an_error_not_an_empty_token() {
        for value in ["${MISSING}", "${EMPTY}", "${UNTERMINATED"] {
            let text = format!("//npm.pkg.github.com/:_authToken={value}\n");
            assert!(
                parse(&registry("https://npm.pkg.github.com/"), &text, &env).is_err(),
                "{value}"
            );
        }
        // ...but only on the registry's own line.
        let text = "//other.example/:_authToken=${MISSING}\n";
        assert!(
            parse(&registry("https://npm.pkg.github.com/"), text, &env)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_bare_node_auth_token_is_not_a_credential() {
        let dir = tempfile::tempdir().unwrap();
        let none = |name: &str| (name == "NODE_AUTH_TOKEN").then(|| "ghp_token".to_owned());
        assert!(
            credential(
                &registry("https://registry.npmjs.org/"),
                Some(dir.path()),
                &none
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn the_project_npmrc_wins_over_the_user_config() {
        let project = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        fs::write(
            project.path().join(".npmrc"),
            "//npm.pkg.github.com/:_authToken=project\n",
        )
        .unwrap();
        let userconfig = user.path().join("npmrc");
        fs::write(&userconfig, "//npm.pkg.github.com/:_authToken=user\n").unwrap();
        let path = userconfig.to_string_lossy().into_owned();
        let env = move |name: &str| (name == "NPM_CONFIG_USERCONFIG").then(|| path.clone());
        let npm = registry("https://npm.pkg.github.com/");
        let found = credential(&npm, Some(project.path()), &env).unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Bearer project"));
        let found = credential(&npm, Some(user.path()), &env).unwrap();
        assert_eq!(authorization(found).as_deref(), Some("Bearer user"));
    }
}
