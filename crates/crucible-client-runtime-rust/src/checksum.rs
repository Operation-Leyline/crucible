//! HTTP checksums: `Content-MD5` for `@httpChecksumRequired`, and the
//! flexible `x-amz-checksum-*` headers of `@aws.protocols#httpChecksum`,
//! computed over the bytes actually sent and verified over the bytes received.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crucible_wire::operation::{Checksum, ChecksumAlgorithm, Location, MemberInfo};
use serde_json::Value;
use sha1::Digest as _;

/// When a client computes request checksums or validates response checksums.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChecksumPolicy {
    /// Whenever the operation supports one: the default.
    #[default]
    WhenSupported,
    /// Only when the operation requires one, or the caller asked for one.
    WhenRequired,
}

/// Whether this runtime computes `algorithm`.
pub fn supported(algorithm: ChecksumAlgorithm) -> bool {
    matches!(
        algorithm,
        ChecksumAlgorithm::Crc32
            | ChecksumAlgorithm::Crc32c
            | ChecksumAlgorithm::Crc64Nvme
            | ChecksumAlgorithm::Md5
            | ChecksumAlgorithm::Sha1
            | ChecksumAlgorithm::Sha256
            | ChecksumAlgorithm::Sha512
    )
}

/// The base64 checksum of `data`, or `None` for an algorithm this runtime
/// does not compute.
pub fn digest(algorithm: ChecksumAlgorithm, data: &[u8]) -> Option<String> {
    let mut hasher = Hasher::new(algorithm)?;
    hasher.update(data);
    Some(hasher.finish())
}

static CRC32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
static CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
static CRC64NVME: crc::Crc<u64> = crc::Crc::<u64>::new(&crc::CRC_64_NVME);

/// An incremental checksum, so a streamed body is checked as it passes.
pub(crate) enum Hasher {
    Crc32(crc::Digest<'static, u32>),
    Crc64(crc::Digest<'static, u64>),
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
}

impl Hasher {
    pub(crate) fn new(algorithm: ChecksumAlgorithm) -> Option<Self> {
        Some(match algorithm {
            ChecksumAlgorithm::Crc32 => Self::Crc32(CRC32.digest()),
            ChecksumAlgorithm::Crc32c => Self::Crc32(CRC32C.digest()),
            ChecksumAlgorithm::Crc64Nvme => Self::Crc64(CRC64NVME.digest()),
            ChecksumAlgorithm::Md5 => Self::Md5(md5::Md5::new()),
            ChecksumAlgorithm::Sha1 => Self::Sha1(sha1::Sha1::new()),
            ChecksumAlgorithm::Sha256 => Self::Sha256(sha2::Sha256::new()),
            ChecksumAlgorithm::Sha512 => Self::Sha512(sha2::Sha512::new()),
            _ => return None,
        })
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        match self {
            Self::Crc32(digest) => digest.update(data),
            Self::Crc64(digest) => digest.update(data),
            Self::Md5(digest) => digest.update(data),
            Self::Sha1(digest) => digest.update(data),
            Self::Sha256(digest) => digest.update(data),
            Self::Sha512(digest) => digest.update(data),
        }
    }

    pub(crate) fn finish(self) -> String {
        match self {
            Self::Crc32(digest) => STANDARD.encode(digest.finalize().to_be_bytes()),
            Self::Crc64(digest) => STANDARD.encode(digest.finalize().to_be_bytes()),
            Self::Md5(digest) => STANDARD.encode(digest.finalize()),
            Self::Sha1(digest) => STANDARD.encode(digest.finalize()),
            Self::Sha256(digest) => STANDARD.encode(digest.finalize()),
            Self::Sha512(digest) => STANDARD.encode(digest.finalize()),
        }
    }
}

/// Compares without an early exit, as the expected value comes from the wire.
pub(crate) fn same(expected: &[u8], actual: &[u8]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// What one call computes and verifies, resolved once from the model, the
/// input and the client's policies.
#[derive(Debug, Clone, Default)]
pub(crate) struct Plan {
    /// The algorithm and the header it travels in.
    pub request: Option<(ChecksumAlgorithm, &'static str)>,
    /// A streamed body cannot be hashed up front: fail rather than omit a
    /// checksum the model or the caller requires.
    pub required: bool,
    /// The response checksums to verify, in modeled order; the first one
    /// the response carries is checked.
    pub response: Vec<ChecksumAlgorithm>,
}

pub(crate) struct Policies {
    pub request: ChecksumPolicy,
    pub response: ChecksumPolicy,
    pub streaming: bool,
}

/// Resolves the plan for one call, adding the algorithm and validation-mode
/// members the client fills on the caller's behalf to `bound` (header
/// members) or `body` (JSON body members). The caller's input is unchanged.
pub(crate) fn plan(
    checksum: Option<Checksum>,
    members: &'static [MemberInfo],
    bound: &mut Vec<(&'static MemberInfo, Value)>,
    body: &mut Vec<u8>,
    policies: Policies,
) -> Result<Plan, String> {
    let Some(checksum) = checksum else {
        return Ok(Plan::default());
    };
    let Checksum::Flexible {
        request_required,
        request_algorithm_member,
        validation_mode_member,
        response_algorithms,
    } = checksum
    else {
        return Ok(Plan {
            request: Some((ChecksumAlgorithm::Md5, "content-md5")),
            required: true,
            response: Vec::new(),
        });
    };
    let mut plan = Plan::default();
    if let Some(name) = request_algorithm_member {
        let algorithm = match member_value(members, bound, body, name)? {
            Some(chosen) => Some((chosen, true)),
            // A streamed body cannot be hashed before it is sent; only a
            // required checksum is attempted, and then fails clearly.
            None if request_required
                || (policies.request == ChecksumPolicy::WhenSupported && !policies.streaming) =>
            {
                set_member(members, bound, body, name, "CRC32")?;
                Some(("CRC32".to_owned(), request_required))
            }
            None => None,
        };
        if let Some((name, required)) = algorithm {
            let algorithm = ChecksumAlgorithm::from_name(&name)
                .filter(|algorithm| supported(*algorithm))
                .ok_or_else(|| format!("unsupported checksum algorithm: {name}"))?;
            plan.request = Some((algorithm, algorithm.header()));
            plan.required = required;
        }
    } else if request_required {
        plan.request = Some((ChecksumAlgorithm::Md5, "content-md5"));
        plan.required = true;
    }
    if let Some(name) = validation_mode_member {
        let mut mode = member_value(members, bound, body, name)?;
        if mode.is_none() && policies.response == ChecksumPolicy::WhenSupported {
            set_member(members, bound, body, name, "ENABLED")?;
            mode = Some("ENABLED".to_owned());
        }
        if mode.as_deref() == Some("ENABLED") {
            plan.response = response_algorithms
                .iter()
                .copied()
                .filter(|algorithm| supported(*algorithm))
                .collect();
        }
    }
    Ok(plan)
}

fn member(members: &'static [MemberInfo], name: &str) -> Result<&'static MemberInfo, String> {
    members
        .iter()
        .find(|member| member.name == name)
        .ok_or_else(|| format!("checksum member `{name}` is not an input member"))
}

fn member_value(
    members: &'static [MemberInfo],
    bound: &[(&'static MemberInfo, Value)],
    body: &[u8],
    name: &str,
) -> Result<Option<String>, String> {
    let member = member(members, name)?;
    let value = if member.location == Location::Body {
        if body.is_empty() {
            None
        } else {
            let object: Value = serde_json::from_slice(body).map_err(|e| e.to_string())?;
            object.get(name).cloned()
        }
    } else {
        bound
            .iter()
            .find(|(bound, _)| bound.name == name)
            .map(|(_, value)| value.clone())
    };
    Ok(value.and_then(|value| value.as_str().map(str::to_owned)))
}

fn set_member(
    members: &'static [MemberInfo],
    bound: &mut Vec<(&'static MemberInfo, Value)>,
    body: &mut Vec<u8>,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let member = member(members, name)?;
    match member.location {
        Location::Body => {
            let mut object: serde_json::Map<String, Value> = if body.is_empty() {
                serde_json::Map::new()
            } else {
                serde_json::from_slice(body).map_err(|e| e.to_string())?
            };
            object.insert(name.to_owned(), Value::String(value.to_owned()));
            *body = serde_json::to_vec(&object).map_err(|e| e.to_string())?;
        }
        Location::Header(_) | Location::Query(_) => {
            bound.push((member, Value::String(value.to_owned())));
        }
        _ => return Err(format!("checksum member `{name}` cannot be filled")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Vectors {
        input_utf8: String,
        base64: std::collections::BTreeMap<String, String>,
        empty_md5: String,
    }

    #[test]
    fn every_supported_algorithm_matches_the_shared_vectors() {
        let vectors: Vectors =
            serde_json::from_str(include_str!("../../../tests/conformance/checksums.json"))
                .unwrap();
        assert_eq!(vectors.base64.len(), 7);
        for (name, expected) in &vectors.base64 {
            let algorithm = ChecksumAlgorithm::from_name(name).unwrap();
            assert!(supported(algorithm));
            assert_eq!(
                digest(algorithm, vectors.input_utf8.as_bytes()).as_deref(),
                Some(expected.as_str()),
                "{name}"
            );
            // Incremental hashing agrees with one pass.
            let mut hasher = Hasher::new(algorithm).unwrap();
            for byte in vectors.input_utf8.as_bytes() {
                hasher.update(std::slice::from_ref(byte));
            }
            assert_eq!(&hasher.finish(), expected, "{name}");
        }
        assert_eq!(
            digest(ChecksumAlgorithm::Md5, b"").as_deref(),
            Some(vectors.empty_md5.as_str())
        );
        for algorithm in [
            ChecksumAlgorithm::Xxhash64,
            ChecksumAlgorithm::Xxhash3,
            ChecksumAlgorithm::Xxhash128,
        ] {
            assert!(!supported(algorithm));
            assert_eq!(digest(algorithm, b"x"), None);
        }
    }

    use crucible_wire::operation::{Redaction, Scalar};

    const fn member(name: &'static str, location: Location) -> MemberInfo {
        crate::__generated::member_info(name, location, Scalar::String, &Redaction::Public, &[])
    }
    static RPC: [MemberInfo; 2] = [
        member("algorithm", Location::Body),
        member("mode", Location::Body),
    ];
    static REST: [MemberInfo; 2] = [
        member("algorithm", Location::Header("x-algorithm")),
        member("mode", Location::Header("x-mode")),
    ];
    const FLEXIBLE: Checksum = Checksum::Flexible {
        request_required: false,
        request_algorithm_member: Some("algorithm"),
        validation_mode_member: Some("mode"),
        response_algorithms: &[ChecksumAlgorithm::Xxhash3, ChecksumAlgorithm::Sha1],
    };
    fn policies(policy: ChecksumPolicy, streaming: bool) -> Policies {
        Policies {
            request: policy,
            response: policy,
            streaming,
        }
    }

    #[test]
    fn defaults_fill_body_members_of_rpc_inputs() {
        let mut body = br#"{"other":1}"#.to_vec();
        let mut bound = Vec::new();
        let plan = plan(
            Some(FLEXIBLE),
            &RPC,
            &mut bound,
            &mut body,
            policies(ChecksumPolicy::WhenSupported, false),
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            body,
            serde_json::json!({"other":1,"algorithm":"CRC32","mode":"ENABLED"})
        );
        assert!(bound.is_empty());
        assert_eq!(
            plan.request,
            Some((ChecksumAlgorithm::Crc32, "x-amz-checksum-crc32"))
        );
        assert!(!plan.required);
        assert_eq!(plan.response, [ChecksumAlgorithm::Sha1]);
    }

    #[test]
    fn policies_and_streams_leave_optional_checksums_to_the_caller() {
        for (policy, streaming) in [
            (ChecksumPolicy::WhenRequired, false),
            (ChecksumPolicy::WhenSupported, true),
        ] {
            let mut bound = Vec::new();
            let plan = plan(
                Some(FLEXIBLE),
                &REST,
                &mut bound,
                &mut Vec::new(),
                policies(policy, streaming),
            )
            .unwrap();
            assert_eq!(plan.request, None);
            assert_eq!(
                bound.len(),
                usize::from(policy == ChecksumPolicy::WhenSupported)
            );
        }
        let mut bound = vec![(&REST[0], Value::String("SHA256".into()))];
        let plan = plan(
            Some(FLEXIBLE),
            &REST,
            &mut bound,
            &mut Vec::new(),
            policies(ChecksumPolicy::WhenRequired, true),
        )
        .unwrap();
        assert_eq!(
            plan.request,
            Some((ChecksumAlgorithm::Sha256, "x-amz-checksum-sha256"))
        );
        assert!(plan.required, "a chosen algorithm is required");
        assert!(plan.response.is_empty());
    }

    #[test]
    fn comparison_is_exact() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"ab"));
    }
}
