//! Compile-time construction of descriptions owned by generated bindings.

use crate::operation::{
    Classify, CorsInfo, ErrorInfo, Fault, InternalFn, Location, MemberInfo, OperationInfo,
    Redaction, RejectFn, Scalar, Segment, ServiceInfo,
};

#[allow(clippy::too_many_arguments)]
pub const fn operation_info(
    id: &'static str,
    name: &'static str,
    resource: Option<&'static str>,
    method: &'static str,
    path: &'static [Segment],
    literal_query: &'static [(&'static str, Option<&'static str>)],
    status: u16,
    input: &'static [MemberInfo],
    output: &'static [MemberInfo],
    errors: &'static [&'static str],
    readonly: bool,
    idempotent: bool,
    internal: bool,
    idempotency_token: Option<&'static str>,
    traits: &'static [(&'static str, &'static str)],
) -> OperationInfo {
    OperationInfo {
        id,
        name,
        resource,
        method,
        path,
        literal_query,
        status,
        input,
        output,
        errors,
        readonly,
        idempotent,
        internal,
        idempotency_token,
        traits,
    }
}

#[allow(clippy::too_many_arguments)]
pub const fn service_info(
    id: &'static str,
    version: &'static str,
    cors: Option<&'static CorsInfo>,
    mcp_tools: Option<&'static [crate::mcp::ToolDescriptor]>,
    operations: &'static [&'static OperationInfo],
    errors: &'static [&'static ErrorInfo],
    classify: Classify,
    reject: Option<RejectFn>,
    internal: Option<InternalFn>,
    unknown: Option<InternalFn>,
) -> ServiceInfo {
    ServiceInfo {
        id,
        version,
        cors,
        mcp_tools,
        operations,
        errors,
        classify,
        reject,
        internal,
        unknown,
    }
}

pub const fn member_info(
    name: &'static str,
    location: Location,
    scalar: Scalar,
    redaction: &'static Redaction,
    traits: &'static [(&'static str, &'static str)],
) -> MemberInfo {
    MemberInfo {
        name,
        location,
        scalar,
        redaction,
        traits,
    }
}

pub const fn error_info(
    name: &'static str,
    status: u16,
    fault: Fault,
    retryable: bool,
    throttling: bool,
    members: &'static [MemberInfo],
) -> ErrorInfo {
    ErrorInfo {
        name,
        status,
        fault,
        retryable,
        throttling,
        members,
    }
}
