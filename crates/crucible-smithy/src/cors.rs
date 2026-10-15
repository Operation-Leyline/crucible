//! CORS is service-wide wire metadata, resolved before any emitter runs.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::ir::{ApiKeyLocation, AuthScheme, Checksum, ChecksumAlgorithm, Operation, Shape};
use crate::{Diagnostics, ShapeId};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cors {
    pub origin: String,
    pub max_age: i32,
    pub allow_credentials: bool,
    pub allowed_headers: Vec<String>,
    /// Prefix bindings describe open-ended names, not literal CORS wildcards.
    pub allowed_header_prefixes: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub methods: Vec<String>,
}

pub(crate) fn lower(
    id: &ShapeId,
    value: &Value,
    operations: &[Operation],
    shapes: &BTreeMap<ShapeId, Shape>,
    auth: &BTreeMap<ShapeId, AuthScheme>,
    diagnostics: &mut Diagnostics,
) -> Option<Cors> {
    let Some(object) = value.as_object() else {
        diagnostics.push(id, "@cors must be an object");
        return None;
    };
    for key in object.keys() {
        if key == "origins" {
            diagnostics.push(id, "@cors origins requires deployment origin selection, which is not supported; use origin");
        } else if !matches!(
            key.as_str(),
            "origin" | "maxAge" | "additionalAllowedHeaders" | "additionalExposedHeaders"
        ) {
            diagnostics.push(id, format!("@cors has unknown property {key}"));
        }
    }
    let origin = match object.get("origin") {
        None => "*".to_owned(),
        Some(Value::String(origin))
            if !origin.is_empty()
                && origin.bytes().all(|b| (32..127).contains(&b) && b != b',') =>
        {
            origin.clone()
        }
        Some(_) => {
            diagnostics.push(
                id,
                "@cors origin must be a nonempty ASCII HTTP header value without commas",
            );
            "*".to_owned()
        }
    };
    let max_age = match object.get("maxAge") {
        None => 600,
        Some(value) => match value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value >= -1)
        {
            Some(value) => value,
            None => {
                diagnostics.push(
                    id,
                    "@cors maxAge must be an integer from -1 through 2147483647",
                );
                600
            }
        },
    };
    let mut allowed = BTreeSet::from(["content-type".to_owned()]);
    let mut exposed = BTreeSet::from([
        "content-type".to_owned(),
        "x-crucible-error".to_owned(),
        "x-request-id".to_owned(),
    ]);
    let mut prefixes = BTreeSet::new();
    let mut methods = BTreeSet::new();
    for operation in operations {
        methods.insert(operation.method.clone());
        allowed.extend(
            operation
                .request
                .headers
                .iter()
                .map(|b| b.name.to_ascii_lowercase()),
        );
        prefixes.extend(
            operation
                .request
                .prefix_headers
                .iter()
                .map(|b| b.name.to_ascii_lowercase()),
        );
        exposed.extend(
            operation
                .response
                .headers
                .iter()
                .map(|b| b.name.to_ascii_lowercase()),
        );
        match &operation.checksum {
            Some(Checksum::Md5) => {
                allowed.insert("content-md5".to_owned());
            }
            Some(Checksum::AwsFlexible {
                request_required,
                request_algorithm_member,
                request_algorithms,
                response_algorithms,
                ..
            }) => {
                allowed.extend(
                    request_algorithms
                        .iter()
                        .map(|algorithm| checksum_header(*algorithm).to_owned()),
                );
                exposed.extend(
                    response_algorithms
                        .iter()
                        .map(|algorithm| checksum_header(*algorithm).to_owned()),
                );
                if *request_required && request_algorithm_member.is_none() {
                    allowed.insert("content-md5".to_owned());
                }
            }
            None => {}
        }
    }
    for shape in shapes.values() {
        if let Shape::Structure(structure) = shape
            && let Some(error) = &structure.error
        {
            exposed.extend(error.headers.iter().map(|b| b.name.to_ascii_lowercase()));
        }
    }
    let mut allow_credentials = false;
    for scheme in auth.values() {
        match scheme {
            AuthScheme::Basic | AuthScheme::Digest => {
                allow_credentials = true;
                allowed.insert("authorization".to_owned());
                exposed.insert("www-authenticate".to_owned());
            }
            AuthScheme::Bearer => {
                allowed.insert("authorization".to_owned());
                exposed.insert("www-authenticate".to_owned());
            }
            AuthScheme::ApiKey {
                name,
                location: ApiKeyLocation::Header,
                ..
            } => {
                allowed.insert(name.to_ascii_lowercase());
            }
            AuthScheme::ApiKey { .. } => {}
            AuthScheme::SigV4 { .. } | AuthScheme::SigV4a { .. } => {
                allowed.extend(
                    [
                        "authorization",
                        "x-amz-date",
                        "x-amz-security-token",
                        "x-amz-content-sha256",
                    ]
                    .map(str::to_owned),
                );
                if matches!(scheme, AuthScheme::SigV4a { .. }) {
                    allowed.insert("x-amz-region-set".to_owned());
                }
            }
        }
    }
    if allow_credentials && origin == "*" {
        diagnostics.push(
            id,
            "@cors with browser-managed basic/digest credentials requires a single explicit origin",
        );
    }
    for (property, headers) in [
        ("additionalAllowedHeaders", &mut allowed),
        ("additionalExposedHeaders", &mut exposed),
    ] {
        if let Some(value) = object.get(property) {
            match value.as_array() {
                Some(values) => {
                    for value in values {
                        match value.as_str().filter(|value| header_name(value)) {
                            Some(name) => {
                                headers.insert(name.to_ascii_lowercase());
                            }
                            None => diagnostics.push(
                                id,
                                format!("@cors {property} entries must be HTTP header names"),
                            ),
                        }
                    }
                }
                None => diagnostics.push(
                    id,
                    format!("@cors {property} must be a list of HTTP header names"),
                ),
            }
        }
    }
    Some(Cors {
        origin,
        max_age,
        allow_credentials,
        allowed_headers: allowed.into_iter().collect(),
        allowed_header_prefixes: prefixes.into_iter().collect(),
        exposed_headers: exposed.into_iter().collect(),
        methods: methods.into_iter().collect(),
    })
}

fn header_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn checksum_header(algorithm: ChecksumAlgorithm) -> &'static str {
    match algorithm {
        ChecksumAlgorithm::Md5 => "x-amz-checksum-md5",
        ChecksumAlgorithm::Crc32 => "x-amz-checksum-crc32",
        ChecksumAlgorithm::Crc32c => "x-amz-checksum-crc32c",
        ChecksumAlgorithm::Crc64nvme => "x-amz-checksum-crc64nvme",
        ChecksumAlgorithm::Sha1 => "x-amz-checksum-sha1",
        ChecksumAlgorithm::Sha256 => "x-amz-checksum-sha256",
        ChecksumAlgorithm::Sha512 => "x-amz-checksum-sha512",
        ChecksumAlgorithm::Xxhash64 => "x-amz-checksum-xxhash64",
        ChecksumAlgorithm::Xxhash3 => "x-amz-checksum-xxhash3",
        ChecksumAlgorithm::Xxhash128 => "x-amz-checksum-xxhash128",
    }
}
