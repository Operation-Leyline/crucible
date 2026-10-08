//! Local newline-delimited JSON-RPC. No request values or SDK errors are logged:
//! even validation errors can contain sensitive instance values.
use std::future::Future;

use crucible_wire::mcp::{ToolDescriptor, ToolRegistry};
use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

use crate::Error;

const VERSIONS: &[&str] = &["2025-03-26", "2025-06-18", "2025-11-25"];
const MAX_LINE: usize = 1024 * 1024;

pub async fn serve<F, Fut>(
    spec: &str,
    tools: &'static [ToolDescriptor],
    call: F,
) -> Result<(), Error>
where
    F: FnMut(String, Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let spec: crate::spec::Spec = serde_json::from_str(spec).map_err(Error::input)?;
    let registry = ToolRegistry::new(tools).map_err(Error::input)?;
    session(
        BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
        &spec.name,
        &spec.version,
        registry,
        call,
    )
    .await
}

async fn session<R, W, F, Fut>(
    mut reader: R,
    mut writer: W,
    name: &str,
    version: &str,
    registry: ToolRegistry,
    mut call: F,
) -> Result<(), Error>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(String, Value) -> Fut,
    Fut: Future<Output = Result<Value, Error>>,
{
    let mut initialized = false;
    let mut ready = false;
    loop {
        let mut line = Vec::new();
        // Bound memory while draining an oversized frame so the next line can
        // still be processed. Stdio has no HTTP host body limit to do this.
        let mut oversized = false;
        loop {
            let chunk = reader.fill_buf().await.map_err(Error::request)?;
            if chunk.is_empty() {
                break;
            }
            let end = chunk.iter().position(|b| *b == b'\n').map(|i| i + 1);
            let length = end.unwrap_or(chunk.len());
            if !oversized && line.len() + length <= MAX_LINE {
                line.extend_from_slice(&chunk[..length]);
            } else {
                oversized = true;
            }
            reader.consume(length);
            if end.is_some() {
                break;
            }
        }
        if line.is_empty() && !oversized {
            return Ok(());
        }
        let message = if oversized {
            Err(())
        } else {
            serde_json::from_slice::<Value>(&line).map_err(|_| ())
        };
        let response = match message {
            Err(()) => Some(error(Value::Null, -32700, "Parse error or frame too large")),
            Ok(message) => {
                let id = message.get("id").cloned().unwrap_or(Value::Null);
                if !message.is_object()
                    || message["jsonrpc"] != "2.0"
                    || !message["method"].is_string()
                    || message.get("id").is_some_and(|id| {
                        !(id.is_string() || id.as_i64().is_some() || id.as_u64().is_some())
                    })
                {
                    Some(error(Value::Null, -32600, "Invalid Request"))
                } else if message.get("id").is_none() {
                    if initialized
                        && message["method"] == "notifications/initialized"
                        && message.get("params").is_none_or(Value::is_object)
                    {
                        ready = true;
                    }
                    None
                } else if message.get("params").is_some_and(|p| !p.is_object()) {
                    Some(error(id, -32602, "Invalid params"))
                } else {
                    let params = &message["params"];
                    Some(match message["method"].as_str().expect("checked") {
                        "initialize" if !initialized => {
                            if !params["protocolVersion"].is_string()
                                || !params["capabilities"].is_object()
                                || !params["clientInfo"]["name"].is_string()
                                || !params["clientInfo"]["version"].is_string()
                            {
                                error(id, -32602, "Invalid initialize params")
                            } else {
                                initialized = true;
                                let requested =
                                    params["protocolVersion"].as_str().expect("checked");
                                let protocol = if VERSIONS.contains(&requested) {
                                    requested
                                } else {
                                    VERSIONS[VERSIONS.len() - 1]
                                };
                                result(
                                    id,
                                    json!({"protocolVersion":protocol,"serverInfo":{"name":name,"version":version},"capabilities":{"tools":{"listChanged":false}}}),
                                )
                            }
                        }
                        "initialize" => error(id, -32600, "Already initialized"),
                        "ping" => result(id, json!({})),
                        "tools/list" | "tools/call" if !ready => {
                            error(id, -32600, "Session is not initialized")
                        }
                        "tools/list" => {
                            if params.get("cursor").is_some() {
                                error(id, -32602, "Pagination is not supported")
                            } else {
                                result(id, registry.list().clone())
                            }
                        }
                        "tools/call" => {
                            let tool = params["name"].as_str().and_then(|n| registry.descriptor(n));
                            let arguments = params
                                .get("arguments")
                                .cloned()
                                .unwrap_or_else(|| json!({}));
                            if let Some(tool) = tool {
                                match registry.arguments(tool.name, arguments) {
                                    Err(_) => error(id, -32602, "Invalid tool arguments"),
                                    Ok(arguments) => match call(tool.operation.into(), arguments)
                                        .await
                                    {
                                        Ok(output) => match registry.output(tool.name, output) {
                                            Ok(output) => result(
                                                id,
                                                json!({"content":[{"type":"text","text":output.to_string()}],"structuredContent":output,"isError":false}),
                                            ),
                                            Err(_) => result(
                                                id,
                                                failure("Service returned invalid tool output"),
                                            ),
                                        },
                                        // SDK error Display can contain modeled sensitive messages.
                                        Err(_) => result(id, failure("Service call failed")),
                                    },
                                }
                            } else {
                                error(id, -32602, "Unknown tool")
                            }
                        }
                        _ => error(id, -32601, "Method not found"),
                    })
                }
            }
        };
        if let Some(response) = response {
            let mut bytes = serde_json::to_vec(&response).map_err(Error::request)?;
            bytes.push(b'\n');
            writer.write_all(&bytes).await.map_err(Error::request)?;
            writer.flush().await.map_err(Error::request)?;
        }
    }
}

fn result(id: Value, value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":value})
}
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn failure(message: &str) -> Value {
    json!({"content":[{"type":"text","text":message}],"isError":true})
}
