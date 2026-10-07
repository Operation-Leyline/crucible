use std::collections::BTreeMap;

use clap::ArgMatches;
use crucible_client_runtime_rust::{Config, Token};
use serde::Deserialize;

use crate::Error;
use crate::spec::Spec;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    endpoint_url: Option<String>,
    bearer_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profiles {
    profiles: BTreeMap<String, Profile>,
}

pub(crate) fn resolve(spec: &Spec, matches: &ArgMatches) -> Result<Config, Error> {
    let profile = resolve_with(spec, matches, |key| std::env::var(key).ok())?;
    let mut config = Config::new(profile.endpoint_url.expect("resolved endpoint"));
    if let Some(token) = profile.bearer_token {
        config = config.token(Token::fixed(token));
    }
    Ok(config)
}

fn resolve_with(
    spec: &Spec,
    matches: &ArgMatches,
    env: impl Fn(&str) -> Option<String>,
) -> Result<Profile, Error> {
    let setting = |flag: &str, suffix: &str| {
        matches
            .get_one::<String>(flag)
            .cloned()
            .or_else(|| env(&format!("{}_{suffix}", spec.env_prefix)))
    };
    let selected = setting("profile", "PROFILE");
    let path = setting("config", "CONFIG");
    let profile = if let Some(path) = path {
        let bytes = std::fs::read(&path)
            .map_err(|error| Error::input(format!("cannot read config {path}: {error}")))?;
        // Do not echo JSON decoder context: configuration may hold credentials.
        let mut profiles: Profiles = serde_json::from_slice(&bytes).map_err(|_| Error::input("invalid configuration: expected {\"profiles\":{\"name\":{\"endpoint_url\":\"...\",\"bearer_token\":\"...\"}}}"))?;
        let name = selected.as_deref().unwrap_or("default");
        profiles
            .profiles
            .remove(name)
            .ok_or_else(|| Error::input(format!("profile {name:?} does not exist")))?
    } else {
        if selected.is_some() {
            return Err(Error::input(
                "--profile requires --config or the service CONFIG environment variable",
            ));
        }
        Profile::default()
    };
    let endpoint = setting("endpoint-url", "ENDPOINT_URL")
        .or(profile.endpoint_url)
        .or_else(|| spec.endpoint.clone())
        .ok_or_else(|| {
            Error::input(format!(
                "supply --endpoint-url, {}_ENDPOINT_URL, or a configured endpoint_url",
                spec.env_prefix
            ))
        })?;
    let url = url::Url::parse(&endpoint).map_err(|_| Error::input("invalid endpoint URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::input(
            "endpoint URL must be an absolute http:// or https:// URL",
        ));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(Error::input(
            "endpoint URL must not contain credentials, a query, or a fragment",
        ));
    }
    let bearer_token = env(&format!("{}_BEARER_TOKEN", spec.env_prefix)).or(profile.bearer_token);
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    if bearer_token.is_some() && url.scheme() == "http" && !loopback {
        return Err(Error::input(
            "refusing to send a bearer token over non-loopback HTTP; use https://",
        ));
    }
    Ok(Profile {
        endpoint_url: Some(endpoint),
        bearer_token,
    })
}

#[cfg(test)]
mod tests {
    use super::{Spec, resolve_with};

    fn spec() -> Spec {
        serde_json::from_value(serde_json::json!({"name":"test", "version":"1", "env_prefix":"TEST", "endpoint":"https://default.example", "operations":[], "shapes":{}})).unwrap()
    }

    #[test]
    fn explicit_endpoint_overrides_environment_and_default() {
        let spec = spec();
        let matches = crate::command::build(&spec)
            .unwrap()
            .try_get_matches_from(["test", "--endpoint-url", "https://explicit.example"])
            .unwrap();
        let profile = resolve_with(&spec, &matches, |key| {
            (key == "TEST_ENDPOINT_URL").then(|| "https://env.example".to_owned())
        })
        .unwrap();
        assert_eq!(
            profile.endpoint_url.as_deref(),
            Some("https://explicit.example")
        );
        let matches = crate::command::build(&spec)
            .unwrap()
            .try_get_matches_from(["test"])
            .unwrap();
        let profile = resolve_with(&spec, &matches, |key| {
            (key == "TEST_ENDPOINT_URL").then(|| "https://env.example".to_owned())
        })
        .unwrap();
        assert_eq!(profile.endpoint_url.as_deref(), Some("https://env.example"));
        let profile = resolve_with(&spec, &matches, |_| None).unwrap();
        assert_eq!(
            profile.endpoint_url.as_deref(),
            Some("https://default.example")
        );
    }

    #[test]
    fn selected_profile_is_read_and_environment_credentials_win() {
        let path = std::env::temp_dir().join(format!(
            "crucible-cli-runtime-profile-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"profiles":{"production":{"endpoint_url":"https://profile.example","bearer_token":"profile-token"}}}"#).unwrap();
        let spec = spec();
        let matches = crate::command::build(&spec)
            .unwrap()
            .try_get_matches_from([
                "test",
                "--config",
                path.to_str().unwrap(),
                "--profile",
                "production",
            ])
            .unwrap();
        let profile = resolve_with(&spec, &matches, |_| None).unwrap();
        assert_eq!(
            profile.endpoint_url.as_deref(),
            Some("https://profile.example")
        );
        assert_eq!(profile.bearer_token.as_deref(), Some("profile-token"));
        let error = resolve_with(&spec, &matches, |key| {
            (key == "TEST_ENDPOINT_URL").then(|| "http://api.example".to_owned())
        })
        .err()
        .unwrap();
        assert_eq!(error.exit_code(), 2);
        assert!(!error.to_string().contains("profile-token"));
        let profile = resolve_with(&spec, &matches, |key| match key {
            "TEST_BEARER_TOKEN" => Some("environment-token".to_owned()),
            "TEST_ENDPOINT_URL" => Some("https://env.example".to_owned()),
            _ => None,
        })
        .unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(profile.endpoint_url.as_deref(), Some("https://env.example"));
        assert_eq!(profile.bearer_token.as_deref(), Some("environment-token"));
    }

    #[test]
    fn malformed_or_secret_bearing_endpoints_fail_without_echo() {
        let spec = spec();
        for value in [
            "https://",
            "file:///tmp",
            "https://user:secret@example.com",
            "https://example.com?token=secret",
        ] {
            let matches = crate::command::build(&spec)
                .unwrap()
                .try_get_matches_from(["test", "--endpoint-url", value])
                .unwrap();
            let error = resolve_with(&spec, &matches, |_| None).err().unwrap();
            assert_eq!(error.exit_code(), 2);
            assert!(!error.to_string().contains(value));
            assert!(!error.to_string().contains("secret"));
        }
    }

    #[test]
    fn bearer_tokens_require_https_except_on_loopback() {
        let spec = spec();
        for (endpoint, allowed) in [
            ("https://api.example", true),
            ("http://localhost:8080", true),
            ("http://127.0.0.1:8080", true),
            ("http://127.10.20.30:8080", true),
            ("http://[::1]:8080", true),
            ("http://api.example", false),
            ("http://localhost.example", false),
            ("http://192.168.1.2", false),
            ("http://[2001:db8::1]", false),
        ] {
            let matches = crate::command::build(&spec)
                .unwrap()
                .try_get_matches_from(["test", "--endpoint-url", endpoint])
                .unwrap();
            let result = resolve_with(&spec, &matches, |key| {
                (key == "TEST_BEARER_TOKEN").then(|| "secret-token".to_owned())
            });
            assert_eq!(result.is_ok(), allowed, "{endpoint}");
            if let Err(error) = result {
                assert_eq!(error.exit_code(), 2);
                assert!(error.to_string().contains("https://"));
                assert!(!error.to_string().contains("secret-token"));
            }
            // HTTP remains available when no bearer token is configured.
            assert!(resolve_with(&spec, &matches, |_| None).is_ok());
        }
    }
}
