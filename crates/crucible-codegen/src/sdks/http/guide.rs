//! The guide for a caller without an SDK (`http.guide` of
//! `crucible.sdks/1`): what an SDK's guide says for its language, said for
//! the wire. Its sections have the SDK guides' shape, so a documentation
//! site shows it with the page that shows theirs.
//!
//! Every fact is the model's or the protocol's: the headers are the ones
//! the `curl` commands send, the first call is the operation the SDK
//! guides call, its response is the model's example (else the generated
//! sample), and the type table is the JSON each simple type the service
//! uses is written as.

use std::collections::BTreeSet;

use crucible_codegen_common::describe::Description;
use crucible_codegen_common::docs::IDEMPOTENCY_TOKEN_NOTE;
use crucible_codegen_common::guide::{Section, code_span, used_kinds};
use crucible_codegen_common::snippet::{paginated, showcase};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Operation, Service, Shape, SimpleKind, TimestampFormat};
use serde_json::{Value, json};

use super::CONTENT_SHA256;

/// What the guide is written from.
pub(super) struct Facts<'a> {
    /// The service as every client sees it.
    pub service: &'a Service,
    pub description: &'a Description,
    /// `http.operations`, by absolute ID.
    pub operations: &'a serde_json::Map<String, Value>,
    /// Each default middleware's `(name, header)`.
    pub middleware: &'a [(String, String)],
    pub endpoint_env: &'a str,
    pub token_env: &'a str,
    /// The endpoint the described SDKs call, where they name one.
    pub endpoint: Option<&'a str>,
}

/// `{language, fence, sections}`: an SDK guide without a package.
pub(super) fn guide(facts: &Facts) -> Value {
    let sections: Vec<Section> = [
        Some(request(facts)),
        call(facts),
        types(facts.service),
        pagination(facts.service),
        errors(facts),
        Some(retries(facts.service)),
    ]
    .into_iter()
    .flatten()
    .map(|(id, title, markdown)| Section::new(id, title, markdown))
    .collect();
    json!({"language": "HTTP", "fence": "sh", "sections": sections})
}

type Draft = (&'static str, &'static str, String);

fn fenced(fence: &str, text: &str) -> String {
    // A fence longer than any run of backticks in the text, so model text
    // cannot close it.
    let run = text
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or_default();
    let ticks = "`".repeat(run.max(2) + 1);
    format!("{ticks}{fence}\n{}\n{ticks}\n", text.trim_end_matches('\n'))
}

fn bearer(service: &Service, operation: &Operation) -> bool {
    !operation.public && operation.auth.iter().any(|s| service.is_bearer(s))
}

fn has_payload(service: &Service) -> bool {
    service
        .operations
        .iter()
        .any(|op| op.request.payload.is_some() || op.response.payload.is_some())
}

/// The endpoint, the credential and the headers every request sends.
fn request(facts: &Facts) -> Draft {
    let service = facts.service;
    let mut out = format!(
        "Every operation is one HTTP request to the API's endpoint. A body, in a request or a \
         response, is JSON{}. The commands here read the endpoint from {}",
        if has_payload(service) {
            ", except where an operation's page names another media type"
        } else {
            ""
        },
        code_span(facts.endpoint_env),
    );
    let with_bearer = service.operations.iter().any(|op| bearer(service, op));
    let mut exports = format!(
        "export {}='{}'\n",
        facts.endpoint_env,
        facts.endpoint.unwrap_or("https://<endpoint>")
    );
    if with_bearer {
        out.push_str(&format!(
            " and the token from {}, so that none holds a credential",
            code_span(facts.token_env)
        ));
        exports.push_str(&format!("export {}='<token>'\n", facts.token_env));
    }
    out.push_str(":\n\n");
    out.push_str(&fenced("sh", &exports));

    out.push_str("\nSend these headers:\n\n| Header | Value | On |\n|---|---|---|\n");
    if with_bearer {
        let public = service
            .operations
            .iter()
            .filter(|op| !bearer(service, op))
            .count();
        let on = match public {
            0 => "every request".to_owned(),
            1 => "every request but the one operation that takes no bearer token".to_owned(),
            n => format!("every request but the {n} operations that take no bearer token"),
        };
        out.push_str(&format!(
            "| `Authorization` | `Bearer`, a space, then the token | {on} |\n"
        ));
    }
    out.push_str("| `Content-Type` | `application/json` | a request with a JSON body |\n");
    if let Some(version) = &service.api_version {
        out.push_str(&format!(
            "| `Crucible-Version` | {} | every request |\n",
            code_span(version)
        ));
    }
    let mut seen = BTreeSet::new();
    for (name, header) in facts.middleware {
        if !seen.insert(header) {
            continue;
        }
        if name == CONTENT_SHA256 {
            out.push_str(&format!(
                "| {} | the body's SHA-256, in lowercase hex | {} |\n",
                code_span(header),
                Middleware::content_sha256()
                    .sent_on
                    .as_deref()
                    .unwrap_or("every request"),
            ));
        } else {
            out.push_str(&format!(
                "| {} | what the SDKs' {} middleware sends | every request |\n",
                code_span(header),
                code_span(name),
            ));
        }
    }
    if service.api_version.is_some() {
        out.push_str(
            "\n`Crucible-Version` names the release this reference documents. A request without \
             it is served at a release the service chooses, which may be an older one.",
        );
    }
    if service
        .operations
        .iter()
        .any(|op| !op.public && op.auth.iter().any(|s| !service.is_bearer(s)))
    {
        out.push_str(
            " An operation that takes another credential than a bearer token says so on its page.",
        );
    }
    ("request", "Send a request", out)
}

/// The operation the SDK guides call, as a command, and what it answers.
fn call(facts: &Facts) -> Option<Draft> {
    let operation = showcase(facts.service)?;
    let id = operation.id.to_string();
    let request = facts.operations.get(&id)?;
    let described = facts.description.operations.get(&id)?;
    let mut out = format!(
        "{} is {}:\n\n{}",
        code_span(operation.name()),
        code_span(&format!("{} {}", operation.method, operation.uri)),
        fenced("sh", request["snippet"].as_str().unwrap_or_default()),
    );
    // The response of the example the command sends, else the sample's.
    let example = described["examples"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|example| !example["input"].is_null());
    let response = match example {
        Some(example) if !example["output"].is_null() => &example["output"],
        Some(_) => &Value::Null,
        None => &described["sample"]["output"],
    };
    let status = match &response["responseCode"] {
        Value::Null => &described["status"],
        code => code,
    };
    match &response["body"] {
        Value::Null => {}
        body => out.push_str(&format!(
            "\nIt answers {} with a JSON body:\n\n{}",
            code_span(&status.to_string()),
            fenced(
                "json",
                &serde_json::to_string_pretty(body).expect("a document serializes")
            ),
        )),
    }
    out.push_str(
        "\nEvery operation's page has its request, its response and a command like this one.",
    );
    Some(("call", "Call an operation", out))
}

/// The simple types, in the order the table lists them.
const KINDS: [SimpleKind; 13] = [
    SimpleKind::Boolean,
    SimpleKind::String,
    SimpleKind::Byte,
    SimpleKind::Short,
    SimpleKind::Integer,
    SimpleKind::Long,
    SimpleKind::Float,
    SimpleKind::Double,
    SimpleKind::BigInteger,
    SimpleKind::BigDecimal,
    SimpleKind::Timestamp,
    SimpleKind::Document,
    SimpleKind::Blob,
];

fn kind_name(kind: SimpleKind) -> &'static str {
    match kind {
        SimpleKind::Boolean => "boolean",
        SimpleKind::String => "string",
        SimpleKind::Byte => "8-bit integer (`byte`)",
        SimpleKind::Short => "16-bit integer (`short`)",
        SimpleKind::Integer => "32-bit integer (`integer`)",
        SimpleKind::Long => "64-bit integer (`long`)",
        SimpleKind::Float => "32-bit float (`float`)",
        SimpleKind::Double => "64-bit float (`double`)",
        SimpleKind::BigInteger => "integer of any size (`bigInteger`)",
        SimpleKind::BigDecimal => "decimal of any precision (`bigDecimal`)",
        SimpleKind::Timestamp => "timestamp",
        SimpleKind::Document => "any JSON value (`document`)",
        SimpleKind::Blob => "bytes (`blob`)",
    }
}

/// Whether any member is written in another timestamp format than the
/// protocol's.
fn other_timestamp_formats(service: &Service, default: TimestampFormat) -> bool {
    service.shapes.values().any(|shape| match shape {
        Shape::Structure(s) | Shape::Union(s) => {
            s.members.iter().any(|m| m.wire_timestamp_format != default)
        }
        _ => false,
    })
}

fn json_form(service: &Service, kind: SimpleKind) -> String {
    match kind {
        SimpleKind::Boolean => "`true` or `false`".into(),
        SimpleKind::String => "a string".into(),
        SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer => "a whole number".into(),
        SimpleKind::Long | SimpleKind::BigInteger => "a whole number, every digit written".into(),
        SimpleKind::Float | SimpleKind::Double => "a number".into(),
        SimpleKind::BigDecimal => "a number, every digit written".into(),
        SimpleKind::Timestamp => {
            let default = service.protocol.timestamp_format(false);
            let mut form = match default {
                TimestampFormat::DateTime => {
                    "a string in RFC 3339: `\"2026-01-31T09:30:00Z\"`".to_owned()
                }
                TimestampFormat::HttpDate => {
                    "a string in the HTTP date format: `\"Sat, 31 Jan 2026 09:30:00 GMT\"`"
                        .to_owned()
                }
                TimestampFormat::EpochSeconds => {
                    "a number of seconds since 1970-01-01T00:00:00Z".to_owned()
                }
            };
            if other_timestamp_formats(service, default) {
                form.push_str(", unless the member names another format");
            }
            form
        }
        SimpleKind::Document => "any JSON value".into(),
        SimpleKind::Blob => "a string of base64".into(),
    }
}

/// How each simple type the service uses is written in JSON, then how the
/// shapes built from them are.
fn types(service: &Service) -> Option<Draft> {
    // In one order for every service, not the order the model first uses
    // them in.
    let used = used_kinds(service);
    let kinds: Vec<SimpleKind> = KINDS
        .into_iter()
        .filter(|kind| used.contains(kind))
        .collect();
    if kinds.is_empty() {
        return None;
    }
    let mut out =
        "Each of the API's simple types is written in JSON like this:\n\n| API type | JSON |\n|---|---|\n"
            .to_owned();
    for kind in &kinds {
        out.push_str(&format!(
            "| {} | {} |\n",
            kind_name(*kind),
            json_form(service, *kind)
        ));
    }
    let has = |test: fn(&Shape) -> bool| service.shapes.values().any(test);
    let mut built = vec!["A structure is an object with a member for each of its members"];
    if has(|s| matches!(s, Shape::List(_))) {
        built.push("a list is an array");
    }
    if has(|s| matches!(s, Shape::Map(_))) {
        built.push("a map is an object");
    }
    if has(|s| matches!(s, Shape::Enum(_))) {
        built.push("an enum is its value as a string");
    }
    if has(|s| matches!(s, Shape::IntEnum(_))) {
        built.push("an integer enum is its value as a number");
    }
    if has(|s| matches!(s, Shape::Union(_))) {
        built.push("a union is an object with exactly one member, named for the variant it holds");
    }
    out.push('\n');
    match built.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            out.push_str(&format!("{}, and {last}", rest.join(", ")));
        }
        _ => out.push_str(&built.concat()),
    }
    out.push_str(
        ". Leave an optional member out to send nothing for it. In a path, a query string or a \
         header, a value is written as text.",
    );
    if kinds.iter().any(|kind| {
        matches!(
            kind,
            SimpleKind::Long | SimpleKind::BigInteger | SimpleKind::BigDecimal
        )
    }) {
        out.push_str(
            " A parser that reads every JSON number as a double (JavaScript's `JSON.parse`) \
             rounds a whole number past 2^53: read a response with one that keeps every digit \
             where a value can be that large.",
        );
    }
    Some(("types", "Types", out))
}

/// How a caller follows the pages, with the members' names where every
/// paginated operation shares them.
fn pagination(service: &Service) -> Option<Draft> {
    paginated(service)?;
    let pages: Vec<_> = service
        .operations
        .iter()
        .filter_map(|op| op.paginated.as_ref())
        .collect();
    let names: BTreeSet<_> = pages
        .iter()
        .map(|p| (&p.input_token, &p.output_token, &p.page_size))
        .collect();
    let count = match pages.len() {
        1 => "One operation returns its results in pages".to_owned(),
        n => format!("{n} operations return their results in pages"),
    };
    let mut out = match names.iter().next().filter(|_| names.len() == 1) {
        Some((input, output, size)) => {
            let mut text = format!(
                "{count}. A response carries {} when another page follows: send its value as {} \
                 in the next request, with the other parameters unchanged, until a response has \
                 none.",
                code_span(output),
                code_span(input),
            );
            if let Some(size) = size {
                text.push_str(&format!(" {} asks for a page size.", code_span(size)));
            }
            text
        }
        None => format!(
            "{count}. A response carries a token when another page follows: send it in the next \
             request, with the other parameters unchanged, until a response has none. Each \
             paginated operation's page names the two members."
        ),
    };
    out.push_str(
        " A token is opaque: do not read or build one, and stop if the service returns the same \
         token twice.",
    );
    Some(("pagination", "Pagination", out))
}

/// An error on the wire: the model's first example of one, else an
/// error's generated sample.
fn errors(facts: &Facts) -> Option<Draft> {
    let description = facts.description;
    let first = showcase(facts.service)
        .into_iter()
        .chain(&facts.service.operations)
        .find(|op| !op.errors.is_empty())?;
    let modelled = description
        .operations
        .values()
        .flat_map(|op| op["examples"].as_array().into_iter().flatten())
        .map(|example| &example["error"])
        .find(|error| !error.is_null())
        .map(|error| (&error["message"], error["status"].clone()));
    let (message, status) = modelled.or_else(|| {
        let shape = description.shapes.get(&first.errors.first()?.to_string())?;
        Some((
            &shape["error"]["sample"]["message"],
            shape["error"]["status"].clone(),
        ))
    })?;
    let header = description
        .shapes
        .values()
        .find_map(|shape| shape["error"]["discriminatorHeader"].as_str())?;
    let mut wire = format!("HTTP/1.1 {status}");
    for (name, value) in message["headers"].as_object().into_iter().flatten() {
        wire.push_str(&format!("\n{name}: {}", value.as_str().unwrap_or_default()));
    }
    if !message["body"].is_null() {
        wire.push_str(&format!(
            "\n\n{}",
            serde_json::to_string_pretty(&message["body"]).expect("a document serializes")
        ));
    }
    let mut out = format!(
        "A request that fails answers with a 4xx or 5xx status. The {} header names the error, \
         and the JSON body repeats the name in `__type`, beside the error's members:\n\n{}",
        code_span(header),
        fenced("http", &wire),
    );
    out.push_str(
        "\nTell errors apart by that name, not by the message. Each operation's page lists the \
         errors it returns, with the status of each and whether a retry can succeed.",
    );
    if description.shapes.values().any(|shape| {
        !shape["error"].is_null()
            && (shape["error"]["bodyDiscriminator"].is_null()
                || !shape["error"]["bindings"]["payload"].is_null())
    }) {
        out.push_str(" An error whose body is a payload of its own is named by the header alone.");
    }
    out.push_str(
        " Every response carries the request's ID in `x-request-id`: quote it when you report a \
         failure.",
    );
    Some(("errors", "Errors", out))
}

/// When a retry is safe, and the idempotency token a caller sends itself.
fn retries(service: &Service) -> Draft {
    let mut out = "Retry with backoff, and only where a retry is safe: a throttling error for \
                   any operation; a connection failure, HTTP 500, 502, 503 or 504, or another \
                   error marked retryable, only for an operation that is read-only, idempotent \
                   or takes an idempotency token."
        .to_owned();
    let tokens: Vec<&String> = service
        .operations
        .iter()
        .filter_map(|op| op.idempotency_token.as_ref())
        .collect();
    if !tokens.is_empty() {
        let names: BTreeSet<_> = tokens.iter().collect();
        let count = match tokens.len() {
            1 => "One operation takes an idempotency token".to_owned(),
            n => format!("{n} operations take an idempotency token"),
        };
        let named = match names.iter().next().filter(|_| names.len() == 1) {
            Some(name) => format!(", {}", code_span(name)),
            None => ", which its page names".to_owned(),
        };
        // The shared note opens with what an SDK does; a caller without
        // one needs the rest.
        let rule = IDEMPOTENCY_TOKEN_NOTE
            .split_once("When calling the API directly, send")
            .map_or(IDEMPOTENCY_TOKEN_NOTE, |(_, rule)| rule)
            .trim();
        out.push_str(&format!(
            " {count}{named}: send {rule} The commands in this reference leave the token out, so \
             that a copied command does not replay a request."
        ));
    }
    ("retries", "Retries and idempotency", out)
}
