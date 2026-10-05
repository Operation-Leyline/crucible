use super::*;
use crucible_wire::operation::Fault;
use crucible_wire::operation::{OperationInfo, Unit};

#[derive(Debug)]
struct Known;
static ERROR: ErrorInfo = ErrorInfo {
    name: "Known",
    status: 429,
    fault: Fault::Client,
    retryable: true,
    throttling: true,
    members: &[],
};
impl OperationErrors for Known {
    fn decode(_: u16, name: Option<&str>, _: &HeaderMap, _: &[u8]) -> Option<Self> {
        (name == Some("Known")).then_some(Self)
    }
    fn info(&self) -> &'static ErrorInfo {
        &ERROR
    }
}
struct Probe;
impl Operation for Probe {
    const AUTH_BEARER: bool = true;
    const INFO: &'static OperationInfo = &OperationInfo {
        id: "test#Probe",
        name: "Probe",
        resource: None,
        method: "POST",
        path: &[],
        literal_query: &[],
        status: 200,
        input: &[],
        output: &[],
        errors: &[],
        readonly: true,
        idempotent: false,
        idempotency_token: None,
        traits: &[],
    };
    type Input = Unit;
    type Output = Unit;
}

#[test]
fn modeled_and_unknown_errors_expose_identical_http_metadata() {
    let mut headers = HeaderMap::new();
    headers.insert("x-request-id", HeaderValue::from_static("request-17"));
    headers.insert("retry-after", HeaderValue::from_static("2"));
    for name in ["Known", "Future"] {
        let wire = format!("{{\"__type\":\"{name}\"}}");
        let error = decode_error::<Known>(
            crucible_wire::operation::Protocol::CrucibleJson,
            429,
            &headers,
            wire.as_bytes(),
        );
        let metadata = error.metadata().unwrap();
        assert_eq!(metadata.status, 429);
        assert_eq!(metadata.request_id.as_deref(), Some("request-17"));
        assert_eq!(metadata.headers, headers);
        assert_eq!(matches!(error, Error::Service(_)), name == "Known");
    }
}

#[tokio::test]
async fn deadlines_and_cancellation_include_stalled_credentials() {
    let token = crate::Token::provider(std::future::pending::<Option<String>>);
    let transport = Transport::new(
        Config::new("http://localhost")
            .token(token)
            .timeout(Duration::from_millis(10)),
    );
    assert!(matches!(
        transport.send::<Probe, Known>(&Unit {}).await,
        Err(Error::Timeout)
    ));
    let cancellation = crate::CancellationToken::new();
    let options = CallOptions {
        timeout: Some(Duration::from_secs(10)),
        cancellation: Some(cancellation.clone()),
    };
    let input = Unit {};
    let call = transport.send_with_options::<Probe, Known>(&input, &options);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(call, cancel);
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(matches!(
        transport
            .send_with_options::<Probe, Known>(&Unit {}, &options)
            .await,
        Err(Error::Cancelled)
    ));
}

#[tokio::test]
async fn body_digests_cover_empty_writes_and_raw_bytes() {
    let transport = Transport::new(Config::new("http://localhost").content_sha256(true));
    for (method, body, expected) in [
        (
            Method::POST,
            None,
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        ),
        (
            Method::PUT,
            Some(("application/octet-stream", Bytes::from_static(b"abc"))),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        ),
        (Method::GET, None, None),
    ] {
        let request = transport
            .request::<Known>(
                method,
                "http://localhost",
                &HeaderMap::new(),
                body,
                crate::auth::OperationAuth::of::<Probe>(),
                None,
                &crate::checksum::Plan::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            request
                .headers()
                .get("x-amz-content-sha256")
                .and_then(|h| h.to_str().ok()),
            expected
        );
    }
}

#[tokio::test]
async fn total_deadline_covers_response_body_and_retry_after() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for reply in [
        "HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n",
        "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nRetry-After: 100\r\n\r\n",
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 2048];
            let mut request = Vec::new();
            loop {
                let count = socket.read(&mut buf).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buf[..count]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            socket.write_all(reply.as_bytes()).await.unwrap();
            std::future::pending::<()>().await;
        });
        let transport = Transport::new(Config::new(endpoint).timeout(Duration::from_millis(80)));
        assert!(matches!(
            transport.send::<Probe, Known>(&Unit {}).await,
            Err(Error::Timeout)
        ));
        server.abort();
    }
}

#[tokio::test]
async fn stream_constraints_are_checked_incrementally() {
    for (body, expected) in [
        (
            ByteStream::from_bytes("ab").with_bounds(Some(3), None),
            std::io::ErrorKind::UnexpectedEof,
        ),
        (
            ByteStream::from_bytes("abcd").with_bounds(None, Some(3)),
            std::io::ErrorKind::InvalidData,
        ),
        (
            ByteStream::from_bytes("abc").with_length(4),
            std::io::ErrorKind::UnexpectedEof,
        ),
    ] {
        let mut stream = body.open().unwrap();
        loop {
            match stream.next().await {
                Some(Ok(_)) => continue,
                Some(Err(error)) => {
                    assert_eq!(error.kind(), expected);
                    break;
                }
                None => panic!("stream bounds must fail"),
            }
        }
        assert!(stream.next().await.is_none());
    }
}
