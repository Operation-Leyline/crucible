//! What a call snippet says, before any language spells it: the input
//! value it sends, as a typed tree, the modelled error it handles, the
//! pagination it follows, and the request headers the client adds.
//!
//! The value is the operation's first `example` with an input, and
//! otherwise its generated [`sample`](crate::sample) (every member, so the
//! snippet shows the whole input). Each client generator renders a [`Call`]
//! with its own naming functions, into the `snippet` and `paginatedSnippet`
//! of `crucible.sdks/1` (docs/docs.md) and the package README: the tree
//! settles once what a model value means (which enum value, which union
//! arm, which instant, which bytes), so no generator re-reads model JSON.

use base64::Engine as _;
use crucible_ir::ShapeId;
use crucible_ir::ir::{
    EnumValue, IntEnumValue, Member, Operation, Paginated, Service, Shape, SimpleKind, Structure,
    TimestampFormat,
};
use serde_json::Value;

use crate::sample;

/// A value of a shape, typed: what a native literal renders.
#[derive(Debug, Clone)]
pub struct Node<'a> {
    /// The shape the value is of: a member's target, or the root structure.
    pub target: &'a ShapeId,
    pub kind: Kind<'a>,
}

/// What a [`Node`] holds.
#[derive(Debug, Clone)]
pub enum Kind<'a> {
    /// An explicit null: a sparse collection's element, or a value the
    /// model leaves nullable.
    Null,
    /// A union arm that carries nothing.
    Unit,
    Boolean(bool),
    String(String),
    /// `byte`, `short`, `integer`, `long`, `float`, `double`, `bigInteger`
    /// and `bigDecimal`: the number's exact decimal text.
    Number {
        kind: SimpleKind,
        text: String,
    },
    Timestamp(Timestamp),
    Blob(Vec<u8>),
    Document(Value),
    Enum(&'a EnumValue),
    IntEnum(&'a IntEnumValue),
    List(Vec<Node<'a>>),
    /// Entries in the value's order; keys are strings or enum values.
    Map(Vec<(Node<'a>, Node<'a>)>),
    /// The members the value sets, in model order.
    Structure(Vec<(&'a Member, Node<'a>)>),
    /// The arm the value sets.
    Union(&'a Member, Box<Node<'a>>),
}

/// An instant, in each form a language constructs one from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp {
    /// RFC 3339 in UTC, `2026-01-15T09:30:00Z`, with as many fraction
    /// digits as the value has.
    pub rfc3339: String,
    /// Whole seconds since the Unix epoch, rounded down.
    pub seconds: i64,
    /// Nanoseconds past `seconds`.
    pub nanos: u32,
}

impl Timestamp {
    /// Milliseconds since the epoch, rounded down: what JavaScript's `Date`
    /// holds.
    pub fn millis(&self) -> i64 {
        self.seconds * 1000 + i64::from(self.nanos / 1_000_000)
    }

    /// Seconds since the epoch as decimal text: `1768469400`, or
    /// `1768469400.25` with a fraction.
    pub fn epoch_text(&self) -> String {
        if self.nanos == 0 {
            return self.seconds.to_string();
        }
        let fraction = format!("{:09}", self.nanos);
        format!("{}.{}", self.seconds, fraction.trim_end_matches('0'))
    }
}

impl Node<'_> {
    /// Whether the value is a structure, list, map or union: a renderer
    /// spreads one over several lines.
    pub fn is_aggregate(&self) -> bool {
        matches!(
            self.kind,
            Kind::List(_) | Kind::Map(_) | Kind::Structure(_) | Kind::Union(..)
        )
    }
}

/// The value of `member` written as `value` in model form (member names,
/// RFC 3339 or epoch timestamps, base64 blobs), typed by the member's
/// target.
pub fn member<'a>(service: &'a Service, member: &'a Member, value: &Value) -> Node<'a> {
    let target = &member.target;
    if value.is_null() {
        return Node {
            target,
            kind: Kind::Null,
        };
    }
    let kind = match service.shape(target) {
        Some(Shape::Unit { .. }) => Kind::Unit,
        Some(Shape::Simple(simple)) => simple_kind(service, member, simple.kind, value),
        Some(Shape::Enum(shape)) => {
            let text = value.as_str().unwrap_or_default();
            match shape.values.iter().find(|v| v.value == text) {
                Some(found) => Kind::Enum(found),
                None => Kind::String(text.to_owned()),
            }
        }
        Some(Shape::IntEnum(shape)) => {
            let number = value.as_i64();
            match shape
                .values
                .iter()
                .find(|v| Some(i64::from(v.value)) == number)
            {
                Some(found) => Kind::IntEnum(found),
                None => Kind::Number {
                    kind: SimpleKind::Integer,
                    text: value.to_string(),
                },
            }
        }
        Some(Shape::List(list)) => Kind::List(
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| self::member(service, &list.member, item))
                .collect(),
        ),
        Some(Shape::Map(map)) => Kind::Map(
            value
                .as_object()
                .into_iter()
                .flatten()
                .map(|(key, entry)| {
                    (
                        self::member(service, &map.key, &Value::String(key.clone())),
                        self::member(service, &map.value, entry),
                    )
                })
                .collect(),
        ),
        Some(Shape::Structure(shape)) => return structure(service, target, shape, value),
        Some(Shape::Union(shape)) => {
            let object = value.as_object();
            let arm = object.and_then(|object| {
                shape
                    .members
                    .iter()
                    .find_map(|m| object.get(&m.name).map(|value| (m, value)))
            });
            match arm {
                Some((arm, value)) => Kind::Union(arm, Box::new(self::member(service, arm, value))),
                None => Kind::Document(value.clone()),
            }
        }
        None => Kind::Document(value.clone()),
    };
    Node { target, kind }
}

/// A structure value: the members `value` sets, in model order.
fn structure<'a>(
    service: &'a Service,
    id: &'a ShapeId,
    shape: &'a Structure,
    value: &Value,
) -> Node<'a> {
    let object = value.as_object();
    let members = shape
        .members
        .iter()
        .filter_map(|m| {
            let value = object?.get(&m.name)?;
            Some((m, member(service, m, value)))
        })
        .collect();
    Node {
        target: id,
        kind: Kind::Structure(members),
    }
}

fn simple_kind<'a>(
    service: &Service,
    member: &Member,
    kind: SimpleKind,
    value: &Value,
) -> Kind<'a> {
    match kind {
        SimpleKind::Boolean => Kind::Boolean(value.as_bool().unwrap_or_default()),
        SimpleKind::String => Kind::String(value.as_str().unwrap_or_default().to_owned()),
        SimpleKind::Timestamp => match timestamp(service, member, value) {
            Some(instant) => Kind::Timestamp(instant),
            None => Kind::Document(value.clone()),
        },
        SimpleKind::Blob => Kind::Blob(
            value
                .as_str()
                .and_then(|text| base64::engine::general_purpose::STANDARD.decode(text).ok())
                .unwrap_or_default(),
        ),
        SimpleKind::Document => Kind::Document(value.clone()),
        number => Kind::Number {
            kind: number,
            text: match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            },
        },
    }
}

fn timestamp(service: &Service, member: &Member, value: &Value) -> Option<Timestamp> {
    let rfc3339 = service.wire_example(member, value, Some(TimestampFormat::DateTime));
    let epoch = service.wire_example(member, value, Some(TimestampFormat::EpochSeconds));
    let rfc3339 = rfc3339.as_str()?.to_owned();
    let text = match &epoch {
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let whole: i64 = whole.parse().ok()?;
    let digits = format!("{:0<9}", &fraction[..fraction.len().min(9)]);
    let mut nanos: u32 = digits.parse().ok()?;
    let mut seconds = whole;
    // `-0.25` is a quarter second before the epoch: -1 s plus 0.75 s.
    if text.starts_with('-') && nanos > 0 {
        seconds -= 1;
        nanos = 1_000_000_000 - nanos;
    }
    Some(Timestamp {
        rfc3339,
        seconds,
        nanos,
    })
}

/// The bytes standard padded base64 `text` (a model or wire blob) holds,
/// or `None` when it is not base64.
pub fn base64_bytes(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

/// `bytes` as standard padded base64.
pub fn base64_text(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The modelled error a snippet handles, and the member it reads.
#[derive(Debug, Clone, Copy)]
pub struct ErrorCase<'a> {
    pub id: &'a ShapeId,
    pub shape: &'a Structure,
    /// The member a handler prints: `message` where the error has one,
    /// else its first member; `None` for an error without members.
    pub read: Option<&'a Member>,
}

/// What a paginated snippet follows.
#[derive(Debug, Clone, Copy)]
pub struct Pages<'a> {
    pub paginated: &'a Paginated,
    /// The page's top-level output member holding its items, where the
    /// model names one directly on the output.
    pub items: Option<&'a Member>,
}

/// One operation's snippet, before a language spells it.
#[derive(Debug, Clone)]
pub struct Call<'a> {
    pub operation: &'a Operation,
    /// The input structure's value: `None` for an operation without input.
    pub input: Option<Node<'a>>,
    /// The title of the example the input comes from; `None` when it is
    /// the generated sample.
    pub example: Option<&'a str>,
    pub output: Option<&'a Structure>,
    pub error: Option<ErrorCase<'a>>,
    pub pages: Option<Pages<'a>>,
    /// The call sends a bearer token: the operation is not public and
    /// names a bearer scheme.
    pub bearer: bool,
}

/// The snippet for `operation` of `service` (the projected service the
/// client is generated from).
pub fn call<'a>(service: &'a Service, operation: &'a Operation) -> Call<'a> {
    let input_shape = operation
        .input
        .as_ref()
        .and_then(|id| match service.shape(id) {
            Some(Shape::Structure(shape)) => Some((id, shape)),
            _ => None,
        });
    let example = operation
        .examples
        .iter()
        .find(|example| example.input.is_some());
    let (input, title) = match input_shape {
        None => (None, None),
        Some((id, shape)) => match example.and_then(|e| e.input.as_ref().map(|v| (e, v))) {
            Some((example, value)) => (
                Some(structure(service, id, shape, value)),
                Some(example.title.as_str()),
            ),
            None => {
                let text = text_members(operation);
                let generated =
                    sample::structure(service, Some(id), sample::Variant::Full, "input", &text);
                // A long optional value (a hundred-entry list, a long
                // string its minimum length asks for) would bury the call:
                // a sample shows it only where the input needs it.
                let mut value = generated.value;
                if let Some(object) = value.as_object_mut() {
                    object.retain(|name, member| {
                        let optional = shape
                            .members
                            .iter()
                            .find(|m| &m.name == name)
                            .is_some_and(|m| !m.required_without_default());
                        !optional || member.to_string().len() <= SAMPLE_BUDGET
                    });
                }
                let mut node = structure(service, id, shape, &value);
                // A fixed idempotency token in copied code would replay
                // one logical request forever: the client fills it.
                if let Kind::Structure(members) = &mut node.kind {
                    members.retain(|(m, _)| !m.idempotency_token);
                }
                (Some(node), None)
            }
        },
    };
    let output = operation
        .output
        .as_ref()
        .and_then(|id| match service.shape(id) {
            Some(Shape::Structure(shape)) => Some(shape),
            _ => None,
        });
    let error = operation
        .examples
        .iter()
        .find_map(|example| example.error.as_ref().map(|error| &error.shape))
        .filter(|id| operation.errors.contains(id))
        .or_else(|| operation.errors.first())
        .and_then(|id| match service.shape(id) {
            Some(Shape::Structure(shape)) => Some(ErrorCase {
                id,
                shape,
                read: shape
                    .members
                    .iter()
                    .find(|m| m.name == "message")
                    .or_else(|| shape.members.first()),
            }),
            _ => None,
        });
    let pages = operation.paginated.as_ref().map(|paginated| Pages {
        paginated,
        items: paginated
            .items
            .as_deref()
            .filter(|path| !path.contains('.'))
            .and_then(|name| output?.members.iter().find(|m| m.name == name)),
    });
    let bearer = !operation.public && operation.auth.iter().any(|s| service.is_bearer(s));
    Call {
        operation,
        input,
        example: title,
        output,
        error,
        pages,
        bearer,
    }
}

/// The longest optional member value, as JSON text, a sampled snippet
/// shows; a longer one is left out.
const SAMPLE_BUDGET: usize = 160;

/// The members bound to a header, a path label or the query string.
fn text_members(operation: &Operation) -> Vec<&str> {
    let request = &operation.request;
    request
        .labels
        .iter()
        .map(String::as_str)
        .chain(request.query.iter().map(|b| b.member.as_str()))
        .chain(request.headers.iter().map(|b| b.member.as_str()))
        .collect()
}

/// The operation a package README shows: the first, by shape ID, with an
/// example input and a read-only one first; else the first with input;
/// else the first.
pub fn showcase(service: &Service) -> Option<&Operation> {
    let with_example = |op: &&Operation| op.examples.iter().any(|e| e.input.is_some());
    service
        .operations
        .iter()
        .filter(with_example)
        .find(|op| op.readonly)
        .or_else(|| service.operations.iter().find(with_example))
        .or_else(|| service.operations.iter().find(|op| op.input.is_some()))
        .or_else(|| service.operations.first())
}

/// The first paginated operation, by shape ID: a README's pagination
/// example.
pub fn paginated(service: &Service) -> Option<&Operation> {
    service.operations.iter().find(|op| op.paginated.is_some())
}

/// `{SERVICE}_{suffix}`: the environment variable a snippet reads its
/// endpoint (`ENDPOINT`) or bearer token (`TOKEN`) from, so no snippet
/// holds a credential. `LeylineDbaas` gives `LEYLINE_DBAAS_TOKEN`.
pub fn env_var(service: &Service, suffix: &str) -> String {
    let mut out = String::new();
    let name = service.id.name();
    let chars: Vec<char> = name.chars().collect();
    for (at, c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            continue;
        }
        let boundary = at > 0
            && c.is_uppercase()
            && (chars[at - 1].is_lowercase()
                || chars[at - 1].is_ascii_digit()
                || chars.get(at + 1).is_some_and(|next| next.is_lowercase()));
        if boundary && !out.ends_with('_') {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    if !out.ends_with('_') {
        out.push('_');
    }
    out.push_str(suffix);
    out
}

/// What the client adds to every request beside the operation's own
/// headers, for a snippet's comment: `Authorization: Bearer` where the
/// service has bearer auth, `Crucible-Version`, and each default
/// middleware's headers.
pub fn added_headers(service: &Service, middleware: &[crate::surface::Middleware]) -> Vec<String> {
    let mut headers = Vec::new();
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        headers.push("Authorization: Bearer <token>".to_owned());
    }
    if let Some(version) = &service.api_version {
        headers.push(format!("Crucible-Version: {version}"));
    }
    for middleware in middleware {
        for header in &middleware.headers {
            headers.push(format!("{header} ({})", middleware.name));
        }
    }
    headers
}

/// [`added_headers`] as one sentence, or `None` when there are none:
/// `The client sends Authorization: Bearer <token>, Crucible-Version:
/// 2026-10-01 and x-amz-content-sha256 (content-sha256) on every request.`
/// For a code comment; [`added_headers_markdown`] is the Markdown form.
pub fn added_headers_sentence(
    service: &Service,
    middleware: &[crate::surface::Middleware],
) -> Option<String> {
    sentence(added_headers(service, middleware))
}

/// [`added_headers_sentence`] with each header as a Markdown code span,
/// which no model text in a header can close.
pub fn added_headers_markdown(
    service: &Service,
    middleware: &[crate::surface::Middleware],
) -> Option<String> {
    sentence(
        added_headers(service, middleware)
            .into_iter()
            .map(|header| match header.split_once(" (") {
                Some((header, rest)) => format!(
                    "{} ({})",
                    crate::guide::code_span(header),
                    crate::guide::markdown_text(rest.trim_end_matches(')'))
                ),
                None => crate::guide::code_span(&header),
            })
            .collect(),
    )
}

fn sentence(headers: Vec<String>) -> Option<String> {
    let (last, rest) = headers.split_last()?;
    let list = if rest.is_empty() {
        last.clone()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    Some(format!("The client sends {list} on every request."))
}

#[cfg(test)]
mod tests {
    use super::Timestamp;

    #[test]
    fn epoch_text_keeps_the_fraction() {
        let instant = Timestamp {
            rfc3339: String::new(),
            seconds: 1_768_469_400,
            nanos: 250_000_000,
        };
        assert_eq!(instant.epoch_text(), "1768469400.25");
        assert_eq!(instant.millis(), 1_768_469_400_250);
    }
}
