use crucible_client::{BasicCredentials, Config, Error, Identity, Retry, Token};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod header {
    include!(concat!(env!("OUT_DIR"), "/auth_header_client.rs"));
}
#[allow(dead_code)]
mod query {
    include!(concat!(env!("OUT_DIR"), "/auth_query_client.rs"));
}
#[allow(dead_code)]
mod sigv4 {
    include!(concat!(env!("OUT_DIR"), "/sigv4_client.rs"));
}

#[tokio::test]
async fn generated_sigv4_refreshes_after_interceptors_and_obeys_endpoint_scope() {
    struct AddHeader;
    impl crucible_client::Interceptor for AddHeader {
        fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
            request
                .headers_mut()
                .insert("x-custom", "signed".parse().unwrap());
            Ok(())
        }
    }
    let (endpoint, requests) = serve(vec![503, 204, 204]).await;
    let identities = Arc::new(AtomicUsize::new(0));
    let count = identities.clone();
    let config = Config::new(endpoint)
        .region("client-region")
        .aws_credentials(Identity::provider(move || {
            let attempt = count.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                let mut value =
                    crucible_client::AwsCredentials::new(format!("key{attempt}"), "secret");
                value.session_token = Some(format!("session{attempt}"));
                Some(value)
            }
        }))
        .interceptor(AddHeader)
        .retry(Retry {
            base_delay: Duration::ZERO,
            ..Retry::default()
        });
    let client = sigv4::AuthHeaderClient::new(config);
    client.mixed().await.unwrap();
    client.public().await.unwrap();
    let requests = requests.await.unwrap();
    assert_eq!(identities.load(Ordering::SeqCst), 2);
    assert!(requests[0].contains("Credential=key1/"));
    assert!(requests[1].contains("Credential=key2/"));
    assert!(requests[1].contains("/rule-region/overridden/aws4_request"));
    assert!(requests[1].contains(";x-custom;x-endpoint"));
    assert!(requests[1].contains("x-amz-security-token: session2"));
    assert!(!requests[2].contains("authorization:"));
}

async fn serve(statuses: Vec<u16>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for status in statuses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let size = stream.read(&mut chunk).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if bytes.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Result\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
        requests
    });
    (endpoint, task)
}

#[tokio::test]
async fn ordered_auth_selection_refreshes_per_attempt() {
    let (endpoint, requests) = serve(vec![503, 204]).await;
    let bearer = Arc::new(AtomicUsize::new(0));
    let basic = Arc::new(AtomicUsize::new(0));
    let bearer_count = bearer.clone();
    let basic_count = basic.clone();
    let config = Config::new(endpoint)
        .token(Token::provider(move || {
            bearer_count.fetch_add(1, Ordering::SeqCst);
            async { None }
        }))
        .basic_credentials(Identity::provider(move || {
            let count = basic_count.fetch_add(1, Ordering::SeqCst) + 1;
            async move { Some(BasicCredentials::new("user", format!("pass{count}"))) }
        }))
        .api_key(Token::provider(|| async {
            panic!("unrelated identity provider")
        }))
        .retry(Retry {
            base_delay: Duration::ZERO,
            ..Retry::default()
        });
    header::AuthHeaderClient::new(config).mixed().await.unwrap();
    let requests = requests.await.unwrap();
    assert!(requests[0].contains("authorization: Basic dXNlcjpwYXNzMQ=="));
    assert!(requests[1].contains("authorization: Basic dXNlcjpwYXNzMg=="));
    assert_eq!(bearer.load(Ordering::SeqCst), 2);
    assert_eq!(basic.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn public_optional_and_explicit_credentials_skip_providers() {
    let (endpoint, requests) = serve(vec![204, 204]).await;
    let config = Config::new(endpoint)
        .token(Token::provider(|| async { panic!("unused token") }))
        .basic_credentials(Identity::provider(|| async {
            panic!("unused basic identity")
        }));
    header::AuthHeaderClient::new(config.clone())
        .public()
        .await
        .unwrap();
    let config = config.header(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_static("Basic explicit"),
    );
    header::AuthHeaderClient::new(config).mixed().await.unwrap();
    let requests = requests.await.unwrap();
    assert!(!requests[0].contains("authorization:"));
    assert!(requests[1].contains("authorization: Basic explicit"));
    let (endpoint, request) = serve(vec![204]).await;
    header::AuthHeaderClient::new(Config::new(endpoint))
        .optional()
        .await
        .unwrap();
    request.await.unwrap();
    assert!(matches!(
        header::AuthHeaderClient::new(Config::new("http://127.0.0.1:1"))
            .basic()
            .await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn api_keys_and_utf8_basic_credentials_use_the_modeled_binding() {
    let (endpoint, requests) = serve(vec![204, 204, 204]).await;
    header::AuthHeaderClient::new(Config::new(&endpoint).api_key(Token::fixed("key")))
        .api()
        .await
        .unwrap();
    query::AuthQueryClient::new(Config::new(&endpoint).api_key(Token::fixed("a b/+")))
        .api()
        .await
        .unwrap();
    header::AuthHeaderClient::new(
        Config::new(&endpoint)
            .basic_credentials(Identity::fixed(BasicCredentials::new("\u{e9}", "p"))),
    )
    .basic()
    .await
    .unwrap();
    let requests = requests.await.unwrap();
    assert!(requests[0].contains("x-api-key: ApiKey key"));
    assert!(requests[1].starts_with("GET /api?key=a+b%2F%2B "));
    assert!(requests[2].contains("authorization: Basic w6k6cA=="));
}

#[tokio::test]
async fn credential_resolution_has_the_total_call_deadline() {
    let config = Config::new("http://127.0.0.1:1")
        .timeout(Duration::from_millis(5))
        .basic_credentials(Identity::provider(|| std::future::pending()));
    assert!(matches!(
        header::AuthHeaderClient::new(config).basic().await,
        Err(Error::Timeout)
    ));
}
