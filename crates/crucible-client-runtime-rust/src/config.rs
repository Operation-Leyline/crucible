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

/// A bearer token or API key, fixed or refreshed by a provider.
pub type Token = Identity<String>;

#[derive(Clone)]
/// HTTP basic credentials. Values are sent only for a modeled basic-auth scheme.
pub struct BasicCredentials {
    /// The basic-auth username.
    pub username: String,
    /// The basic-auth password; keep it out of diagnostic output.
    pub password: String,
}

impl BasicCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

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
/// signing, tracing headers, a home-region pin.
pub trait Interceptor: Send + Sync + 'static {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String>;
}

/// A client's settings. `Config::new(endpoint)` and the builder methods.
#[derive(Clone)]
pub struct Config {
    pub(crate) endpoint: String,
    pub(crate) region: Option<String>,
    pub(crate) endpoint_parameters: std::collections::BTreeMap<String, serde_json::Value>,
    pub(crate) endpoint_builtins: std::collections::BTreeMap<String, serde_json::Value>,
    pub(crate) disable_host_prefix: bool,
    pub(crate) token: Option<Token>,
    pub(crate) basic_credentials: Option<Identity<BasicCredentials>>,
    pub(crate) api_key: Option<Token>,
    pub(crate) aws_credentials: Option<Identity<crate::AwsCredentials>>,
    pub(crate) signing_name: Option<String>,
    pub(crate) retry: Retry,
    pub(crate) interceptors: Vec<Arc<dyn Interceptor>>,
    pub(crate) headers: HeaderMap,
    pub(crate) http: reqwest::Client,
    pub(crate) timeout: Option<Duration>,
    pub(crate) content_sha256: bool,
    pub(crate) request_checksum_calculation: crate::ChecksumPolicy,
    pub(crate) response_checksum_validation: crate::ChecksumPolicy,
    pub(crate) checksum_validated: Option<ChecksumValidated>,
}

/// Told which operation's response checksum was verified, and with which algorithm.
pub type ChecksumValidated =
    Arc<dyn Fn(&'static str, crucible_wire::operation::ChecksumAlgorithm) + Send + Sync>;

impl Config {
    /// Creates settings with three attempts, a reqwest connection pool, and no total call deadline.
    pub fn new(endpoint: impl Into<String>) -> Self {
        // reqwest needs a process-wide TLS crypto provider. Install ring
        // unless the application already chose one; that choice stands.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_owned(),
            region: None,
            endpoint_parameters: Default::default(),
            endpoint_builtins: Default::default(),
            disable_host_prefix: false,
            token: None,
            basic_credentials: None,
            api_key: None,
            aws_credentials: None,
            signing_name: None,
            retry: Retry::default(),
            interceptors: Vec::new(),
            headers: HeaderMap::new(),
            http: reqwest::Client::new(),
            timeout: None,
            content_sha256: false,
            request_checksum_calculation: crate::ChecksumPolicy::WhenSupported,
            response_checksum_validation: crate::ChecksumPolicy::WhenSupported,
            checksum_validated: None,
        }
    }

    /// Sets the SigV4 region and AWS::Region endpoint built-in.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }
    /// Sets a named client input for modeled endpoint rules.
    pub fn endpoint_parameter(
        mut self,
        name: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.endpoint_parameters.insert(name.into(), value.into());
        self
    }
    /// Sets an endpoint-rule built-in value.
    pub fn endpoint_builtin(
        mut self,
        name: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.endpoint_builtins.insert(name.into(), value.into());
        self
    }
    /// Disables modeled operation host-prefix injection when true.
    pub fn disable_host_prefix(mut self, disabled: bool) -> Self {
        self.disable_host_prefix = disabled;
        self
    }

    /// A total budget for one call, including credentials, retries and body reads.
    /// No total-call deadline is imposed unless configured.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Hashes the replayed body before caller interceptors (including empty writes).
    pub fn content_sha256(mut self, enabled: bool) -> Self {
        self.content_sha256 = enabled;
        self
    }

    /// When modeled request checksums are computed: `WhenSupported` (the
    /// default) fills an absent algorithm member with CRC32; `WhenRequired`
    /// computes one only when the model requires it or the caller chose one.
    pub fn request_checksum_calculation(mut self, policy: crate::ChecksumPolicy) -> Self {
        self.request_checksum_calculation = policy;
        self
    }

    /// When modeled response checksums are verified: `WhenSupported` (the
    /// default) enables an absent validation-mode member; `WhenRequired`
    /// verifies only when the caller enabled it.
    pub fn response_checksum_validation(mut self, policy: crate::ChecksumPolicy) -> Self {
        self.response_checksum_validation = policy;
        self
    }

    /// Called with the operation name and algorithm whenever a response
    /// checksum is verified. A streamed body is verified when it ends.
    pub fn on_checksum_validated(
        mut self,
        callback: impl Fn(&'static str, crucible_wire::operation::ChecksumAlgorithm)
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.checksum_validated = Some(Arc::new(callback));
        self
    }

    /// Supplies bearer credentials for operations that model bearer authentication.
    pub fn token(mut self, token: Token) -> Self {
        self.token = Some(token);
        self
    }

    /// Supplies credentials for modeled HTTP basic authentication.
    pub fn basic_credentials(mut self, credentials: Identity<BasicCredentials>) -> Self {
        self.basic_credentials = Some(credentials);
        self
    }

    /// Supplies an API key for the modeled header or query location.
    pub fn api_key(mut self, key: Token) -> Self {
        self.api_key = Some(key);
        self
    }

    /// Supplies credentials for modeled SigV4 request signing.
    pub fn aws_credentials(mut self, credentials: Identity<crate::AwsCredentials>) -> Self {
        self.aws_credentials = Some(credentials);
        self
    }
    /// Overrides the SigV4 signing service name.
    pub fn signing_name(mut self, name: impl Into<String>) -> Self {
        self.signing_name = Some(name.into());
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

    /// The HTTP client to send with: for connection pools, proxies, timeouts.
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
