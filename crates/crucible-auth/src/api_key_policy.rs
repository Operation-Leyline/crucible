use crate::{ApiKeyUsageRecorder, presented_credential::PresentedCredential};
use axum::http::{HeaderName, HeaderValue, header, request::Parts};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Longer values are not keys any store mints; refusing to recognize them
/// bounds the hashing a request can cause.
pub(crate) const MAX_CREDENTIAL_LEN: usize = 1024;

pub(crate) type WellFormed = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub(crate) type Fence = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// An `ApiKeyAuthenticator`'s configuration, shared with its background
/// refreshes. Cloned only while the authenticator is being configured.
#[derive(Clone)]
pub(crate) struct ApiKeyPolicy {
    pub prefix: String,
    pub well_formed: Option<WellFormed>,
    pub headers: Vec<HeaderName>,
    pub allow_bare: bool,
    pub issuer: String,
    pub valid_ttl: Duration,
    pub refresh_ahead: Option<Duration>,
    pub valid_capacity: usize,
    pub refusal_initial: Duration,
    pub refusal_ceiling: Duration,
    pub refused_capacity: usize,
    pub lookup_concurrency: usize,
    pub fence: Option<Fence>,
    pub usage: Option<Arc<dyn ApiKeyUsageRecorder>>,
    pub last_used_resolution: Duration,
    pub clock: Clock,
}

impl ApiKeyPolicy {
    pub fn new(prefix: String) -> Self {
        Self {
            prefix,
            well_formed: None,
            headers: vec![header::AUTHORIZATION],
            allow_bare: false,
            issuer: "api-key".into(),
            valid_ttl: Duration::from_secs(300),
            refresh_ahead: None,
            valid_capacity: 10_000,
            refusal_initial: Duration::from_secs(2),
            refusal_ceiling: Duration::from_secs(60),
            refused_capacity: 20_000,
            lookup_concurrency: 32,
            fence: None,
            usage: None,
            last_used_resolution: Duration::from_secs(3600),
            clock: Arc::new(SystemTime::now),
        }
    }

    /// The first configured header present is the only one read, so a header a
    /// proxy owns (`Authorization` behind CloudFront OAC) can sit behind a
    /// service-specific one.
    pub fn presented<'a>(&self, request: &'a Parts) -> PresentedCredential<'a> {
        for name in &self.headers {
            let mut values = request.headers.get_all(name).iter();
            let Some(first) = values.next() else {
                continue;
            };
            let mut rest = values.peekable();
            if rest.peek().is_none() {
                return self
                    .recognize(first)
                    .map_or(PresentedCredential::Absent, PresentedCredential::Key);
            }
            return if std::iter::once(first)
                .chain(rest)
                .any(|value| self.recognize(value).is_some())
            {
                PresentedCredential::Ambiguous
            } else {
                PresentedCredential::Absent
            };
        }
        PresentedCredential::Absent
    }

    /// Recognition happens before any read, so a credential meant for another
    /// authenticator (a JWT) never costs a store lookup.
    fn recognize<'a>(&self, value: &'a HeaderValue) -> Option<&'a str> {
        let value = value.to_str().ok()?;
        let key = match value.split_once(' ') {
            Some((scheme, key)) if scheme.eq_ignore_ascii_case("Bearer") => key,
            Some(_) => return None,
            None if self.allow_bare => value,
            None => return None,
        };
        let recognized = key.len() > self.prefix.len()
            && key.len() <= MAX_CREDENTIAL_LEN
            && key.starts_with(&self.prefix)
            && !key.contains(char::is_whitespace)
            && self.well_formed.as_ref().is_none_or(|check| check(key));
        recognized.then_some(key)
    }

    /// A valid entry read at `read_at` is refreshed in the background once it
    /// is in the last part of its TTL (the final fifth unless configured).
    pub fn refresh_due(&self, now: SystemTime, read_at: SystemTime) -> bool {
        let window = self.refresh_ahead.unwrap_or(self.valid_ttl / 5);
        !window.is_zero()
            && now
                .duration_since(read_at)
                .is_ok_and(|age| age >= self.valid_ttl.saturating_sub(window))
    }
}
