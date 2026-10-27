//! One absolute budget spans credentials, opening and every body read.
use crate::{CancellationToken, Error};
use tokio::time::Instant;

pub(crate) async fn within<T, E>(
    deadline: Option<Instant>,
    cancellation: Option<&CancellationToken>,
    work: impl Future<Output = Result<T, Error<E>>>,
) -> Result<T, Error<E>> {
    let cancelled = async {
        match cancellation {
            Some(token) => token.cancelled().await,
            None => std::future::pending().await,
        }
    };
    let expired = async {
        match deadline {
            Some(at) if at > Instant::now() => tokio::time::sleep_until(at).await,
            Some(_) => (),
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        biased;
        () = cancelled => Err(Error::Cancelled),
        () = expired => Err(Error::Timeout),
        result = work => result,
    }
}
