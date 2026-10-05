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
    let key = hmac(
        format!("AWS4{}", credentials.secret_access_key).as_bytes(),
        &date.as_bytes()[..8],
    );
    let key = hmac(&key, scope.region.as_bytes());
    let key = hmac(&key, scope.name.as_bytes());
    let key = hmac(&key, b"aws4_request");
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
