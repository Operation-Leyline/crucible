//! The `http` pseudo-target of `crucible.sdks/1`: each operation's request
//! as it travels, with every header a caller without an SDK must send
//! (`Content-Type`, `Crucible-Version`, the bearer token, and what the SDKs'
//! default middleware adds), and a `curl` command sending it.
//!
//! The request is the one `describe.json` shows for the operation: its first
//! example with an input, else its generated sample, built by the same code
//! (`crucible_codegen_common::describe`), so the reference and the snippet
//! never disagree about a label, a query parameter or a body.

use crucible_codegen_common::snippet::{base64_bytes, env_var};
use crucible_ir::Audience;
use crucible_ir::ir::Service;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The middleware whose header is the body's SHA-256, in lowercase hex.
const CONTENT_SHA256: &str = "content-sha256";

/// What a request body is, as the snippet sends it.
struct Body {
    bytes: Vec<u8>,
    /// The JSON contract's form: UTF-8 text, or base64 for raw bytes.
    text: String,
    encoding: &'static str,
}

fn body(message: &Value) -> Option<Body> {
    if let Some(payload) = message["payload"].as_object() {
        let value = payload.get("value")?.as_str()?;
        return Some(if payload.get("encoding") == Some(&json!("base64")) {
            Body {
                bytes: base64_bytes(value).unwrap_or_default(),
                text: value.to_owned(),
                encoding: "base64",
            }
        } else {
            Body {
                bytes: value.as_bytes().to_vec(),
                text: value.to_owned(),
                encoding: "utf-8",
            }
        });
    }
    let document = message.get("body").filter(|body| !body.is_null())?;
    let text = serde_json::to_string(document).expect("a document serializes");
    Some(Body {
        bytes: text.as_bytes().to_vec(),
        text,
        encoding: "utf-8",
    })
}

/// `text` inside single quotes for a POSIX shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `text` as it stands inside double quotes, where `$VARIABLE` would
/// otherwise expand.
fn in_double_quotes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '"' | '\\' | '`' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The `http` object: `{endpointEnv, tokenEnv, operations}`, operations by
/// absolute ID. `middleware` is each default middleware's `(name, header)`.
pub(super) fn document(
    service: &Service,
    audience: Audience,
    middleware: &[(String, String)],
) -> Result<Value, String> {
    let service = crucible_ir::project(service, audience).map_err(|d| d.to_string())?;
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
        let (message, title) = match example {
            Some(example) => (&example["input"], example["title"].clone()),
            None => (&described["sample"]["input"], Value::Null),
        };
        let method = message["method"].as_str().unwrap_or(&operation.method);
        let uri = message["uri"].as_str().unwrap_or_default();
        let body = body(message);
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

        let mut curl = Vec::new();
        let mut command = String::from("curl");
        if method != "GET" || body.is_some() {
            command.push_str(&format!(" -X {method}"));
        }
        // The endpoint and token are the caller's variables; the rest is
        // literal.
        command.push_str(&format!(" \"${endpoint}{}\"", in_double_quotes(uri)));
        let mut prefix = String::new();
        if let Some(body) = &body
            && body.encoding == "base64"
        {
            prefix = format!("printf '%s' {} | base64 -d | ", quoted(&body.text));
        }
        curl.push(format!("{prefix}{command}"));
        for (name, value) in &headers {
            curl.push(if name == "Authorization" && bearer {
                format!("-H \"Authorization: Bearer ${token}\"")
            } else {
                format!("-H {}", quoted(&format!("{name}: {value}")))
            });
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
