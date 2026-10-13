use super::*;
use crucible_wire::operation::Fault;
use crucible_wire::operation::{OperationInfo, Unit};

const fn response_header(scalar: Scalar) -> MemberInfo {
    crate::__generated::member_info(
        "names",
        Location::Header("x-names"),
        scalar,
        &crucible_wire::operation::Redaction::Public,
        &[],
    )
}

#[test]
fn repeated_response_header_lines_preserve_list_values() {
    static MEMBERS: [MemberInfo; 1] = [response_header(Scalar::List(&Scalar::String))];
    for (lines, expected) in [
        (vec!["a", "b"], vec!["a", "b"]),
        (vec!["a, b"], vec!["a", "b"]),
        (vec!["a", "\"b,c\", d"], vec!["a", "b,c", "d"]),
    ] {
        let mut headers = HeaderMap::new();
        for line in &lines {
            headers.append("x-names", HeaderValue::from_str(line).unwrap());
        }
        let output: Value = read_response(200, &MEMBERS, &headers, b"{}", Bytes::new).unwrap();
        assert_eq!(output["names"], serde_json::json!(expected));
    }
}

#[test]
fn non_ascii_response_headers_are_decode_errors() {
    static SCALAR: [MemberInfo; 1] = [response_header(Scalar::String)];
    static LIST: [MemberInfo; 1] = [response_header(Scalar::List(&Scalar::String))];
    for (members, valid_first) in [(&SCALAR, false), (&LIST, false), (&LIST, true)] {
        let mut headers = HeaderMap::new();
        if valid_first {
            headers.append("x-names", HeaderValue::from_static("a"));
        }
        headers.append("x-names", HeaderValue::from_bytes(b"\xff").unwrap());
        let error =
            decode_output::<Probe, Known>(200, members, &headers, &Bytes::from_static(b"{}"))
                .unwrap_err();
        assert!(matches!(error, Error::Decode(message) if message.contains("x-names")));
    }
}

#[test]
fn absent_response_headers_and_scalar_first_line_are_preserved() {
    static SCALAR: [MemberInfo; 1] = [response_header(Scalar::String)];
    static LIST: [MemberInfo; 1] = [response_header(Scalar::List(&Scalar::String))];
    let mut headers = HeaderMap::new();
    for members in [&SCALAR, &LIST] {
        let output: Value = read_response(200, members, &headers, b"{}", Bytes::new).unwrap();
        assert_eq!(output, serde_json::json!({}));
    }
    headers.append("x-names", HeaderValue::from_static("a"));
    headers.append("x-names", HeaderValue::from_static("b"));
    let output: Value = read_response(200, &SCALAR, &headers, b"{}", Bytes::new).unwrap();
    assert_eq!(output["names"], "a");
}

#[test]
fn response_bodies_cannot_supply_header_bound_members() {
    static MEMBERS: [MemberInfo; 2] = [
        response_header(Scalar::String),
        crate::__generated::member_info(
            "message",
            Location::Body,
            Scalar::String,
            &crucible_wire::operation::Redaction::Public,
            &[],
        ),
    ];
    static HEADER_ERROR: ErrorInfo =
        crate::__generated::error_info("HeaderError", 400, Fault::Client, false, false, &MEMBERS);
    struct HeaderOutput;
    impl Operation for HeaderOutput {
        const INFO: &'static OperationInfo = Probe::INFO;
        type Input = Unit;
        type Output = Value;
    }
    let body = Bytes::from_static(br#"{"names":"from-body","message":"kept"}"#);
    for value in [None, Some("from-header")] {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert("x-names", HeaderValue::from_static(value));
        }
        let expected = match value {
            Some(value) => serde_json::json!({"names": value, "message": "kept"}),
            None => serde_json::json!({"message": "kept"}),
        };
        let output = decode_output::<HeaderOutput, Known>(200, &MEMBERS, &headers, &body).unwrap();
        assert_eq!(output, expected);
        let error: Value = decode_modelled_at(400, &HEADER_ERROR, &headers, &body).unwrap();
        assert_eq!(error, expected);
    }
}

async fn request_headers(socket: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = socket.read(&mut buffer).await.unwrap();
        assert_ne!(count, 0);
        request.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(request).unwrap()
}

async fn redirect_attempt<O: Operation<Input = Unit, Output = Unit>>(
    configure: impl FnOnce(Config) -> Config,
) -> (Result<Unit, Error<Known>>, String, Option<String>) {
    use tokio::io::AsyncWriteExt;
    let source = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", source.local_addr().unwrap());
    let location = format!("http://{}/redirected", target.local_addr().unwrap());
    let source_task = tokio::spawn(async move {
        let (mut socket, _) = source.accept().await.unwrap();
        let request = request_headers(&mut socket).await;
        socket.write_all(format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        request
    });
    let mut target_task = tokio::spawn(async move {
        let (mut socket, _) = target.accept().await.unwrap();
        let request = request_headers(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
        request
    });
    let transport = Transport::new(
        configure(Config::new(endpoint))
            .retry(crate::Retry::none())
            .timeout(Duration::from_secs(5)),
    );
    let result = transport.send::<O, Known>(&Unit {}).await;
    // A followed redirect must finish before the call returns. Start this
    // observation window afterward so client setup cannot expire it early.
    let target = match tokio::time::timeout(Duration::from_millis(100), &mut target_task).await {
        Ok(request) => Some(request.unwrap()),
        Err(_) => {
            target_task.abort();
            None
        }
    };
    (result, source_task.await.unwrap(), target)
}

struct ApiKeyProbe;
impl Operation for ApiKeyProbe {
    const INFO: &'static OperationInfo = Probe::INFO;
    const AUTH_SCHEMES: Option<&'static [crucible_wire::operation::AuthScheme]> =
        Some(&[crucible_wire::operation::AuthScheme::ApiKey {
            name: "x-api-key",
            query: false,
            scheme: None,
        }]);
    type Input = Unit;
    type Output = Unit;
}

struct SigV4Probe;
impl Operation for SigV4Probe {
    const INFO: &'static OperationInfo = Probe::INFO;
    const AUTH_SCHEMES: Option<&'static [crucible_wire::operation::AuthScheme]> =
        Some(&[crucible_wire::operation::AuthScheme::SigV4 {
            signing_name: "test",
        }]);
    type Input = Unit;
    type Output = Unit;
}

#[tokio::test]
async fn default_client_does_not_redirect_api_keys_to_another_origin() {
    let (result, source, target) = redirect_attempt::<ApiKeyProbe>(|config| {
        config.api_key(crate::Token::fixed("test-api-key"))
    })
    .await;
    assert!(source.contains("x-api-key: test-api-key"));
    assert!(target.is_none(), "the redirect target received a request");
    assert!(matches!(result, Err(Error::Unknown(error)) if error.metadata.status == 302));
}

#[tokio::test]
async fn default_client_does_not_redirect_sigv4_session_tokens_to_another_origin() {
    let mut credentials = crate::AwsCredentials::new("test-access-key", "test-secret-key");
    credentials.session_token = Some("test-session-token".into());
    let (result, source, target) = redirect_attempt::<SigV4Probe>(|config| {
        config
            .region("us-west-2")
            .aws_credentials(crate::Identity::fixed(credentials))
    })
    .await;
    assert!(source.contains("x-amz-security-token: test-session-token"));
    assert!(source.contains("authorization: AWS4-HMAC-SHA256"));
    assert!(target.is_none(), "the redirect target received a request");
    assert!(matches!(result, Err(Error::Unknown(error)) if error.metadata.status == 302));
}

#[tokio::test]
async fn custom_http_client_can_explicitly_enable_redirects() {
    let (result, _, target) = redirect_attempt::<Probe>(|config| {
        config
            .token(crate::Token::fixed("test-bearer-token"))
            .http_client(
                reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::limited(1))
                    .build()
                    .unwrap(),
            )
    })
    .await;
    assert!(result.is_ok());
    assert!(target.is_some());
}

#[test]
fn modeled_errors_cannot_read_absent_header_members_from_the_body() {
    static MEMBERS: [MemberInfo; 1] = [response_header(Scalar::String)];
    static HEADER_ERROR: ErrorInfo =
        crate::__generated::error_info("HeaderError", 400, Fault::Client, false, false, &MEMBERS);
    #[derive(Debug, serde::Deserialize)]
    struct HeaderError {
        names: Option<String>,
    }
    let error: HeaderError = decode_modelled_at(
        400,
        &HEADER_ERROR,
        &HeaderMap::new(),
        br#"{"names":"from-body"}"#,
    )
    .unwrap();
    assert_eq!(error.names, None);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct UploadInput<T> {
    data: T,
    content_type: Option<String>,
}
impl<T> crucible_wire::validate::Validate for UploadInput<T> {
    fn validate(&self, _: &mut crucible_wire::validate::Validator) {}
}

const fn upload_member(name: &'static str, location: Location) -> MemberInfo {
    crate::__generated::member_info(
        name,
        location,
        Scalar::String,
        &crucible_wire::operation::Redaction::Public,
        &[],
    )
}
const fn upload_info(members: &'static [MemberInfo]) -> OperationInfo {
    crate::__generated::operation_info(
        "test#Upload",
        "Upload",
        None,
        "PUT",
        &[],
        &[],
        200,
        members,
        &[],
        &[],
        false,
        false,
        false,
        None,
        &[],
    )
}

struct BufferedUpload;
impl Operation for BufferedUpload {
    const PROTOCOL: Protocol = Protocol::AwsRestJson1;
    const INFO: &'static OperationInfo = &upload_info(&[
        upload_member("data", Location::Payload("application/octet-stream")),
        upload_member("content_type", Location::Header("content-type")),
    ]);
    type Input = UploadInput<crate::Blob>;
    type Output = Unit;
}

struct StreamingUpload;
impl Operation for StreamingUpload {
    const PROTOCOL: Protocol = Protocol::AwsRestJson1;
    const INFO: &'static OperationInfo = &upload_info(&[
        upload_member(
            "data",
            Location::StreamingPayload {
                media_type: "application/octet-stream",
                requires_length: true,
                minimum: None,
                maximum: None,
            },
        ),
        upload_member("content_type", Location::Header("content-type")),
    ]);
    type Input = UploadInput<ByteStream>;
    type Output = Unit;
    fn input_stream(input: &Self::Input) -> Option<ByteStream> {
        Some(input.data.clone())
    }
}

async fn upload_content_type<O: Operation<Output = Unit>>(
    input: &O::Input,
    configured: Option<&'static str>,
) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = request_headers(&mut socket).await;
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        let header = |name: &str| {
            headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case(name)
                        .then(|| value.trim().to_owned())
                })
                .unwrap()
        };
        let content_type = header("content-type");
        let length: usize = header("content-length").parse().unwrap();
        let mut body = body.as_bytes().to_vec();
        if body.len() < length {
            let mut remaining = vec![0; length - body.len()];
            socket.read_exact(&mut remaining).await.unwrap();
            body.extend(remaining);
        }
        assert_eq!(body, b"png-data");
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
        content_type
    });
    let mut config = Config::new(endpoint)
        .retry(crate::Retry::none())
        .timeout(Duration::from_secs(5));
    if let Some(content_type) = configured {
        config = config.header(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static(content_type),
        );
    }
    Transport::new(config)
        .send::<O, Known>(input)
        .await
        .unwrap();
    server.await.unwrap()
}

#[tokio::test]
async fn buffered_upload_content_type_preserves_explicit_headers_and_defaults_when_absent() {
    for (modeled, configured, expected) in [
        (Some("image/png"), None, "image/png"),
        (None, None, "application/octet-stream"),
        (None, Some("image/jpeg"), "image/jpeg"),
        (Some("image/png"), Some("image/jpeg"), "image/png"),
    ] {
        let input = UploadInput {
            data: crate::Blob::new(Bytes::from_static(b"png-data")),
            content_type: modeled.map(str::to_owned),
        };
        assert_eq!(
            upload_content_type::<BufferedUpload>(&input, configured).await,
            expected
        );
    }
}

#[tokio::test]
async fn streaming_upload_content_type_preserves_explicit_headers_and_defaults_when_absent() {
    for (modeled, configured, expected) in [
        (Some("image/png"), None, "image/png"),
        (None, None, "application/octet-stream"),
        (None, Some("image/jpeg"), "image/jpeg"),
        (Some("image/png"), Some("image/jpeg"), "image/png"),
    ] {
        let input = UploadInput {
            data: ByteStream::from_bytes(Bytes::from_static(b"png-data")),
            content_type: modeled.map(str::to_owned),
        };
        assert_eq!(
            upload_content_type::<StreamingUpload>(&input, configured).await,
            expected
        );
    }
}

#[derive(Debug)]
struct Known;
static ERROR: ErrorInfo =
    crate::__generated::error_info("Known", 429, Fault::Client, true, true, &[]);
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
    const INFO: &'static OperationInfo = &crate::__generated::operation_info(
        "test#Probe",
        "Probe",
        None,
        "POST",
        &[],
        &[],
        200,
        &[],
        &[],
        &[],
        true,
        false,
        false,
        None,
        &[],
    );
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

#[tokio::test(start_paused = true)]
async fn expired_deadlines_do_not_poll_credentials() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Stay between timer ticks so a zero budget cannot rely on timer readiness.
    tokio::time::advance(Duration::from_nanos(1)).await;
    for configured in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let token = crate::Token::provider(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            std::future::ready(None)
        });
        let timeout = if configured {
            Duration::ZERO
        } else {
            Duration::from_secs(1)
        };
        let transport = Transport::new(
            Config::new("http://localhost")
                .token(token)
                .timeout(timeout),
        );
        let mut options = CallOptions {
            timeout: (!configured).then_some(Duration::ZERO),
            cancellation: None,
        };
        assert!(matches!(
            transport
                .send_with_options::<Probe, Known>(&Unit {}, &options)
                .await,
            Err(Error::Timeout)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let cancellation = crate::CancellationToken::new();
        cancellation.cancel();
        options.cancellation = Some(cancellation);
        assert!(matches!(
            transport
                .send_with_options::<Probe, Known>(&Unit {}, &options)
                .await,
            Err(Error::Cancelled)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
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
                None,
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

struct RpcJsonProbe;
impl Operation for RpcJsonProbe {
    const PROTOCOL: Protocol = Protocol::SmithyRpcV2Json;
    const INFO: &'static OperationInfo = Probe::INFO;
    type Input = Unit;
    type Output = Unit;
}
struct RpcCborProbe;
impl Operation for RpcCborProbe {
    const PROTOCOL: Protocol = Protocol::SmithyRpcV2Cbor;
    const INFO: &'static OperationInfo = Probe::INFO;
    type Input = Unit;
    type Output = Unit;
}
struct UnsafeRpcProbe;
impl Operation for UnsafeRpcProbe {
    const PROTOCOL: Protocol = Protocol::SmithyRpcV2Json;
    const INFO: &'static OperationInfo = &crate::__generated::operation_info(
        Probe::INFO.id,
        Probe::INFO.name,
        Probe::INFO.resource,
        Probe::INFO.method,
        Probe::INFO.path,
        Probe::INFO.literal_query,
        Probe::INFO.status,
        Probe::INFO.input,
        Probe::INFO.output,
        Probe::INFO.errors,
        false,
        Probe::INFO.idempotent,
        Probe::INFO.internal,
        Probe::INFO.idempotency_token,
        Probe::INFO.traits,
    );
    type Input = Unit;
    type Output = Unit;
}

async fn rpc_missing_header_attempts<O: Operation<Input = Unit, Output = Unit>>(
    status: u16,
    expected: usize,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let size = socket.read(&mut chunk).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            count.fetch_add(1, Ordering::SeqCst);
            socket.write_all(format!("HTTP/1.1 {status} Result\r\nContent-Length: 5\r\nRetry-After: 0\r\nConnection: close\r\n\r\nproxy").as_bytes()).await.unwrap();
        }
    });
    let error = Transport::new(Config::new(endpoint).retry(crate::Retry {
        max_attempts: 3,
        base_delay: Duration::ZERO,
        ..crate::Retry::default()
    }))
    .send::<O, Known>(&Unit {})
    .await
    .unwrap_err();
    assert!(matches!(error, Error::Decode(message) if message.contains("smithy-protocol")));
    assert_eq!(attempts.load(Ordering::SeqCst), expected, "HTTP {status}");
    server.abort();
}

#[tokio::test]
async fn rpc_v2_missing_protocol_header_retries_only_safe_transient_statuses() {
    for status in [500, 502, 503, 504] {
        rpc_missing_header_attempts::<RpcJsonProbe>(status, 3).await;
        rpc_missing_header_attempts::<RpcCborProbe>(status, 3).await;
        rpc_missing_header_attempts::<UnsafeRpcProbe>(status, 1).await;
    }
    for status in [200, 400, 429, 501] {
        rpc_missing_header_attempts::<RpcJsonProbe>(status, 1).await;
        rpc_missing_header_attempts::<RpcCborProbe>(status, 1).await;
    }
}
