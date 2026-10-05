//! Streaming payloads cross the generated client/server boundary without collection.
use crucible::ByteStream as ServerStream;
use crucible::host;
use crucible_client::{ByteStream, CallOptions, CancellationToken, Config, Retry};
use futures_util::{Stream, StreamExt};
use std::io;
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;

#[allow(dead_code)]
mod client {
    include!(concat!(env!("OUT_DIR"), "/streaming_client.rs"));
}
#[allow(dead_code)]
mod server {
    include!(concat!(env!("OUT_DIR"), "/streaming_server.rs"));
}

struct Chunks {
    remaining: usize,
    stalled: bool,
    dropped: Arc<AtomicUsize>,
}
impl Stream for Chunks {
    type Item = io::Result<bytes::Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.stalled {
            return Poll::Pending;
        }
        if self.remaining == 0 {
            return Poll::Ready(None);
        }
        self.remaining -= 1;
        Poll::Ready(Some(Ok(vec![7u8; 65536].into())))
    }
}
impl Drop for Chunks {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
struct Streams {
    bytes: AtomicUsize,
    uploaded: AtomicUsize,
    fail_upload: bool,
    stalled: bool,
    dropped: Arc<AtomicUsize>,
}
impl server::UploadOperation for Streams {
    async fn upload(&self, input: server::model::UploadInput) -> crucible::Result<()> {
        self.uploaded.fetch_add(1, Ordering::SeqCst);
        let mut body = input
            .body
            .open()
            .map_err(|error| crucible::Error::internal(error.to_string()))?;
        while let Some(chunk) = body.next().await {
            self.bytes.fetch_add(
                chunk
                    .map_err(|error| crucible::Error::internal(error.to_string()))?
                    .len(),
                Ordering::SeqCst,
            );
        }
        if self.fail_upload {
            Err(crucible::Error::internal("retry probe"))
        } else {
            Ok(())
        }
    }
}
impl server::SizedOperation for Streams {
    async fn sized(&self, input: server::model::SizedInput) -> crucible::Result<()> {
        let mut body = input
            .body
            .open()
            .map_err(|error| crucible::Error::internal(error.to_string()))?;
        while let Some(chunk) = body.next().await {
            self.bytes.fetch_add(
                chunk
                    .map_err(|error| crucible::Error::internal(error.to_string()))?
                    .len(),
                Ordering::SeqCst,
            );
        }
        Ok(())
    }
}
impl server::DownloadOperation for Streams {
    async fn download(&self) -> crucible::Result<server::model::DownloadOutput> {
        Ok(server::model::DownloadOutput {
            count: None,
            body: ServerStream::new(Chunks {
                remaining: 1024,
                stalled: self.stalled,
                dropped: self.dropped.clone(),
            }),
        })
    }
}
async fn serve(stalled: bool, fail_upload: bool) -> (String, Arc<Streams>) {
    let streams = Arc::new(Streams {
        bytes: AtomicUsize::new(0),
        uploaded: AtomicUsize::new(0),
        fail_upload,
        stalled,
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    let service = crucible::Crucible::builder(server::operations(streams.clone()))
        .handler(crucible::JsonProtocol)
        .body_limit(128 * 1024 * 1024)
        .on_rejection(|_| {
            Box::new(weather_service::contract::errors::ConflictError {
                message: "invalid".into(),
            })
        })
        .build()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(host::http_on(service, listener));
    (endpoint, streams)
}
fn retry() -> Retry {
    Retry {
        max_attempts: 2,
        base_delay: Duration::ZERO,
        max_delay: Duration::ZERO,
    }
}
async fn dropped(counter: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn large_upload_and_download_remain_streamed() {
    let (endpoint, state) = serve(false, false).await;
    let client = client::StreamingClient::new(Config::new(endpoint));
    let source = ByteStream::new(Chunks {
        remaining: 1024,
        stalled: false,
        dropped: Arc::new(AtomicUsize::new(0)),
    });
    client
        .upload(client::model::UploadInput { body: source })
        .await
        .unwrap();
    assert_eq!(state.bytes.load(Ordering::SeqCst), 64 * 1024 * 1024);
    let output = client.download().await.unwrap();
    let mut stream = output.body.open().unwrap();
    let mut total = 0;
    while let Some(chunk) = stream.next().await {
        total += chunk.unwrap().len();
    }
    assert_eq!(total, 64 * 1024 * 1024);
    dropped(&state.dropped).await;
}
#[tokio::test]
async fn one_shot_never_retries_but_factory_opens_once_per_attempt() {
    let (endpoint, state) = serve(false, true).await;
    let client = client::StreamingClient::new(Config::new(endpoint).retry(retry()));
    let once = ByteStream::new(futures_util::stream::iter([Ok::<_, io::Error>(
        vec![1u8; 3].into(),
    )]));
    assert!(
        client
            .upload(client::model::UploadInput { body: once })
            .await
            .is_err()
    );
    assert_eq!(state.uploaded.load(Ordering::SeqCst), 1);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let replay = ByteStream::replay(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        futures_util::stream::iter([Ok::<_, io::Error>(vec![1u8; 3].into())])
    });
    assert!(
        client
            .upload(client::model::UploadInput { body: replay })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(state.uploaded.load(Ordering::SeqCst), 3);
    assert_eq!(state.bytes.load(Ordering::SeqCst), 9);
}
#[tokio::test]
async fn deadline_and_cancellation_survive_returning_a_stream() {
    let (endpoint, state) = serve(true, false).await;
    let client =
        client::StreamingClient::new(Config::new(endpoint).timeout(Duration::from_millis(100)));
    let output = client.download().await.unwrap();
    let mut body = output.body.open().unwrap();
    assert_eq!(
        body.next().await.unwrap().unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    drop(body);
    dropped(&state.dropped).await;
    let token = CancellationToken::new();
    let output = client
        .download_with_options(CallOptions {
            timeout: Some(Duration::from_secs(2)),
            cancellation: Some(token.clone()),
        })
        .await
        .unwrap();
    let mut body = output.body.open().unwrap();
    token.cancel();
    assert_eq!(
        body.next().await.unwrap().unwrap_err().kind(),
        io::ErrorKind::Interrupted
    );
}
#[tokio::test]
async fn closing_an_unopened_response_releases_server_reader() {
    let (endpoint, state) = serve(true, false).await;
    let client = client::StreamingClient::new(Config::new(endpoint));
    let output = client.download().await.unwrap();
    output.body.close();
    dropped(&state.dropped).await;
}
#[tokio::test]
async fn streaming_digests_and_required_lengths_are_explicit() {
    let (endpoint, state) = serve(false, false).await;
    let client = client::StreamingClient::new(Config::new(endpoint).content_sha256(true));
    let missing = client
        .upload(client::model::UploadInput {
            body: ByteStream::from_bytes("abc"),
        })
        .await
        .unwrap_err();
    assert!(missing.to_string().contains("precomputed"));
    assert_eq!(state.uploaded.load(Ordering::SeqCst), 0);
    let body = ByteStream::from_bytes("abc")
        .with_sha256("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        .unwrap();
    client
        .sized(client::model::SizedInput { body })
        .await
        .unwrap();
    assert_eq!(state.bytes.load(Ordering::SeqCst), 3);
}
