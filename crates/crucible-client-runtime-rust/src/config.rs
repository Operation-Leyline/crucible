//! How a client reaches its service: endpoint, credentials, retries, and
//! interceptors that see every attempt.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue};

/// An owned, thread-safe future returned by an identity provider.
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// A bearer token: fixed, or asked for on every attempt so a refreshed one
/// is picked up.
#[derive(Clone)]
pub enum Identity<T> {
    /// Reuses one identity for every attempt.
    Fixed(T),
    /// Invokes an asynchronous provider for each attempt.
    Provider(Arc<dyn Fn() -> BoxFuture<Option<T>> + Send + Sync>),
}

/// A bearer token, fixed or refreshed by a provider.
pub type Token = Identity<String>;

impl<T: Clone> Identity<T> {
    /// Uses a fixed identity for every request attempt.
    pub fn fixed(token: impl Into<T>) -> Self {
        Self::Fixed(token.into())
    }

    /// Refreshes the identity on every attempt; returning None declines this scheme.
    pub fn provider<F, Fut>(provider: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<T>> + Send + 'static,
    {
        Self::Provider(Arc::new(move || Box::pin(provider())))
    }

    pub(crate) async fn get(&self) -> Option<T> {
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
    /// Initial full-jitter exponential backoff bound.
    pub base_delay: Duration,
    /// Maximum exponential backoff bound; Retry-After can override the delay.
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
    /// Disables retries by allowing only the first attempt.
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }
}

/// Sees, and may change, every attempt of every request before it is sent:
/// tracing headers, a home-region pin.
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
    pub(crate) timeout: Option<Duration>,
}

impl Config {
    /// Creates settings with three attempts, a reqwest connection pool, no redirects,
    /// and no total call deadline.
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
            // Redirects bypass authentication and interceptors, and reqwest
            // retains custom credential headers across origins.
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("default HTTP client configuration is valid"),
            timeout: None,
        }
    }

    /// A total budget for one call, including credentials, retries and body reads.
    /// No total-call deadline is imposed unless configured.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Supplies bearer credentials for operations that model bearer authentication.
    pub fn token(mut self, token: Token) -> Self {
        self.token = Some(token);
        self
    }

    /// Sets attempt limits and exponential backoff bounds.
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

    /// The HTTP client to send with: for connection pools, proxies, timeouts,
    /// and an explicit redirect policy.
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new("")
    }
}

/// Per-call settings; an explicit timeout overrides the client default.
#[derive(Clone, Debug, Default)]
pub struct CallOptions {
    /// Total call budget; None uses the client default.
    pub timeout: Option<Duration>,
    /// Optional cancellation shared with the caller.
    pub cancellation: Option<crate::CancellationToken>,
}
