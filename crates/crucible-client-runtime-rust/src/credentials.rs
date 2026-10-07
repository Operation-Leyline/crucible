//! AWS identities and explicit portable provider composition.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use crate::Identity;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub expiration: Option<DateTime<Utc>>,
}
impl std::fmt::Debug for AwsCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsCredentials")
            .field("expiration", &self.expiration)
            .finish_non_exhaustive()
    }
}
impl AwsCredentials {
    pub fn new(access_key_id: impl Into<String>, secret_access_key: impl Into<String>) -> Self {
        Self {
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
            expiration: None,
        }
    }
    pub fn from_environment() -> Result<Option<Self>, String> {
        let mut environment = BTreeMap::new();
        for name in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        ] {
            match std::env::var(name) {
                Ok(value) => {
                    environment.insert(name.to_owned(), value);
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(_) => return Err("AWS credential environment value is not Unicode".into()),
            }
        }
        Self::from_environment_map(&environment)
    }
    pub fn from_environment_map(
        environment: &BTreeMap<String, String>,
    ) -> Result<Option<Self>, String> {
        let access = environment
            .get("AWS_ACCESS_KEY_ID")
            .filter(|value| !value.is_empty());
        let secret = environment
            .get("AWS_SECRET_ACCESS_KEY")
            .filter(|value| !value.is_empty());
        match (access, secret) {
            (None, None) => Ok(None),
            (Some(access), Some(secret)) => Ok(Some(Self {
                access_key_id: access.clone(),
                secret_access_key: secret.clone(),
                session_token: environment
                    .get("AWS_SESSION_TOKEN")
                    .filter(|value| !value.is_empty())
                    .cloned(),
                expiration: None,
            })),
            _ => Err("incomplete AWS environment credentials".into()),
        }
    }
    /// Parse shared credentials or config text supplied by the application.
    pub fn from_profile(text: &str, profile: &str) -> Result<Option<Self>, String> {
        let mut values = BTreeMap::new();
        let mut selected = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].trim();
                selected = name.strip_prefix("profile ").unwrap_or(name) == profile;
                continue;
            }
            if selected && let Some((name, value)) = line.split_once('=') {
                values.insert(name.trim().to_owned(), value.trim().to_owned());
            }
        }
        let environment = [
            ("AWS_ACCESS_KEY_ID", "aws_access_key_id"),
            ("AWS_SECRET_ACCESS_KEY", "aws_secret_access_key"),
            ("AWS_SESSION_TOKEN", "aws_session_token"),
        ]
        .into_iter()
        .filter_map(|(target, source)| {
            values
                .get(source)
                .map(|value| (target.to_owned(), value.clone()))
        })
        .collect();
        Self::from_environment_map(&environment)
    }
    pub fn from_profile_file(
        path: impl AsRef<std::path::Path>,
        profile: &str,
    ) -> Result<Option<Self>, String> {
        Self::from_profile(
            &std::fs::read_to_string(path).map_err(|e| e.to_string())?,
            profile,
        )
    }
}

impl Identity<AwsCredentials> {
    pub fn chain(providers: impl IntoIterator<Item = Self>) -> Self {
        let providers = Arc::new(providers.into_iter().collect::<Vec<_>>());
        Self::provider(move || {
            let providers = providers.clone();
            async move {
                for provider in providers.iter() {
                    if let Some(value) = provider.get().await {
                        return Some(value);
                    }
                }
                None
            }
        })
    }
    /// Serialize concurrent refreshes; expired credentials are never returned.
    pub fn cached(self, refresh_before: Duration) -> Self {
        let provider = self;
        let cached = Arc::new(tokio::sync::Mutex::new(None::<AwsCredentials>));
        Self::provider(move || {
            let provider = provider.clone();
            let cached = cached.clone();
            async move {
                let mut cached = cached.lock().await;
                if let Some(value) = cached.as_ref()
                    && value.expiration.is_none_or(|expiration| {
                        expiration
                            .signed_duration_since(Utc::now())
                            .to_std()
                            .is_ok_and(|remaining| remaining > refresh_before)
                    })
                {
                    return Some(value.clone());
                }
                *cached = provider.get().await.filter(|value| {
                    value
                        .expiration
                        .is_none_or(|expiration| expiration > Utc::now())
                });
                cached.clone()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn empty_session_tokens_are_absent() {
        let mut environment = BTreeMap::from([
            ("AWS_ACCESS_KEY_ID".into(), "key".into()),
            ("AWS_SECRET_ACCESS_KEY".into(), "secret".into()),
            ("AWS_SESSION_TOKEN".into(), String::new()),
        ]);
        assert!(
            AwsCredentials::from_environment_map(&environment)
                .unwrap()
                .unwrap()
                .session_token
                .is_none()
        );
        environment.insert("AWS_SESSION_TOKEN".into(), "token".into());
        assert_eq!(
            AwsCredentials::from_environment_map(&environment)
                .unwrap()
                .unwrap()
                .session_token
                .as_deref(),
            Some("token")
        );
        assert!(AwsCredentials::from_profile("[default]\naws_access_key_id=key\naws_secret_access_key=secret\naws_session_token=", "default").unwrap().unwrap().session_token.is_none());
    }

    #[tokio::test]
    async fn portable_sources_chain_and_concurrent_cache_refresh() {
        assert!(
            AwsCredentials::from_environment_map(&BTreeMap::new())
                .unwrap()
                .is_none()
        );
        assert!(
            AwsCredentials::from_environment_map(&BTreeMap::from([(
                "AWS_ACCESS_KEY_ID".into(),
                "partial".into()
            )]))
            .is_err()
        );
        let credentials=AwsCredentials::from_profile("[default]\naws_access_key_id=other\naws_secret_access_key=other\n[profile selected]\naws_access_key_id=selected\naws_secret_access_key=secret\naws_session_token=session","selected").unwrap().unwrap();
        assert_eq!(credentials.access_key_id, "selected");
        assert_eq!(credentials.session_token.as_deref(), Some("session"));
        assert!(!format!("{credentials:?}").contains("secret"));
        let provider = Identity::chain([
            Identity::provider(|| async { None }),
            Identity::fixed(credentials),
        ]);
        assert_eq!(provider.get().await.unwrap().access_key_id, "selected");
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let provider = Identity::provider(move || {
            count.fetch_add(1, Ordering::SeqCst);
            async {
                tokio::task::yield_now().await;
                let mut value = AwsCredentials::new("key", "secret");
                value.expiration = Some(Utc::now() + chrono::Duration::hours(1));
                Some(value)
            }
        })
        .cached(Duration::from_secs(300));
        let (a, b, c) = tokio::join!(provider.get(), provider.get(), provider.get());
        assert!(a.is_some() && b.is_some() && c.is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let expired = Identity::provider(|| async {
            let mut value = AwsCredentials::new("key", "secret");
            value.expiration = Some(Utc::now() - chrono::Duration::seconds(1));
            Some(value)
        })
        .cached(Duration::ZERO);
        assert!(expired.get().await.is_none());
    }
}
