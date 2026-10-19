//! Cooperative cancellation shared by any number of client calls.

/// Clones refer to the same cancellation state. Cancellation is permanent.
#[derive(Clone, Debug)]
pub struct CancellationToken(tokio::sync::watch::Sender<bool>);

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancellationToken {
    pub fn new() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn token_is_not_cancelled_initially() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn token_can_be_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_returns_immediately_if_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();

        let timeout = tokio::time::timeout(Duration::from_millis(10), token.cancelled());
        assert!(timeout.await.is_ok(), "cancelled() should return immediately");
    }

    #[tokio::test]
    async fn cancelled_waits_for_cancellation() {
        let token = CancellationToken::new();
        let token_clone = token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            token_clone.cancel();
        });

        let start = std::time::Instant::now();
        token.cancelled().await;
        assert!(start.elapsed() >= Duration::from_millis(10));
        assert!(token.is_cancelled());
    }
}
