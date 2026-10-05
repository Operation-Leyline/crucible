//! A loopback HTTP/1.1 server for client features the server framework does
//! not serve: each request is answered by a test's handler.
#![allow(dead_code)]

mod request;

pub use request::Request;

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Status, headers and body.
pub type Response = (u16, Vec<(String, String)>, Vec<u8>);

/// Serves every connection with `handler`; returns the endpoint.
pub async fn serve(handler: impl Fn(Request) -> Response + Send + Sync + 'static) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let Some(request) = Request::read(&mut stream).await else {
                    return;
                };
                let (status, headers, body) = handler(request);
                let mut head = format!("HTTP/1.1 {status} Status\r\nconnection: close\r\n");
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    endpoint
}

impl Request {
    async fn read(stream: &mut tokio::net::TcpStream) -> Option<Self> {
        let mut bytes = Vec::new();
        let end = loop {
            let mut chunk = [0; 4096];
            let size = stream.read(&mut chunk).await.ok()?;
            if size == 0 {
                return None;
            }
            bytes.extend_from_slice(&chunk[..size]);
            if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break end;
            }
        };
        let head = String::from_utf8(bytes[..end].to_vec()).ok()?;
        let mut lines = head.split("\r\n");
        let mut start = lines.next()?.split(' ');
        let method = start.next()?.to_owned();
        let target = start.next()?.to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        let length: usize = headers
            .iter()
            .find(|(name, _)| name == "content-length")
            .map_or(0, |(_, value)| value.parse().unwrap());
        let mut body = bytes[end + 4..].to_vec();
        while body.len() < length {
            let mut chunk = [0; 4096];
            let size = stream.read(&mut chunk).await.ok()?;
            if size == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..size]);
        }
        Some(Self {
            method,
            target,
            headers,
            body,
        })
    }
}
