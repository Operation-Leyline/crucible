//! The `http` pseudo-target of `crucible.sdks/1`: each operation's request
//! as it travels, with every header a caller without an SDK must send
//! (`Content-Type`, `Crucible-Version`, the bearer token, and what the SDKs'
//! default middleware adds), and a `curl` command sending it.
//!
//! The request is the one `describe.json` shows for the operation: its first
//! example with an input, else its generated sample, built by the same code
//! (`crucible_codegen_common::describe`), so the reference and the snippet
//! never disagree about a label, a query parameter or a body. The request
//! leaves out the idempotency token, as the SDK snippets do: a fixed token
//! in a copied command would replay one logical request forever.
//!
//! The command is pasted into a terminal, so model text reaches it only as
//! printable characters: a body holding any other is sent through
//! `base64 -d`, and a header value no HTTP request may carry (a line
//! break, a NUL, another control) is left out of the command and named in
//! `omitted`. A raw `\x03` or `ESC [201~` would otherwise interrupt the
//! shell or end a bracketed paste early and run what follows.

use crucible_codegen_common::snippet::{base64_bytes, base64_text, env_var};
use crucible_ir::Audience;
use crucible_ir::ir::{Operation, Service};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The middleware whose header is the body's SHA-256, in lowercase hex.
const CONTENT_SHA256: &str = "content-sha256";

/// What a request body is, as the snippet sends it.
struct Body {
    bytes: Vec<u8>,
    /// UTF-8 text, or base64 of raw or unprintable bytes.
    text: String,
    encoding: &'static str,
}

impl Body {
    /// `bytes` as text where every character prints, else as base64.
    fn of(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(text) if text.chars().all(printable) => Self {
                bytes: text.as_bytes().to_vec(),
                text,
                encoding: "utf-8",
            },
            Ok(text) => Self::base64(text.into_bytes()),
            Err(error) => Self::base64(error.into_bytes()),
        }
    }

    fn base64(bytes: Vec<u8>) -> Self {
        Self {
            text: base64_text(&bytes),
            bytes,
            encoding: "base64",
        }
    }
}

/// A character that shows as itself in a terminal and an editor: not a
/// control (C0, DEL, C1) other than a line feed or tab, not a format
/// character (the bidirectional controls, zero-width characters, a byte
/// order mark, a soft hyphen) and not a line or paragraph separator.
fn printable(c: char) -> bool {
    if matches!(c, '\n' | '\t') {
        return true;
    }
    !(c.is_control()
        || crucible_codegen_common::bidi(c)
        || matches!(
            c,
            '\u{ad}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{2028}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        ))
}

fn body(message: &Value) -> Option<Body> {
    if let Some(payload) = message["payload"].as_object() {
        return match payload.get("value")? {
            Value::Null => None,
            Value::String(value) if payload.get("encoding") == Some(&json!("base64")) => {
                Some(Body::base64(base64_bytes(value).unwrap_or_default()))
            }
            Value::String(value) => Some(Body::of(value.as_bytes().to_vec())),
            // A structure or document payload is its JSON value, sent as
            // the whole body.
            document => Some(Body::of(
                serde_json::to_string(document)
                    .expect("a document serializes")
                    .into_bytes(),
            )),
        };
    }
    let document = message.get("body").filter(|body| !body.is_null())?;
    let text = serde_json::to_string(document).expect("a document serializes");
    Some(Body::of(text.into_bytes()))
}

/// `message` without the operation's idempotency token: the body member,
/// header or query parameter it travels in.
fn without_token(operation: &Operation, message: &Value) -> Value {
    let mut message = message.clone();
    let Some(token) = &operation.idempotency_token else {
        return message;
    };
    if let Some(body) = message["body"].as_object_mut() {
        body.remove(token);
    }
    let request = &operation.request;
    // A token bound as the whole payload goes with its body and the body's
    // media type: the request is then sent without one, as a caller who
    // leaves the token to the service does.
    if request.payload.as_ref().is_some_and(|p| &p.member == token) {
        message["payload"] = Value::Null;
        message["body"] = Value::Null;
        if let Some(headers) = message["headers"].as_object_mut() {
            headers.retain(|name, _| !name.eq_ignore_ascii_case("content-type"));
        }
    }
    if let Some(binding) = request.headers.iter().find(|b| &b.member == token)
        && let Some(headers) = message["headers"].as_object_mut()
    {
        headers.retain(|name, _| !name.eq_ignore_ascii_case(&binding.name));
    }
    if let Some(binding) = request.query.iter().find(|b| &b.member == token)
        && let Some(uri) = message["uri"].as_str()
        && let Some((path, query)) = uri.split_once('?')
    {
        let kept: Vec<&str> = query
            .split('&')
            .filter(|pair| pair.split('=').next() != Some(binding.name.as_str()))
            .collect();
        message["uri"] = json!(if kept.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{}", kept.join("&"))
        });
    }
    message
}

/// `text` inside single quotes for a POSIX shell, where nothing expands.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Why a header value cannot be sent as written, if it cannot: HTTP field
/// values hold no control but a tab (RFC 9110, section 5.5), and a
/// pasted command holds no unprintable character.
fn unsendable(value: &str) -> Option<&'static str> {
    if value.contains(['\r', '\n']) {
        Some("holds a line break")
    } else if value.contains('\0') {
        Some("holds a NUL")
    } else if value.chars().any(|c| c != '\t' && !printable(c)) {
        Some("holds a control or format character")
    } else {
        None
    }
}

/// The `http` object: `{endpointEnv, tokenEnv, operations}`, operations by
/// absolute ID. `middleware` is each default middleware's `(name, header)`.
pub(super) fn document(
    service: &Service,
    audience: Audience,
    middleware: &[(String, String)],
) -> Result<Value, String> {
    // As every client sees it: a sender-signed operation is called by a
    // third party, never by a caller of the API, so it has no request here.
    let service = crucible_ir::client_view(
        &crucible_ir::project(service, audience).map_err(|d| d.to_string())?,
    );
    let description =
        crucible_codegen_common::describe::describe(&service).map_err(|d| d.to_string())?;
    let endpoint = env_var(&service, "ENDPOINT");
    let token = env_var(&service, "TOKEN");
    let mut operations = serde_json::Map::new();
    for operation in &service.operations {
        let id = operation.id.to_string();
        let Some(described) = description.operations.get(&id) else {
            continue;
        };
        let example = described["examples"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|example| !example["input"].is_null());
        // An example's token, like a sample's, is left out: a copied
        // command repeated with it would replay one logical request.
        let (message, title) = match example {
            Some(example) => (
                without_token(operation, &example["input"]),
                example["title"].clone(),
            ),
            None => (
                without_token(operation, &described["sample"]["input"]),
                Value::Null,
            ),
        };
        let method = message["method"].as_str().unwrap_or(&operation.method);
        let uri = message["uri"].as_str().unwrap_or_default();
        let body = body(&message);
        let mut headers: Vec<(String, String)> = message["headers"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_owned()))
            .collect();
        let bearer = !operation.public && operation.auth.iter().any(|s| service.is_bearer(s));
        if bearer {
            headers.push(("Authorization".into(), "Bearer <token>".into()));
        }
        if let Some(version) = &service.api_version {
            headers.push(("Crucible-Version".into(), version.clone()));
        }
        for (name, header) in middleware {
            if name == CONTENT_SHA256 {
                // The client digests an empty POST, PUT or PATCH body too,
                // and skips other bodyless requests.
                let empty = matches!(method, "POST" | "PUT" | "PATCH");
                if let Some(bytes) = body
                    .as_ref()
                    .map(|body| body.bytes.as_slice())
                    .or(empty.then_some(&[][..]))
                {
                    let digest: String = Sha256::digest(bytes)
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    headers.push((header.clone(), digest));
                }
            } else {
                headers.push((header.clone(), format!("<{name}>")));
            }
        }
        headers.sort_by_key(|(name, _)| name.to_ascii_lowercase());

        let mut omitted = Vec::new();
        let mut command = String::from("curl");
        if method != "GET" || body.is_some() {
            command.push_str(&format!(" -X {method}"));
        }
        // The endpoint is the caller's variable, expanded in double quotes;
        // the path and query are literal, in single quotes, where neither
        // `$` nor `!` expands.
        command.push_str(&format!(" \"${endpoint}\"{}", quoted(uri)));
        let mut prefix = String::new();
        if let Some(body) = &body
            && body.encoding == "base64"
        {
            prefix = format!("printf '%s' {} | base64 -d | ", quoted(&body.text));
        }
        let mut curl = vec![format!("{prefix}{command}")];
        for (name, value) in &headers {
            if name == "Authorization" && bearer {
                curl.push(format!("-H \"Authorization: Bearer ${token}\""));
            } else if let Some(reason) = unsendable(value) {
                omitted.push(json!({"name": name, "reason": reason}));
            } else {
                curl.push(format!("-H {}", quoted(&format!("{name}: {value}"))));
            }
        }
        if let Some(body) = &body {
            curl.push(match body.encoding {
                "base64" => "--data-binary @-".to_owned(),
                _ => format!("--data-binary {}", quoted(&body.text)),
            });
        }
        operations.insert(
            id,
            json!({
                "method": method,
                "uri": uri,
                "headers": headers
                    .iter()
                    .map(|(name, value)| json!({"name": name, "value": value}))
                    .collect::<Vec<_>>(),
                "body": body.as_ref().map(|body| body.text.clone()),
                "bodyEncoding": body.as_ref().map(|body| body.encoding),
                "example": title,
                "omitted": omitted,
                "snippet": format!("{}\n", curl.join(" \\\n  ")),
            }),
        );
    }
    Ok(json!({
        "endpointEnv": endpoint,
        "tokenEnv": service.auth.iter().any(|s| service.is_bearer(s)).then_some(token),
        "operations": operations,
    }))
}
