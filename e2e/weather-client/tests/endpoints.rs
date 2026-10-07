use crucible_client_runtime_rust::{
    CancellationToken, Config, Error, Retry, WaiterFailure, WaiterOptions, WaiterResult,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
mod sdk {
    include!(concat!(env!("OUT_DIR"), "/endpoint_client.rs"));
}

async fn serve(
    replies: Vec<(u16, &'static str)>,
) -> (Config, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = Config::new(format!("http://localhost:{}", address.port()))
        .region("local")
        .retry(Retry::none());
    let config = config.http_client(
        reqwest::Client::builder()
            .no_proxy()
            .resolve("customer.localhost", address)
            .build()
            .unwrap(),
    );
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for (status, body) in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let size = stream.read(&mut chunk).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&chunk[..size]);
                if let Some(index) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..index]);
                    let length = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .and_then(|length| length.parse::<usize>().ok())
                        .unwrap_or(0);
                    if bytes.len() >= index + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            stream.write_all(format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        }
        requests
    });
    (config, task)
}
fn input() -> sdk::model::Input {
    sdk::model::Input::builder().account("customer").build()
}
fn options() -> WaiterOptions {
    let mut options = WaiterOptions::new(Duration::from_secs(3));
    options.min_delay = Some(Duration::ZERO);
    options.max_delay = Some(Duration::ZERO);
    options
}

#[tokio::test]
async fn generated_endpoint_rules_host_prefix_and_waiter_collision_over_http() {
    let (config, requests) = serve(vec![
        (200, r#"{"wire-status":"PENDING"}"#),
        (200, r#"{"wire-status":"READY"}"#),
        (204, ""),
    ])
    .await;
    let client = sdk::ServiceClient::new(config);
    let result = client.wait_until_ready_2(input(), options()).await.unwrap();
    match result {
        WaiterResult::Response { output, attempts } => {
            assert_eq!(attempts, 2);
            assert_eq!(output.status.value().unwrap(), "READY");
        }
        other => panic!("{other:?}"),
    }
    client.wait_until_ready().await.unwrap();
    let requests = requests.await.unwrap();
    assert!(requests[0].starts_with("POST /base/resource HTTP/1.1"));
    assert!(requests[0].contains("host: customer.localhost:"));
    assert!(requests[0].contains("x-region: local"));
    assert!(requests[0].ends_with(r#"{"account":"customer"}"#));
    assert!(requests[2].starts_with("GET /base/normal"));
}

#[tokio::test]
async fn generated_waiter_reserves_budget_for_one_final_http_poll() {
    let (config, requests) = serve(vec![
        (200, r#"{"wire-status":"WAIT"}"#),
        (200, r#"{"wire-status":"READY"}"#),
    ])
    .await;
    let mut budget = WaiterOptions::new(Duration::from_millis(1_500));
    budget.min_delay = Some(Duration::from_secs(1));
    budget.max_delay = Some(Duration::from_secs(1));
    let result = sdk::ServiceClient::new(config)
        .wait_until_ready_2(input(), budget)
        .await
        .unwrap();
    match result {
        WaiterResult::Response { output, attempts } => {
            assert_eq!(attempts, 2);
            assert_eq!(output.status.value().unwrap(), "READY");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(requests.await.unwrap().len(), 2);
}

#[tokio::test]
async fn generated_waiter_normalizes_model_names_timestamps_and_exact_numbers() {
    let (config,requests) = serve(vec![(200,r#"{"wire-status":"READY","time":"Thu, 01 Jan 1970 00:00:00 GMT","amount":9007199254740992.000000001}"#)]).await;
    let input = sdk::model::Input::builder()
        .account("customer")
        .time(chrono::DateTime::from_timestamp(0, 0).unwrap())
        .build();
    assert!(matches!(
        sdk::ServiceClient::new(config)
            .wait_until_exact(input, options())
            .await
            .unwrap(),
        WaiterResult::Response { attempts: 1, .. }
    ));
    requests.await.unwrap();
}

#[tokio::test]
async fn generated_waiter_success_can_return_error_and_unmatched_transport_preserves_cause() {
    let (config, requests) = serve(vec![(404, r#"{"__type":"Missing"}"#)]).await;
    assert!(matches!(
        sdk::ServiceClient::new(config)
            .wait_until_gone(input(), options())
            .await
            .unwrap(),
        WaiterResult::Error {
            error: Error::Service(_),
            attempts: 1
        }
    ));
    requests.await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = Config::new(format!("http://{address}"))
        .region("local")
        .disable_host_prefix(true)
        .retry(Retry::none());
    let failure = sdk::ServiceClient::new(config)
        .wait_until_ready_2(input(), options())
        .await
        .unwrap_err();
    assert_eq!(failure.kind, WaiterFailure::Failure);
    assert!(matches!(
        failure.cause.as_deref(),
        Some(Error::Transport(_))
    ));
    assert!(std::error::Error::source(&failure).is_some());
}

#[tokio::test]
async fn waiter_deadline_cancellation_and_invalid_endpoint_stop_before_extra_polls() {
    let (config, requests) = serve(vec![(200, "{}")]).await;
    let mut budget = WaiterOptions::new(Duration::from_millis(100));
    budget.min_delay = Some(Duration::from_secs(1));
    let failure = sdk::ServiceClient::new(config.clone())
        .wait_until_ready_2(input(), budget)
        .await
        .unwrap_err();
    assert_eq!(failure.kind, WaiterFailure::Timeout);
    assert_eq!(failure.attempts, 1);
    assert!(failure.response.is_some());
    requests.await.unwrap();
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = options();
    cancelled.call.cancellation = Some(token);
    let failure = sdk::ServiceClient::new(config.clone())
        .wait_until_ready_2(input(), cancelled)
        .await
        .unwrap_err();
    assert_eq!(failure.kind, WaiterFailure::Cancelled);
    assert_eq!(failure.attempts, 0);
    let client = sdk::ServiceClient::new(config);
    assert!(matches!(
        client
            .read(sdk::model::Input::builder().account("evil.name").build())
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(
        matches!(sdk::ServiceClient::new(Config::new("https://example.test").region("unknown")).read(input()).await,Err(Error::Invalid(message)) if message.contains("Unknown region"))
    );
}
