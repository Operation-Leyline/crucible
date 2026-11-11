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
use crucible_ir::Protocol;
use crucible_ir::ir::{
    Bindings, Operation, ResponseMode, Service, Shape, SimpleKind, TimestampFormat,
};
use serde_json::{Value, json};

use super::{CONTENT_SHA256, quoted};

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
    /// Some operation of the reference sends or answers a body that is
    /// not JSON, one a third party calls included: the reference lists
    /// that operation too, so "a body is JSON" must not read as a rule
    /// without exception.
    pub other_bodies: bool,
}

/// Whether any operation answers with a stream of events rather than one
/// body.
fn has_streams(service: &Service) -> bool {
    service
        .operations
        .iter()
        .any(|op| op.response_mode == ResponseMode::ServerStream)
}

/// The stream's media type without its parameters: what a reader looks for.
fn stream_media_type() -> &'static str {
    Protocol::STREAM_CONTENT_TYPE
        .split(';')
        .next()
        .unwrap_or(Protocol::STREAM_CONTENT_TYPE)
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

/// Whether any operation's request or response body is a payload of its
/// own rather than JSON.
pub(super) fn has_payload(service: &Service) -> bool {
    service
        .operations
        .iter()
        .any(|op| op.request.payload.is_some() || op.response.payload.is_some())
}

/// Whether any error's body is a payload of its own rather than JSON.
fn has_payload_errors(description: &Description) -> bool {
    description.shapes.values().any(|shape| {
        !shape["error"].is_null()
            && (shape["error"]["bodyDiscriminator"].is_null()
                || !shape["error"]["bindings"]["payload"].is_null())
    })
}

/// The endpoint, the credential and the headers every request sends.
fn request(facts: &Facts) -> Draft {
    let service = facts.service;
    let mut out = format!(
        "Every operation is one HTTP request to the API's endpoint. A body, in a request or a \
         response, is JSON{}.{} The commands here read the endpoint from {}",
        // An error's payload counts: a caller must not expect to decode
        // every failure as JSON either.
        if facts.other_bodies || has_payload_errors(facts.description) {
            ", except where its page in the reference names another media type"
        } else {
            ""
        },
        if has_streams(service) {
            format!(
                " A streaming operation answers with server-sent events ({}) instead of one body.",
                code_span(stream_media_type())
            )
        } else {
            String::new()
        },
        code_span(facts.endpoint_env),
    );
    let with_bearer = service.operations.iter().any(|op| bearer(service, op));
    // Without trailing slashes, as the SDKs' runtimes take it: the commands
    // append a URI that starts with one, and `/v1//cities` is another path.
    // Quoted as the commands quote model text: an endpoint may hold an
    // apostrophe, which would otherwise end the string and run what follows.
    let endpoint = facts.endpoint.map_or("https://<endpoint>", |endpoint| {
        endpoint.trim_end_matches('/')
    });
    let mut exports = format!("export {}={}\n", facts.endpoint_env, quoted(endpoint));
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

/// What the first call answers, after its command: `None` where the
/// description has no response to show.
///
/// `response` is a described message, whose `payload` holds the modelled
/// value and its kind, not the bytes that travel. So the kind decides the
/// form: a document is JSON whatever its value (a string is `"hello"`, a
/// null is `null`), a string payload is the text itself, and a blob is
/// named by its media type, since base64 of it is not what is sent.
fn answer(response: &Value, status: &str, stream: bool) -> Option<String> {
    let status = code_span(status);
    let json = |value: &Value| {
        fenced(
            "json",
            &serde_json::to_string_pretty(value).expect("a document serializes"),
        )
    };
    if stream {
        return Some(format!(
            "\nIt answers {status} with a stream of server-sent events ({}); its page says what \
             each event carries.\n",
            code_span(stream_media_type())
        ));
    }
    if !response["body"].is_null() {
        return Some(format!(
            "\nIt answers {status} with a JSON body:\n\n{}",
            json(&response["body"])
        ));
    }
    let payload = response["payload"].as_object()?;
    // A payload's media type, where the model names one.
    let media = payload
        .get("mediaType")
        .and_then(Value::as_str)
        .map(|media| format!(" of {}", code_span(media)))
        .unwrap_or_default();
    let value = payload.get("value").unwrap_or(&Value::Null);
    Some(match payload.get("kind").and_then(Value::as_str) {
        Some("blob") => {
            format!("\nIt answers {status} with a body{media}: the bytes themselves, not JSON.\n")
        }
        Some("string") => format!(
            "\nIt answers {status} with a body{media}:\n\n{}",
            fenced("text", value.as_str().unwrap_or_default())
        ),
        _ => format!("\nIt answers {status} with a JSON body:\n\n{}", json(value)),
    })
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
    let stream = operation.response_mode == ResponseMode::ServerStream;
    if let Some(answer) = answer(response, &status.to_string(), stream) {
        out.push_str(&answer);
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
        // Base64 only inside JSON: as an operation's whole body the bytes
        // travel as they are.
        SimpleKind::Blob => {
            "a string of base64 in a JSON body; the bytes themselves where it is the whole body"
                .into()
        }
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

/// Where `member` of a request or response travels, as a phrase: a query
/// parameter or header by the name it is bound to, the whole body for a
/// payload, its segment of the path for a label, else the member in the
/// JSON body. `member` may be a path into
/// the output (`result.next`); only a top-level member can be bound.
fn place(bindings: &Bindings, member: &str) -> String {
    if let Some(binding) = bindings.query.iter().find(|b| b.member == member) {
        return format!("the query parameter {}", code_span(&binding.name));
    }
    if let Some(binding) = bindings.headers.iter().find(|b| b.member == member) {
        return format!("the {} header", code_span(&binding.name));
    }
    if bindings
        .payload
        .as_ref()
        .is_some_and(|p| p.member == member)
    {
        return "the whole body".to_owned();
    }
    if bindings.labels.iter().any(|label| label == member) {
        return format!(
            "the {} segment of the path",
            code_span(&format!("{{{member}}}"))
        );
    }
    format!("{} in the JSON body", code_span(member))
}

/// How a caller follows the pages, naming where the tokens travel where
/// every paginated operation agrees on it.
fn pagination(service: &Service) -> Option<Draft> {
    paginated(service)?;
    // Where each member travels, not its name in the model: a token bound
    // to a renamed query parameter or a header is sent under that name.
    let places: Vec<(String, String, Option<String>)> = service
        .operations
        .iter()
        .filter_map(|op| {
            let pages = op.paginated.as_ref()?;
            Some((
                place(&op.request, &pages.input_token),
                place(&op.response, &pages.output_token),
                pages
                    .page_size
                    .as_ref()
                    .map(|size| place(&op.request, size)),
            ))
        })
        .collect();
    let distinct: BTreeSet<_> = places.iter().collect();
    let count = match places.len() {
        1 => "One operation returns its results in pages".to_owned(),
        n => format!("{n} operations return their results in pages"),
    };
    let mut out = match distinct.iter().next().filter(|_| distinct.len() == 1) {
        Some((input, output, size)) => {
            let mut text = format!(
                "{count}. A response carries the next page's token ({output}) when another page \
                 follows: send it as {input} in the next request, with the other parameters \
                 unchanged, until a response has none."
            );
            if let Some(size) = size {
                text.push_str(&format!(" A page size is asked for with {size}."));
            }
            text
        }
        None => format!(
            "{count}. A response carries the next page's token when another page follows: send \
             it in the next request, with the other parameters unchanged, until a response has \
             none. Where the token is read from and sent differs between operations: each \
             paginated operation's page says."
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
    // Every error the guide could show: the model's examples, then each
    // error's generated sample. One with a JSON body is shown first, since
    // that is the form most errors have.
    let examples = description
        .operations
        .values()
        .flat_map(|op| op["examples"].as_array().into_iter().flatten())
        .map(|example| &example["error"])
        .filter(|error| !error.is_null())
        .map(|error| (&error["message"], error["status"].clone()));
    let samples = first
        .errors
        .iter()
        .chain(facts.service.operations.iter().flat_map(|op| &op.errors))
        .filter_map(|id| description.shapes.get(&id.to_string()))
        .map(|shape| {
            (
                &shape["error"]["sample"]["message"],
                shape["error"]["status"].clone(),
            )
        });
    let candidates: Vec<_> = examples.chain(samples).collect();
    let (message, status) = candidates
        .iter()
        .find(|(message, _)| !message["body"].is_null())
        .or_else(|| candidates.first())?;
    let header = description
        .shapes
        .values()
        .find_map(|shape| shape["error"]["discriminatorHeader"].as_str())?;
    let mut wire = format!("HTTP/1.1 {status}");
    for (name, value) in message["headers"].as_object().into_iter().flatten() {
        wire.push_str(&format!("\n{name}: {}", value.as_str().unwrap_or_default()));
    }
    let json_body = !message["body"].is_null();
    if json_body {
        wire.push_str(&format!(
            "\n\n{}",
            serde_json::to_string_pretty(&message["body"]).expect("a document serializes")
        ));
    }
    // `__type` is said only of an error that has a JSON body: one whose
    // body is its own payload has none to look in.
    let mut out = format!(
        "A request that fails answers with a 4xx or 5xx status. The {} header names the error{}:\n\n{}",
        code_span(header),
        if json_body {
            ", and an error's JSON body repeats the name in `__type`, beside the error's members"
        } else {
            ""
        },
        fenced("http", &wire),
    );
    out.push_str(
        "\nTell errors apart by that name, not by the message. Each operation's page lists the \
         errors it returns, with the status of each and whether a retry can succeed.",
    );
    if has_payload_errors(description) {
        out.push_str(
            " An error whose body is a payload of its own has no `__type`: the header alone \
             names it.",
        );
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
    // Where each operation's token travels, as for a page token: one bound
    // to a query parameter or a header is sent under that name, not its
    // member's.
    let places: Vec<String> = service
        .operations
        .iter()
        .filter_map(|op| Some(place(&op.request, op.idempotency_token.as_ref()?)))
        .collect();
    if !places.is_empty() {
        let distinct: BTreeSet<_> = places.iter().collect();
        let count = match places.len() {
            1 => "One operation takes an idempotency token".to_owned(),
            n => format!("{n} operations take an idempotency token"),
        };
        let named = match distinct.iter().next().filter(|_| distinct.len() == 1) {
            Some(place) => format!(", sent as {place}"),
            None => ", sent where each one's page says".to_owned(),
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

#[cfg(test)]
mod tests {
    use super::{answer, place};
    use crucible_ir::ir::{Binding, Bindings};
    use serde_json::json;

    /// A payload's kind, not its value's type, decides how the response is
    /// shown: a document that is a string or a null is still JSON.
    #[test]
    fn a_payload_is_shown_as_its_kind_travels() {
        let payload = |kind: &str, value: serde_json::Value| json!({"body": null, "payload": {"kind": kind, "mediaType": "x/y", "value": value}});
        let document = answer(&payload("document", json!("hello")), "200", false).unwrap();
        assert!(
            document.contains("with a JSON body") && document.contains("\"hello\""),
            "{document}"
        );
        let null = answer(&payload("document", json!(null)), "200", false).unwrap();
        assert!(null.contains("```json\nnull\n```"), "{null}");
        let text = answer(&payload("string", json!("hello")), "200", false).unwrap();
        assert!(
            text.contains("a body of `x/y`") && text.contains("```text\nhello\n```"),
            "{text}"
        );
        let bytes = answer(&payload("blob", json!("aGVsbG8=")), "200", false).unwrap();
        assert!(
            bytes.contains("the bytes themselves") && !bytes.contains("aGVsbG8="),
            "{bytes}"
        );
        // Nothing described, nothing claimed.
        assert_eq!(
            answer(&json!({"body": null, "payload": null}), "204", false),
            None
        );
    }

    /// A member is placed where its binding sends it, a path label included.
    #[test]
    fn a_member_is_placed_where_it_is_bound() {
        let bindings = Bindings {
            labels: vec!["limit".into()],
            query: vec![Binding {
                member: "nextToken".into(),
                name: "token".into(),
            }],
            headers: vec![Binding {
                member: "cursor".into(),
                name: "X-Cursor".into(),
            }],
            ..Bindings::default()
        };
        assert_eq!(
            place(&bindings, "limit"),
            "the `{limit}` segment of the path"
        );
        assert_eq!(place(&bindings, "nextToken"), "the query parameter `token`");
        assert_eq!(place(&bindings, "cursor"), "the `X-Cursor` header");
        assert_eq!(place(&bindings, "next"), "`next` in the JSON body");
    }
}
