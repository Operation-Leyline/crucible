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
        let output: Value = read_response(&MEMBERS, &headers, b"{}", Bytes::new).unwrap();
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
        let error = decode_output::<Probe, Known>(members, &headers, &Bytes::from_static(b"{}"))
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
        let output: Value = read_response(members, &headers, b"{}", Bytes::new).unwrap();
        assert_eq!(output, serde_json::json!({}));
    }
    headers.append("x-names", HeaderValue::from_static("a"));
    headers.append("x-names", HeaderValue::from_static("b"));
    let output: Value = read_response(&SCALAR, &headers, b"{}", Bytes::new).unwrap();
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
        let output = decode_output::<HeaderOutput, Known>(&MEMBERS, &headers, &body).unwrap();
        assert_eq!(output, expected);
        let error: Value = decode_modelled(&HEADER_ERROR, &headers, &body).unwrap();
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

#[tokio::test]
async fn default_client_does_not_redirect_bearer_tokens_to_another_origin() {
    let (result, source, target) =
        redirect_attempt::<Probe>(|config| config.token(crate::Token::fixed("test-bearer-token")))
            .await;
    assert!(source.contains("authorization: Bearer test-bearer-token"));
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
    let error: HeaderError = decode_modelled(
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
    const INFO: &'static OperationInfo = &upload_info(&[
        upload_member("data", Location::Payload("application/octet-stream")),
        upload_member("content_type", Location::Header("content-type")),
    ]);
    type Input = UploadInput<crate::Blob>;
    type Output = Unit;
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
async fn buffered_upload_content_type_prefers_modeled_header_then_media_type_then_config() {
    for (modeled, configured, expected) in [
        (Some("image/png"), None, "image/png"),
        (None, None, "application/octet-stream"),
        // The payload's media type outranks a client-wide default.
        (None, Some("image/jpeg"), "application/octet-stream"),
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

struct JsonBodyProbe;
impl Operation for JsonBodyProbe {
    const INFO: &'static OperationInfo = &upload_info(&[
        upload_member("data", Location::Body),
        upload_member("content_type", Location::Header("content-type")),
    ]);
    type Input = UploadInput<String>;
    type Output = Unit;
}

async fn json_content_type(modeled: Option<&str>, expected: &str) {
    struct Capture(std::sync::Arc<std::sync::Mutex<Option<String>>>);
    impl crate::Interceptor for Capture {
        fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
            assert!(request.body().is_some());
            *self.0.lock().unwrap() = Some(
                request.headers()[http::header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .to_owned(),
            );
            Err("request captured".into())
        }
    }
    let captured = Arc::new(std::sync::Mutex::new(None));
    let config = Config::new("http://localhost")
        .header(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("image/png"),
        )
        .interceptor(Capture(captured.clone()));
    let input = UploadInput {
        data: "json-data".into(),
        content_type: modeled.map(str::to_owned),
    };
    let result = Transport::new(config)
        .send::<JsonBodyProbe, Known>(&input)
        .await;
    assert!(matches!(result, Err(Error::Invalid(message)) if message == "request captured"));
    assert_eq!(captured.lock().unwrap().as_deref(), Some(expected));
}

#[tokio::test]
async fn json_document_content_type_overrides_configured_and_modeled_headers() {
    for modeled in [None, Some("image/jpeg")] {
        json_content_type(modeled, "application/json").await;
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
        let error = decode_error::<Known>(429, &headers, wire.as_bytes());
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
