//! HTTP checksums over loopback HTTP with the shared `checksums` fixture and
//! vectors: `Content-MD5`, the seven flexible algorithms, default CRC32,
//! response validation and its reporting, preserved caller checksums,
//! policies, and a streamed variant verified as it is read.
mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crucible_client_runtime_rust::OpenEnum;
use crucible_client_runtime_rust::{
    Blob, ByteStream, ChecksumAlgorithm, ChecksumPolicy, Config, Error, Interceptor, Retry,
};
use futures_util::StreamExt;

#[allow(dead_code)]
mod checksum {
    include!(concat!(env!("OUT_DIR"), "/checksum_client.rs"));
}
#[allow(dead_code)]
mod streaming {
    include!(concat!(env!("OUT_DIR"), "/checksum_streaming_client.rs"));
}

struct Vectors {
    input_utf8: String,
    base64: BTreeMap<String, String>,
    empty_md5: String,
}

fn vectors() -> Vectors {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/conformance/checksums.json")).unwrap();
    Vectors {
        input_utf8: value["inputUtf8"].as_str().unwrap().to_owned(),
        base64: serde_json::from_value(value["base64"].clone()).unwrap(),
        empty_md5: value["emptyMd5"].as_str().unwrap().to_owned(),
    }
}

/// Answers with the shared payload and a checksum header for the request's
/// algorithm (CRC32 by default), corrupted or omitted as the mode asks.
async fn server(mode: Arc<Mutex<&'static str>>, seen: Arc<Mutex<Vec<common::Request>>>) -> String {
    let vectors = vectors();
    common::serve(move |request| {
        seen.lock().unwrap().push(request.clone());
        let mode = *mode.lock().unwrap();
        let selected = request.header("x-algorithm").unwrap_or("CRC32").to_owned();
        let mut headers = vec![("x-amz-checksum-xxhash3".to_owned(), "unmodeled".to_owned())];
        if mode != "unmodeled" {
            let value = if mode == "bad" {
                "AAAA".to_owned()
            } else {
                vectors.base64[&selected].clone()
            };
            headers.push((format!("x-amz-checksum-{}", selected.to_lowercase()), value));
        }
        if mode == "one" {
            headers.push(("x-amz-checksum-sha256".into(), "AAAA".into()));
        }
        (200, headers, vectors.input_utf8.clone().into_bytes())
    })
    .await
}

type Validated = Arc<Mutex<Vec<(&'static str, ChecksumAlgorithm)>>>;

fn config(endpoint: &str, validated: &Validated) -> Config {
    let validated = validated.clone();
    Config::new(endpoint)
        .retry(Retry::none())
        .on_checksum_validated(move |operation, algorithm| {
            validated.lock().unwrap().push((operation, algorithm))
        })
}

#[tokio::test]
async fn request_checksums_response_validation_and_overrides() {
    let vectors = vectors();
    let data = Blob::new(vectors.input_utf8.clone().into_bytes());
    let mode = Arc::new(Mutex::new("normal"));
    let seen = Arc::new(Mutex::new(Vec::<common::Request>::new()));
    let validated: Validated = Arc::default();
    let endpoint = server(mode.clone(), seen.clone()).await;
    let client = checksum::ChecksumsClient::new(config(&endpoint, &validated));
    let last = || seen.lock().unwrap().last().cloned().unwrap();
    let input = checksum::Input::builder().data(data.clone()).build();

    // Content-MD5: the legacy trait, and a required checksum with no algorithm member.
    client.legacy(input.clone()).await.unwrap();
    assert_eq!(
        last().header("content-md5"),
        Some(vectors.base64["MD5"].as_str())
    );
    client.required(input.clone()).await.unwrap();
    assert_eq!(
        last().header("content-md5"),
        Some(vectors.base64["MD5"].as_str())
    );
    client
        .legacy(checksum::Input::builder().build())
        .await
        .unwrap();
    assert_eq!(
        last().header("content-md5"),
        Some(vectors.empty_md5.as_str())
    );
    assert!(validated.lock().unwrap().is_empty());

    // Defaults: CRC32 and validation, sent without changing the input.
    let output = client.flexible(input.clone()).await.unwrap();
    assert_eq!(
        output.data().unwrap().as_bytes(),
        vectors.input_utf8.as_bytes()
    );
    let request = last();
    assert_eq!(request.header("x-algorithm"), Some("CRC32"));
    assert_eq!(request.header("x-validation"), Some("ENABLED"));
    assert_eq!(
        request.header("x-amz-checksum-crc32"),
        Some(vectors.base64["CRC32"].as_str())
    );
    assert!(input.algorithm.is_absent() && input.validation.is_absent());
    assert_eq!(
        validated.lock().unwrap().last(),
        Some(&("Flexible", ChecksumAlgorithm::Crc32))
    );

    for (name, expected) in &vectors.base64 {
        let selected = checksum::Input::builder()
            .data(data.clone())
            .algorithm(checksum::Algorithm::from_string(name.clone()))
            .build();
        client.flexible(selected).await.unwrap();
        let header = format!("x-amz-checksum-{}", name.to_lowercase());
        assert_eq!(last().header(&header), Some(expected.as_str()), "{name}");
        assert_eq!(
            validated.lock().unwrap().last(),
            Some(&("Flexible", ChecksumAlgorithm::from_name(name).unwrap()))
        );
    }

    *mode.lock().unwrap() = "bad";
    let error = client.flexible(input.clone()).await.unwrap_err();
    assert!(
        matches!(error, Error::ChecksumMismatch(ChecksumAlgorithm::Crc32)),
        "{error:?}"
    );
    // The first modeled algorithm the response carries is the one checked.
    *mode.lock().unwrap() = "one";
    client.flexible(input.clone()).await.unwrap();
    *mode.lock().unwrap() = "unmodeled";
    let count = validated.lock().unwrap().len();
    client.flexible(input.clone()).await.unwrap();
    assert_eq!(validated.lock().unwrap().len(), count, "unmodeled checksum");

    // A caller's own checksum header is sent unchanged.
    *mode.lock().unwrap() = "unmodeled";
    let explicit = checksum::Input::builder()
        .data(data.clone())
        .checksum("supplied")
        .build();
    client.flexible(explicit).await.unwrap();
    assert_eq!(last().header("x-amz-checksum-crc32"), Some("supplied"));

    let unsupported = checksum::Input::builder()
        .data(data.clone())
        .algorithm(checksum::Algorithm::Xxhash64)
        .build();
    let error = client.flexible(unsupported).await.unwrap_err();
    assert!(matches!(&error, Error::Invalid(message) if message.contains("XXHASH64")));

    // Opting out leaves optional checksums and validation to the caller.
    *mode.lock().unwrap() = "bad";
    let optout = checksum::ChecksumsClient::new(
        config(&endpoint, &validated)
            .request_checksum_calculation(ChecksumPolicy::WhenRequired)
            .response_checksum_validation(ChecksumPolicy::WhenRequired),
    );
    optout.flexible(input.clone()).await.unwrap();
    let request = last();
    assert_eq!(request.header("x-algorithm"), None);
    assert_eq!(request.header("x-validation"), None);
    assert_eq!(request.header("x-amz-checksum-crc32"), None);
}

#[tokio::test]
async fn checksums_cover_the_bytes_interceptors_send_and_keep_theirs() {
    struct Replace(Vec<u8>);
    impl Interceptor for Replace {
        fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
            *request.body_mut() = Some(self.0.clone().into());
            Ok(())
        }
    }
    struct Supply;
    impl Interceptor for Supply {
        fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
            request
                .headers_mut()
                .insert("content-md5", "supplied".parse().unwrap());
            Ok(())
        }
    }
    let vectors = vectors();
    let mode = Arc::new(Mutex::new("normal"));
    let seen = Arc::new(Mutex::new(Vec::<common::Request>::new()));
    let validated: Validated = Arc::default();
    let endpoint = server(mode, seen.clone()).await;
    let hooked = checksum::ChecksumsClient::new(
        config(&endpoint, &validated).interceptor(Replace(vectors.input_utf8.clone().into())),
    );
    hooked
        .legacy(checksum::Input::builder().build())
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.body, vectors.input_utf8.as_bytes());
    assert_eq!(
        request.header("content-md5"),
        Some(vectors.base64["MD5"].as_str())
    );
    let supplied =
        checksum::ChecksumsClient::new(config(&endpoint, &validated).interceptor(Supply));
    supplied
        .legacy(checksum::Input::builder().build())
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.header("content-md5"), Some("supplied"));
}

async fn drain(stream: &ByteStream) -> std::io::Result<Vec<u8>> {
    let mut chunks = stream.open()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = chunks.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    Ok(bytes)
}

#[tokio::test]
async fn streamed_bodies_are_verified_as_they_are_read() {
    let vectors = vectors();
    let mode = Arc::new(Mutex::new("normal"));
    let seen = Arc::new(Mutex::new(Vec::<common::Request>::new()));
    let validated: Validated = Arc::default();
    let endpoint = server(mode.clone(), seen.clone()).await;
    let client = streaming::ChecksumsClient::new(config(&endpoint, &validated));
    let body = || ByteStream::from_bytes(vectors.input_utf8.clone().into_bytes());

    // An optional checksum cannot be computed before a stream is sent: none is.
    let output = client
        .flexible(streaming::Input::builder().data(body()).build())
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(request.header("x-algorithm"), None);
    assert_eq!(request.header("x-amz-checksum-crc32"), None);
    assert_eq!(request.header("x-validation"), Some("ENABLED"));
    assert!(
        validated.lock().unwrap().is_empty(),
        "verified only at the end"
    );
    assert_eq!(
        drain(&output.data).await.unwrap(),
        vectors.input_utf8.as_bytes()
    );
    assert_eq!(
        validated.lock().unwrap().last(),
        Some(&("Flexible", ChecksumAlgorithm::Crc32))
    );

    *mode.lock().unwrap() = "bad";
    let output = client
        .flexible(streaming::Input::builder().data(body()).build())
        .await
        .unwrap();
    let error = drain(&output.data).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    // A chosen algorithm needs a caller-supplied value for a stream.
    let chosen = || {
        streaming::Input::builder()
            .data(body())
            .algorithm(streaming::Algorithm::Crc32)
    };
    let error = client.flexible(chosen().build()).await.unwrap_err();
    assert!(matches!(&error, Error::Invalid(message) if message.contains("x-amz-checksum-crc32")));
    *mode.lock().unwrap() = "normal";
    client
        .flexible(chosen().checksum(vectors.base64["CRC32"].clone()).build())
        .await
        .unwrap();
    let request = seen.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        request.header("x-amz-checksum-crc32"),
        Some(vectors.base64["CRC32"].as_str())
    );
    // A legacy MD5 checksum is required, so a stream without one fails.
    let error = client
        .legacy(streaming::Input::builder().data(body()).build())
        .await
        .unwrap_err();
    assert!(matches!(&error, Error::Invalid(message) if message.contains("content-md5")));
}
