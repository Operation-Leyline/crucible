//! AWS Signature Version 4, with buffered hashes or explicit streaming hashes.
use crate::AwsCredentials;
use sha2::{Digest, Sha256};

pub(crate) struct Scope<'a> {
    pub(crate) region: &'a str,
    pub(crate) name: &'a str,
    pub(crate) double_encode: bool,
    pub(crate) normalize_path: bool,
    pub(crate) unsigned_payload: bool,
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    crate::transport::encode(&mut out, value);
    out
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn hash(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|byte| byte ^ 0x36));
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(block.map(|byte| byte ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn signing_key(secret: &str, date: &str, region: &str, name: &str) -> [u8; 32] {
    let key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let key = hmac(&key, region.as_bytes());
    let key = hmac(&key, name.as_bytes());
    hmac(&key, b"aws4_request")
}

/// The payload hash a request declares when its body is a signed event
/// stream: each message is signed as it is sent instead.
pub(crate) const STREAMING_EVENTS: &str = "STREAMING-AWS4-HMAC-SHA256-EVENTS";

/// Signs event stream messages in sequence: each signature chains the one
/// before it, starting from the HTTP request's own signature.
#[derive(Clone)]
pub(crate) struct EventSigner {
    secret: String,
    region: String,
    name: String,
    prior: String,
}

impl EventSigner {
    pub(crate) fn new(secret: &str, region: &str, name: &str, seed: String) -> Self {
        Self {
            secret: secret.to_owned(),
            region: region.to_owned(),
            name: name.to_owned(),
            prior: seed,
        }
    }

    /// The `AWS4-HMAC-SHA256-PAYLOAD` string to sign for `payload` (an
    /// encoded message, or nothing for the final empty one) at `time`.
    pub(crate) fn string_to_sign(
        &self,
        payload: &[u8],
        time: chrono::DateTime<chrono::Utc>,
    ) -> String {
        use crucible_wire::event_stream::{Header, HeaderValue, encode_headers};
        let mut date = Vec::new();
        encode_headers(
            &[Header {
                name: ":date".into(),
                value: HeaderValue::Timestamp(time.timestamp() * 1000),
            }],
            &mut date,
        )
        .expect("a date header encodes");
        format!(
            "AWS4-HMAC-SHA256-PAYLOAD
{}
{}/{}/{}/aws4_request
{}
{}
{}",
            time.format("%Y%m%dT%H%M%SZ"),
            time.format("%Y%m%d"),
            self.region,
            self.name,
            self.prior,
            hash(&date),
            hash(payload)
        )
    }

    /// The envelope message carrying `payload`, signed; the signature
    /// becomes the next message's prior one. Sub-second time is dropped, as
    /// the signed date has whole seconds.
    pub(crate) fn sign(
        &mut self,
        payload: bytes::Bytes,
        time: chrono::DateTime<chrono::Utc>,
    ) -> crucible_wire::event_stream::Message {
        use crucible_wire::event_stream::{HeaderValue, Message};
        let time = chrono::DateTime::from_timestamp(time.timestamp(), 0).expect("whole seconds");
        let key = signing_key(
            &self.secret,
            &time.format("%Y%m%d").to_string(),
            &self.region,
            &self.name,
        );
        let signature = hmac(&key, self.string_to_sign(&payload, time).as_bytes());
        self.prior = hex(&signature);
        Message::new(payload)
            .with_header(
                ":chunk-signature",
                HeaderValue::Bytes(bytes::Bytes::copy_from_slice(&signature)),
            )
            .with_header(":date", HeaderValue::Timestamp(time.timestamp() * 1000))
    }
}

pub(crate) fn sign(
    request: &mut reqwest::Request,
    credentials: &AwsCredentials,
    scope: Scope<'_>,
) -> Result<(), String> {
    if credentials.access_key_id.is_empty()
        || credentials.secret_access_key.is_empty()
        || scope.region.is_empty()
        || scope.name.is_empty()
    {
        return Err("SigV4 requires credentials, region, and signing name".into());
    }
    if [&credentials.access_key_id, scope.region, scope.name]
        .into_iter()
        .any(|value| {
            value
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '/' | ','))
        })
    {
        return Err("invalid SigV4 credential scope".into());
    }
    if credentials
        .expiration
        .is_some_and(|expiration| expiration <= chrono::Utc::now())
    {
        return Err("AWS credentials are expired".into());
    }
    let date = request
        .headers()
        .get("x-amz-date")
        .map(|value| value.to_str().map(str::to_owned))
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string());
    if date.len() != 16
        || date.as_bytes()[8] != b'T'
        || date.as_bytes()[15] != b'Z'
        || !date
            .bytes()
            .enumerate()
            .all(|(i, c)| matches!(i, 8 | 15) || c.is_ascii_digit())
    {
        return Err("invalid x-amz-date".into());
    }
    request.headers_mut().insert(
        "x-amz-date",
        date.parse().map_err(|_| "invalid signing date header")?,
    );
    if let Some(token) = &credentials.session_token {
        request.headers_mut().insert(
            "x-amz-security-token",
            token.parse().map_err(|_| "invalid session token header")?,
        );
    }
    let payload = if let Some(payload) = request.headers().get("x-amz-content-sha256") {
        payload.to_str().map_err(|e| e.to_string())?.to_owned()
    } else if scope.unsigned_payload {
        "UNSIGNED-PAYLOAD".into()
    } else {
        let bytes = match request.body() {
            None => &[][..],
            Some(body) => body
                .as_bytes()
                .ok_or("SigV4 streaming payload requires a precomputed ByteStream sha256")?,
        };
        hash(bytes)
    };
    request.headers_mut().insert(
        "x-amz-content-sha256",
        payload
            .parse()
            .map_err(|_| "invalid content digest header")?,
    );
    let mut path = request.url().path().to_owned();
    if scope.normalize_path {
        while path.contains("//") {
            path = path.replace("//", "/");
        }
    }
    if scope.double_encode {
        path = path.split('/').map(encode).collect::<Vec<_>>().join("/");
    }
    let mut query = request
        .url()
        .query_pairs()
        .map(|(name, value)| (encode(&name), encode(&value)))
        .collect::<Vec<_>>();
    query.sort();
    let query = query
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut headers = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (name, value) in request.headers() {
        if matches!(
            name.as_str(),
            "authorization"
                | "connection"
                | "expect"
                | "transfer-encoding"
                | "user-agent"
                | "x-amzn-trace-id"
                | "accept"
                | "accept-encoding"
                | "content-length"
                | "host"
        ) {
            continue;
        }
        headers.entry(name.to_string()).or_default().push(
            value
                .to_str()
                .map_err(|e| e.to_string())?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    let mut authority = request
        .url()
        .host_str()
        .ok_or("missing signing host")?
        .to_owned();
    if let Some(port) = request.url().port() {
        authority.push_str(&format!(":{port}"));
    }
    headers.insert("host".into(), vec![authority]);
    let names = headers.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical_headers = headers
        .into_iter()
        .map(|(name, values)| format!("{name}:{}\n", values.join(",")))
        .collect::<String>();
    let canonical = format!(
        "{}\n{path}\n{query}\n{canonical_headers}\n{names}\n{payload}",
        request.method()
    );
    let credential_scope = format!(
        "{}/{}/{}/aws4_request",
        &date[..8],
        scope.region,
        scope.name
    );
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{credential_scope}\n{}",
        hash(canonical.as_bytes())
    );
    let key = signing_key(
        &credentials.secret_access_key,
        &date[..8],
        scope.region,
        scope.name,
    );
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={names}, Signature={}",
        credentials.access_key_id,
        hex(&hmac(&key, to_sign.as_bytes()))
    );
    request.headers_mut().insert(
        http::header::AUTHORIZATION,
        authorization
            .parse()
            .map_err(|_| "invalid signature header")?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_event_stream_string_to_sign_and_chained_signature() {
        use crucible_wire::event_stream::{HeaderValue, Message};
        // tests/conformance/event-streams.json: smithy-rs aws-sigv4
        // event_stream::tests::string_to_sign.
        let vectors: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/conformance/event-streams.json"
        ))
        .unwrap();
        let vector = &vectors["signing"];
        let message = Message::new(&b"test payload"[..])
            .with_header("some-header", HeaderValue::String("value".into()))
            .encode()
            .unwrap();
        let mut signer = EventSigner::new(
            vector["secretAccessKey"].as_str().unwrap(),
            "us-east-1",
            "testservice",
            vector["priorSignature"].as_str().unwrap().to_owned(),
        );
        let time = chrono::DateTime::from_timestamp(123_456_789, 1234).unwrap();
        let whole = chrono::DateTime::from_timestamp(123_456_789, 0).unwrap();
        assert_eq!(
            signer.string_to_sign(&message, whole),
            vector["stringToSign"].as_str().unwrap()
        );
        let signed = signer.sign(message, time);
        let expected = vector["signature"].as_str().unwrap();
        assert_eq!(signer.prior, expected);
        assert_eq!(
            signed.header(":chunk-signature"),
            Some(&HeaderValue::Bytes(
                (0..32)
                    .map(|i| u8::from_str_radix(&expected[i * 2..i * 2 + 2], 16).unwrap())
                    .collect::<Vec<_>>()
                    .into()
            ))
        );
        assert_eq!(
            signed.header(":date"),
            Some(&HeaderValue::Timestamp(123_456_789_000))
        );
        // The next message chains from this signature.
        assert!(signer.string_to_sign(b"", whole).contains(expected));
    }
    #[test]
    fn published_s3_get_signature() {
        // https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html
        let mut request = reqwest::Request::new(
            http::Method::GET,
            "https://examplebucket.s3.amazonaws.com/test.txt"
                .parse()
                .unwrap(),
        );
        request
            .headers_mut()
            .insert("range", "bytes=0-9".parse().unwrap());
        request
            .headers_mut()
            .insert("x-amz-date", "20130524T000000Z".parse().unwrap());
        sign(
            &mut request,
            &AwsCredentials::new(
                "AKIAIOSFODNN7EXAMPLE",
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            ),
            Scope {
                region: "us-east-1",
                name: "s3",
                double_encode: false,
                normalize_path: false,
                unsigned_payload: false,
            },
        )
        .unwrap();
        assert!(
            request.headers()["authorization"]
                .to_str()
                .unwrap()
                .ends_with(
                    "Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
                )
        );
    }
}
