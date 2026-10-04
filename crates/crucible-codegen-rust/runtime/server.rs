//! crucible server runtime. Generated crates carry a copy of this file, so a
//! crate and its runtime can never disagree. Edit it in the crucible
//! repository (crates/crucible-codegen-rust/runtime/), never in a generated crate.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fmt;

use axum::body::Bytes;
use axum::extract::RawPathParams;
use axum::http::{Extensions, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// What a handler sees of the request besides its input: the place for
/// whatever middleware learned (a principal, a request id) in `extensions`.
#[derive(Debug)]
pub struct Context {
    pub method: Method,
    pub uri: Uri,
    pub headers: HeaderMap,
    pub extensions: Extensions,
}

/// A union member or result that carries no value: `{}` on the wire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {}

/// One way a request broke the model, at a `/`-separated path of member
/// names, list indexes and map keys (or a parameter or header name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: String,
    pub message: String,
    pub kind: ViolationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// Not readable as the model's types: bad JSON, a word where a number
    /// goes, an unknown enum value.
    Malformed,
    /// Readable, but outside the model: a required member missing, a length,
    /// range or pattern broken.
    Constraint,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// A request that could not become an operation's input. Every violation
/// found is listed, not just the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub violations: Vec<Violation>,
}

impl Rejection {
    /// A broken constraint, such as a required member that is missing.
    pub fn one(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::of(ViolationKind::Constraint, path, message)
    }

    pub fn malformed(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::of(ViolationKind::Malformed, path, message)
    }

    fn of(kind: ViolationKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            violations: vec![Violation {
                path: path.into(),
                message: message.into(),
                kind,
            }],
        }
    }

    /// Whether any violation is a malformed request rather than a broken constraint.
    pub fn is_malformed(&self) -> bool {
        self.violations.iter().any(|v| v.kind == ViolationKind::Malformed)
    }

    /// One line for an error message: the first violation, and how many more.
    pub fn summary(&self) -> String {
        match self.violations.as_slice() {
            [] => "the request is invalid".to_owned(),
            [only] => only.to_string(),
            [first, rest @ ..] => format!("{first} (and {} more)", rest.len()),
        }
    }
}

/// The parts of a request that bound members come from.
pub struct Parts {
    labels: Vec<(String, String)>,
    query: Vec<(String, String)>,
    headers: HeaderMap,
}

impl Parts {
    /// `labels` is `None` for a route without labels, where axum has no
    /// path parameters to give.
    pub fn new(labels: Option<&RawPathParams>, query: Option<&str>, headers: &HeaderMap) -> Self {
        Self {
            labels: labels
                .map(|labels| {
                    labels
                        .iter()
                        .map(|(name, value)| (name.to_owned(), value.to_owned()))
                        .collect()
                })
                .unwrap_or_default(),
            query: query.map(parse_query).unwrap_or_default(),
            headers: headers.clone(),
        }
    }

    pub fn label(&self, name: &str) -> Result<&str, Rejection> {
        self.labels
            .iter()
            .find(|(label, _)| label == name)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Rejection::one(name, "is required"))
    }

    pub fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn query_all(&self, name: &str) -> Vec<&str> {
        self.query
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    pub fn header(&self, name: &str) -> Result<Option<&str>, Rejection> {
        match self.headers.get(name) {
            None => Ok(None),
            Some(value) => value
                .to_str()
                .map(Some)
                .map_err(|_| Rejection::one(name, "is not visible ASCII")),
        }
    }
}

/// `a=1&b=x%20y&b=z` into decoded pairs, in order. `+` is a space.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A value read from a label, query parameter or header.
pub trait FromText: Sized {
    fn from_text(text: &str) -> Result<Self, String>;
}

impl FromText for String {
    fn from_text(text: &str) -> Result<Self, String> {
        Ok(text.to_owned())
    }
}

impl FromText for bool {
    fn from_text(text: &str) -> Result<Self, String> {
        match text {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err("must be true or false".to_owned()),
        }
    }
}

macro_rules! numbers {
    ($($ty:ty),*) => {$(
        impl FromText for $ty {
            fn from_text(text: &str) -> Result<Self, String> {
                text.parse().map_err(|_| format!("must be a number (`{text}` is not)"))
            }
        }
        impl ToText for $ty {
            fn to_text(&self) -> String {
                self.to_string()
            }
        }
    )*};
}
numbers!(i8, i16, i32, i64, f32, f64);

impl FromText for DateTime<Utc> {
    fn from_text(text: &str) -> Result<Self, String> {
        DateTime::parse_from_rfc3339(text)
            .map(|time| time.with_timezone(&Utc))
            .map_err(|_| "must be an RFC 3339 timestamp".to_owned())
    }
}

/// A value written into a response header.
pub trait ToText {
    fn to_text(&self) -> String;
}

impl ToText for String {
    fn to_text(&self) -> String {
        self.clone()
    }
}

impl ToText for bool {
    fn to_text(&self) -> String {
        self.to_string()
    }
}

impl ToText for DateTime<Utc> {
    fn to_text(&self) -> String {
        self.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
    }
}

/// Timestamps in the two formats `@timestampFormat` can choose besides the
/// default `date-time`.
pub mod epoch_seconds {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn to_f64(time: &DateTime<Utc>) -> f64 {
        time.timestamp_millis() as f64 / 1000.0
    }

    pub fn from_f64(seconds: f64) -> Option<DateTime<Utc>> {
        DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64)
    }

    pub fn serialize<S: Serializer>(time: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(to_f64(time))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<Utc>, D::Error> {
        let seconds = f64::deserialize(deserializer)?;
        from_f64(seconds).ok_or_else(|| serde::de::Error::custom("timestamp out of range"))
    }

    pub mod option {
        use chrono::{DateTime, Utc};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(time: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error> {
            match time {
                Some(time) => super::serialize(time, serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error> {
            match Option::<f64>::deserialize(deserializer)? {
                Some(seconds) => super::from_f64(seconds)
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("timestamp out of range")),
                None => Ok(None),
            }
        }
    }
}

pub mod http_date {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn format(time: &DateTime<Utc>) -> String {
        time.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
    }

    pub fn parse(text: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc2822(text).ok().map(|time| time.with_timezone(&Utc))
    }

    pub fn serialize<S: Serializer>(time: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format(time))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DateTime<Utc>, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse(&text).ok_or_else(|| serde::de::Error::custom("not an HTTP date"))
    }

    pub mod option {
        use chrono::{DateTime, Utc};
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(time: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error> {
            match time {
                Some(time) => super::serialize(time, serializer),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error> {
            match Option::<String>::deserialize(deserializer)? {
                Some(text) => super::parse(&text)
                    .map(Some)
                    .ok_or_else(|| serde::de::Error::custom("not an HTTP date")),
                None => Ok(None),
            }
        }
    }
}

pub fn parse_epoch_seconds(text: &str) -> Result<DateTime<Utc>, String> {
    text.parse::<f64>()
        .ok()
        .and_then(epoch_seconds::from_f64)
        .ok_or_else(|| "must be seconds since the epoch".to_owned())
}

pub fn parse_http_date(text: &str) -> Result<DateTime<Utc>, String> {
    http_date::parse(text).ok_or_else(|| "must be an HTTP date".to_owned())
}

/// A header list: comma-separated, items quoted when they hold a comma or quote.
pub fn split_header(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut item = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => item.extend(chars.next()),
                    '"' => break,
                    c => item.push(c),
                }
            }
            while chars.peek().is_some_and(|c| *c != ',') {
                chars.next();
            }
        } else {
            while let Some(c) = chars.peek().copied().filter(|c| *c != ',') {
                item.push(c);
                chars.next();
            }
            item = item.trim_end().to_owned();
        }
        items.push(item);
        if chars.next().is_none() {
            break;
        }
    }
    if items.len() == 1 && items[0].is_empty() {
        items.clear();
    }
    items
}

pub fn join_header(items: impl IntoIterator<Item = String>) -> String {
    items
        .into_iter()
        .map(|item| {
            if item.contains([',', '"']) {
                format!("\"{}\"", item.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                item
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Keeps a value, or adds its violations to `rejection` so every problem in a
/// request is reported at once.
pub fn take<T>(rejection: &mut Rejection, result: Result<T, Rejection>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(failed) => {
            rejection.violations.extend(failed.violations);
            None
        }
    }
}

/// Reads a bound member from text, naming the member when it fails.
pub fn read<T>(name: &str, text: &str, parse: impl FnOnce(&str) -> Result<T, String>) -> Result<T, Rejection> {
    parse(text).map_err(|message| Rejection::malformed(name, message))
}

/// The JSON body of a request. An empty body reads as `{}`.
pub fn json_body<T: DeserializeOwned>(body: &Bytes) -> Result<T, Rejection> {
    let body: &[u8] = if body.is_empty() { b"{}" } else { body };
    serde_json::from_slice(body).map_err(|error| {
        let message = match error.classify() {
            serde_json::error::Category::Data => error.to_string(),
            _ => format!("the body is not valid JSON: {error}"),
        };
        Rejection::malformed("", message)
    })
}

/// Collects constraint violations while walking a value.
#[derive(Default)]
pub struct Validator {
    path: Vec<String>,
    violations: Vec<Violation>,
}

impl Validator {
    pub fn finish(self) -> Result<(), Rejection> {
        if self.violations.is_empty() {
            Ok(())
        } else {
            Err(Rejection {
                violations: self.violations,
            })
        }
    }

    pub fn at(&mut self, segment: impl fmt::Display, check: impl FnOnce(&mut Self)) {
        self.path.push(segment.to_string());
        check(self);
        self.path.pop();
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.violations.push(Violation {
            path: self.path.join("/"),
            message: message.into(),
            kind: ViolationKind::Constraint,
        });
    }

    pub fn length(&mut self, length: usize, min: Option<u64>, max: Option<u64>) {
        let length = length as u64;
        match (min, max) {
            (Some(min), Some(max)) if length < min || length > max => {
                self.fail(format!("length must be between {min} and {max}, inclusive"))
            }
            (Some(min), None) if length < min => self.fail(format!("length must be at least {min}")),
            (None, Some(max)) if length > max => self.fail(format!("length must be at most {max}")),
            _ => {}
        }
    }

    pub fn range_integer(&mut self, value: i128, min: Option<i128>, max: Option<i128>) {
        if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
            self.fail(range_message(min.map(|m| m.to_string()), max.map(|m| m.to_string())));
        }
    }

    pub fn range_float(&mut self, value: f64, min: Option<f64>, max: Option<f64>) {
        if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
            self.fail(range_message(min.map(|m| m.to_string()), max.map(|m| m.to_string())));
        }
    }

    pub fn pattern(&mut self, text: &str, regex: &regex::Regex, pattern: &str) {
        if !regex.is_match(text) {
            self.fail(format!("must match the pattern {pattern}"));
        }
    }

    pub fn unique<T: Serialize>(&mut self, items: &[T]) {
        let mut seen = BTreeSet::new();
        for item in items {
            if !seen.insert(serde_json::to_string(item).unwrap_or_default()) {
                self.fail("items must be unique");
                return;
            }
        }
    }
}

fn range_message(min: Option<String>, max: Option<String>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("must be between {min} and {max}, inclusive"),
        (Some(min), None) => format!("must be at least {min}"),
        (None, Some(max)) => format!("must be at most {max}"),
        (None, None) => "is out of range".to_owned(),
    }
}

/// A value that checks its own constraints and its members'.
pub trait Validate {
    fn validate(&self, validator: &mut Validator);
}

impl<T: Validate> Validate for Box<T> {
    fn validate(&self, validator: &mut Validator) {
        (**self).validate(validator);
    }
}

/// A modelled error: its wire name, status and header members.
pub trait ModelledError: Serialize {
    const TYPE: &'static str;
    const STATUS: u16;

    fn headers(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// The error response: the error's members plus `"__type"`.
pub fn error_response<E: ModelledError>(error: &E) -> Response {
    let mut body = serde_json::to_value(error).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = body.as_object_mut() {
        object.insert("__type".to_owned(), serde_json::Value::String(E::TYPE.to_owned()));
    }
    respond(E::STATUS, error.headers(), Some(body.to_string()))
}

/// A successful response: status, header members, and the body members as JSON.
pub fn ok_response<T: Serialize>(status: u16, headers: Vec<(&'static str, String)>, body: Option<&T>) -> Response {
    let body = body.map(|body| serde_json::to_string(body).unwrap_or_else(|_| "{}".to_owned()));
    respond(status, headers, body)
}

fn respond(status: u16, headers: Vec<(&'static str, String)>, body: Option<String>) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut response = match body {
        Some(body) => (status, [(header::CONTENT_TYPE, "application/json")], body).into_response(),
        None => status.into_response(),
    };
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(name), HeaderValue::try_from(value)) {
            response.headers_mut().insert(name, value);
        }
    }
    response
}
