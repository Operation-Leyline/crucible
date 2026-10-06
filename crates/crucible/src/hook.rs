use std::pin::Pin;

/// What a hook gives back: its result now, or a future to wait for. A hook
/// that needs no waiting returns `Ready` and costs no allocation; one that
/// does returns `Hook::wait(async move { .. })`.
pub enum Hook<'a, T> {
    Ready(T),
    Wait(Pin<Box<dyn Future<Output = T> + Send + 'a>>),
}

impl<'a, T> Hook<'a, T> {
    pub fn wait(future: impl Future<Output = T> + Send + 'a) -> Self {
        Self::Wait(Box::pin(future))
    }

    /// The result, waiting for it if it is not ready.
    pub async fn resolve(self) -> T {
        match self {
            Self::Ready(value) => value,
            Self::Wait(future) => future.await,
        }
    }
}
