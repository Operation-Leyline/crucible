//! How a client reaches its service: endpoint, credentials, retries, and
//! interceptors that see every attempt.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue};

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A bearer token: fixed, or asked for on every attempt so a refreshed one
/// is picked up.
#[derive(Clone)]
pub enum Token {
    Fixed(String),
    Provider(Arc<dyn Fn() -> BoxFuture<Option<String>> + Send + Sync>),
}

impl Token {
    pub fn fixed(token: impl Into<String>) -> Self {
        Self::Fixed(token.into())
    }

    pub fn provider<F, Fut>(provider: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<String>> + Send + 'static,
    {
        Self::Provider(Arc::new(move || Box::pin(provider())))
    }

    pub(crate) async fn get(&self) -> Option<String> {
        match self {
            Self::Fixed(token) => Some(token.clone()),
            Self::Provider(provider) => provider().await,
        }
    }
}

/// How failed attempts are retried. Throttling is always retried; other
/// transient failures only when the operation is safe to repeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retry {
    /// Attempts in all, the first included.
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(20),
        }
    }
}

impl Retry {
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }
}

/// Sees, and may change, every attempt of every request before it is sent:
/// signing, tracing headers, a home-region pin.
pub trait Interceptor: Send + Sync + 'static {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String>;
}

/// A client's settings. `Config::new(endpoint)` and the builder methods.
#[derive(Clone)]
pub struct Config {
    pub(crate) endpoint: String,
    pub(crate) token: Option<Token>,
    pub(crate) retry: Retry,
    pub(crate) interceptors: Vec<Arc<dyn Interceptor>>,
    pub(crate) headers: HeaderMap,
    pub(crate) http: reqwest::Client,
}

impl Config {
    pub fn new(endpoint: impl Into<String>) -> Self {
        // reqwest needs a process-wide TLS crypto provider. Install ring
        // unless the application already chose one; that choice stands.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            token: None,
            retry: Retry::default(),
            interceptors: Vec::new(),
            headers: HeaderMap::new(),
            http: reqwest::Client::new(),
        }
    }

    pub fn token(mut self, token: Token) -> Self {
        self.token = Some(token);
        self
    }

    pub fn retry(mut self, retry: Retry) -> Self {
        self.retry = retry;
        self
    }

    /// Runs on every attempt, after the bearer token and default headers, in
    /// the order added.
    pub fn interceptor(mut self, interceptor: impl Interceptor) -> Self {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    /// A header sent on every request.
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    /// The HTTP client to send with: for connection pools, proxies, timeouts.
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }
}
