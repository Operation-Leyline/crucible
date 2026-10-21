use std::time::Duration;

/// Resource and liveness limits for server-response streams. Heartbeats do
/// not reset application-idle time or the request's total deadline.
#[derive(Debug, Clone)]
pub struct StreamOptions {
    pub max_event_bytes: usize,
    pub heartbeat_interval: Option<Duration>,
    pub idle_timeout: Option<Duration>,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self {
            max_event_bytes: 1024 * 1024,
            heartbeat_interval: Some(Duration::from_secs(15)),
            idle_timeout: None,
        }
    }
}
