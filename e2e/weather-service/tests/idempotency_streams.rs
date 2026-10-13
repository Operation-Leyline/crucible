//! `Idempotency` refuses, at build, an operation whose stream it can neither
//! fingerprint (a streaming input) nor replay (a streaming output), naming
//! it, and builds once the service leaves it out.
// The fixture deprecates `Ping`; its description is the base here.
#![allow(deprecated)]
use crucible_server_runtime_rust::__generated as generated;
use std::sync::Arc;

use crucible_server_runtime_rust::idempotency::{Idempotency, MemoryIdempotencyStore};
use crucible_server_runtime_rust::operation::{
    Location, MemberInfo, OutputFromJson, PaginationMember, Redaction, Scalar, Segment, Unit,
};
use crucible_server_runtime_rust::validate::{Validate, Validator};
use crucible_server_runtime_rust::{
    BuildError, ByteStream, Crucible, InterceptorExt, JsonProtocol, Operation, OperationInfo,
    ServiceInfo,
};
use serde::{Deserialize, Serialize};
use weather_service::weather;

const BODY: MemberInfo = generated::member_info(
    "body",
    Location::StreamingPayload {
        media_type: "application/octet-stream",
        requires_length: false,
        minimum: None,
        maximum: None,
    },
    Scalar::Json,
    &Redaction::Public,
    &[],
);
const TOKEN_MEMBER: MemberInfo = generated::member_info(
    "token",
    Location::Header("x-token"),
    Scalar::String,
    &Redaction::Public,
    &[],
);
const TOKEN: PaginationMember = PaginationMember {
    name: "token",
    wire_name: "token",
};

const fn info(
    id: &'static str,
    name: &'static str,
    path: &'static [Segment],
    input: &'static [MemberInfo],
    output: &'static [MemberInfo],
) -> OperationInfo {
    let base = weather::Ping::INFO;
    generated::operation_info(
        id,
        name,
        base.resource,
        "POST",
        path,
        base.literal_query,
        base.status,
        input,
        output,
        base.errors,
        false,
        false,
        base.internal,
        Some("token"),
        base.traits,
    )
}

static UPLOAD: OperationInfo = info(
    "test.idempotency#Upload",
    "Upload",
    &[Segment::Literal("upload")],
    &[BODY, TOKEN_MEMBER],
    &[],
);
static DOWNLOAD: OperationInfo = info(
    "test.idempotency#Download",
    "Download",
    &[Segment::Literal("download")],
    &[TOKEN_MEMBER],
    &[BODY],
);
static SERVICE: ServiceInfo = {
    let base = &(weather::SERVICE);
    generated::service_info(
        "test.idempotency#Service",
        base.version,
        None,
        None,
        &[&UPLOAD, &DOWNLOAD],
        base.errors,
        base.classify,
        base.reject,
        base.internal,
        base.unknown,
    )
};

#[derive(Serialize, Deserialize)]
struct UploadInput {
    body: ByteStream,
    token: Option<String>,
}
impl Validate for UploadInput {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize, Deserialize)]
struct DownloadInput {
    token: Option<String>,
}
impl Validate for DownloadInput {
    fn validate(&self, _: &mut Validator) {}
}
#[derive(Serialize)]
struct DownloadOutput {
    body: ByteStream,
}

/// A token and a streaming input: the stream is not in the fingerprint.
struct Upload;
impl Operation for Upload {
    const INFO: &'static OperationInfo = &UPLOAD;
    const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
    const OUTPUT_FROM_JSON: Option<OutputFromJson<Unit>> = Some(serde_json::from_value::<Unit>);
    type Input = UploadInput;
    type Output = Unit;
    fn input_stream(input: &UploadInput) -> Option<ByteStream> {
        Some(input.body.clone())
    }
    fn set_input_stream(input: &mut UploadInput, body: ByteStream) {
        input.body = body;
    }
}

/// A token and a streaming output, which cannot be replayed.
struct Download;
impl Operation for Download {
    const INFO: &'static OperationInfo = &DOWNLOAD;
    const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
    type Input = DownloadInput;
    type Output = DownloadOutput;
    fn output_stream(output: &DownloadOutput) -> Option<ByteStream> {
        Some(output.body.clone())
    }
}

fn builder() -> crucible_server_runtime_rust::Builder {
    let operations = crucible_server_runtime_rust::Operations::builder(&SERVICE)
        .add::<Upload, _, _>(Arc::new(()), |_, _| Box::pin(async { Ok(Unit {}) }))
        .add::<Download, _, _>(Arc::new(()), |_, _| {
            Box::pin(async {
                Ok(DownloadOutput {
                    body: ByteStream::from_bytes(&b"bytes"[..]),
                })
            })
        })
        .build()
        .unwrap();
    Crucible::builder(operations).protocol(JsonProtocol)
}

fn idempotency() -> Idempotency {
    Idempotency::new(MemoryIdempotencyStore::new(), |_| None)
}

#[test]
fn operations_whose_streams_cannot_be_fingerprinted_or_replayed_are_refused_by_name() {
    let refused = builder().interceptor(idempotency()).build();
    let Err(BuildError::Prepare(message)) = refused else {
        panic!("Idempotency over streams must not build");
    };
    assert!(
        message.contains("cannot fingerprint the streaming input of Upload"),
        "{message}"
    );
    assert!(
        message.contains("cannot replay the output of Download"),
        "{message}"
    );
    assert!(message.contains(".except("), "{message}");

    for (left_out, still_refused) in [("Upload", "Download"), ("Download", "Upload")] {
        let Err(BuildError::Prepare(message)) = builder()
            .interceptor(idempotency().except(&[left_out]))
            .build()
        else {
            panic!("{still_refused} is still covered");
        };
        assert!(message.contains(still_refused), "{message}");
        assert!(!message.contains(left_out), "{message}");
    }

    assert!(
        builder()
            .interceptor(idempotency().except(&["Upload", "Download"]))
            .build()
            .is_ok()
    );
}
