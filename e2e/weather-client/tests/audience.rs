use crucible_client_runtime_rust::{Config, Error, Retry};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod external {
    include!(concat!(env!("OUT_DIR"), "/audience_external.rs"));
}
#[allow(dead_code)]
mod internal {
    include!(concat!(env!("OUT_DIR"), "/audience_internal.rs"));
}

async fn endpoint(header: bool) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4096];
        let mut received = Vec::new();
        loop {
            let length = stream.read(&mut request).await.unwrap();
            assert_ne!(length, 0);
            received.extend_from_slice(&request[..length]);
            if let Some(headers_end) = received.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&received[..headers_end]);
                let body_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if received.len() >= headers_end + 4 + body_length {
                    break;
                }
            }
        }
        let body = r#"{"__type":"InternalFault","message":"private failure"}"#;
        let discriminator = if header {
            "X-Crucible-Error: InternalFault\r\n"
        } else {
            ""
        };
        stream.write_all(format!("HTTP/1.1 400 Bad Request\r\n{discriminator}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    format!("http://{address}")
}

#[tokio::test]
async fn removed_error_uses_unknown_path_even_when_public_error_has_same_status() {
    for header in [true, false] {
        let client =
            external::ApiClient::new(Config::new(endpoint(header).await).retry(Retry::none()));
        match client
            .public(external::PublicInput::builder().build())
            .await
        {
            Err(Error::Unknown(error)) => assert_eq!(error.metadata.status, 400),
            other => panic!("expected unknown internal error, got {other:?}"),
        }
        let client =
            internal::ApiClient::new(Config::new(endpoint(header).await).retry(Retry::none()));
        match client
            .public(internal::PublicInput::builder().build())
            .await
        {
            Err(Error::Service(error)) => assert!(matches!(
                error.error,
                internal::PublicError::InternalFault(_)
            )),
            other => panic!("expected modeled internal error, got {other:?}"),
        }
    }
}
