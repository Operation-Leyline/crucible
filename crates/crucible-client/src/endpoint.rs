//! Evaluate the portable endpoint program emitted by the Smithy compiler.
use std::collections::BTreeMap;

use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{Value, json};

use crate::query::Query;

#[derive(Debug)]
pub(crate) struct ResolvedEndpoint {
    pub(crate) url: String,
    pub(crate) headers: HeaderMap,
    pub(crate) properties: Value,
}

#[derive(Debug)]
enum Source {
    Value(Value),
    Input(String),
    Path(Query),
    Client(String),
    Builtin(String),
}

#[derive(Debug)]
pub(crate) struct EndpointParameters(BTreeMap<String, Vec<Source>>);

impl EndpointParameters {
    pub(crate) fn new(description: &Value) -> Result<Self, String> {
        let mut parameters = BTreeMap::new();
        if let Some(values) = description.as_object() {
            for (name, sources) in values {
                let mut result = Vec::new();
                for source in sources
                    .as_array()
                    .ok_or("endpoint parameter sources must be an array")?
                {
                    result.push(match string(source, "kind")? {
                        "static" | "default" => Source::Value(source["value"].clone()),
                        "inputMember" => Source::Input(string(source, "member")?.into()),
                        "inputPath" => Source::Path(Query::new(&source["path"]["expression"])?),
                        "client" => Source::Client(string(source, "name")?.into()),
                        "builtIn" => Source::Builtin(string(source, "name")?.into()),
                        other => return Err(format!("unknown endpoint parameter source {other}")),
                    });
                }
                parameters.insert(name.clone(), result);
            }
        }
        Ok(Self(parameters))
    }

    pub(crate) fn resolve(
        &self,
        description: &Value,
        input: &Value,
        config: &crate::Config,
    ) -> Result<ResolvedEndpoint, String> {
        let mut builtins = config.endpoint_builtins.clone();
        if let Some(region) = &config.region {
            builtins.insert("AWS::Region".into(), json!(region));
        }
        if !config.endpoint.is_empty() {
            builtins.insert("SDK::Endpoint".into(), json!(config.endpoint));
        }
        let mut values = serde_json::Map::new();
        for (name, parameter) in description["parameters"]
            .as_object()
            .ok_or("missing endpoint parameters")?
        {
            let mut value = Value::Null;
            for source in self.0.get(name).into_iter().flatten() {
                value = match source {
                    Source::Value(value) => value.clone(),
                    Source::Input(member) => input[member].clone(),
                    Source::Path(query) => query.search(input)?,
                    Source::Client(name) => config
                        .endpoint_parameters
                        .get(name)
                        .cloned()
                        .unwrap_or(Value::Null),
                    Source::Builtin(name) => builtins.get(name).cloned().unwrap_or(Value::Null),
                };
                if !value.is_null() {
                    break;
                }
            }
            if value.is_null() && parameter["required"] == true {
                return Err(format!("required endpoint parameter {name} is missing"));
            }
            let valid = value.is_null()
                || match parameter["kind"].as_str() {
                    Some("string") => value.is_string(),
                    Some("boolean") => value.is_boolean(),
                    Some("stringArray") => value
                        .as_array()
                        .is_some_and(|values| values.iter().all(Value::is_string)),
                    _ => false,
                };
            if !valid {
                return Err(format!("endpoint parameter {name} has the wrong type"));
            }
            values.insert(name.clone(), value);
        }
        rules(&description["rules"], &values)?.ok_or_else(|| "no endpoint rule matched".into())
    }
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing endpoint {key}"))
}

fn host_label(value: &str, subdomains: bool) -> bool {
    let valid = |label: &str| {
        !label.is_empty()
            && label.len() <= 63
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    };
    if subdomains {
        value.split('.').all(valid)
    } else {
        valid(value)
    }
}

pub(crate) fn parse_url(value: &str) -> Option<reqwest::Url> {
    if value
        .bytes()
        .any(|c| c <= 32 || c == 127 || matches!(c, b'?' | b'#' | b'\\'))
        || !(value.starts_with("http://") || value.starts_with("https://"))
    {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if !host.starts_with('[') && !host_label(host.trim_end_matches('.'), true) {
        return None;
    }
    Some(url)
}

pub(crate) fn host_prefix(
    endpoint: &str,
    description: &Value,
    input: &Value,
) -> Result<String, String> {
    let mut prefix = String::new();
    for part in description["parts"]
        .as_array()
        .ok_or("missing endpoint prefix parts")?
    {
        if part["kind"] == "literal" {
            prefix.push_str(string(part, "value")?);
        } else {
            let member = string(part, "member")?;
            let value = input[member]
                .as_str()
                .filter(|value| host_label(value, false))
                .ok_or_else(|| format!("invalid endpoint host label {member}"))?;
            prefix.push_str(value);
        }
    }
    let mut url = parse_url(endpoint).ok_or("invalid endpoint HTTP(S) URL")?;
    prefix.push_str(url.host_str().ok_or("missing endpoint host")?);
    if prefix.len() > 253 || !host_label(&prefix, true) {
        return Err("invalid endpoint hostname after prefix expansion".into());
    }
    url.set_host(Some(&prefix)).map_err(|e| e.to_string())?;
    Ok(url.to_string())
}

fn attribute(mut value: Value, path: &Value) -> Value {
    if let Some(steps) = path.as_array() {
        for step in steps {
            value = if step["kind"] == "member" {
                value
                    .get(step["name"].as_str().unwrap_or(""))
                    .cloned()
                    .unwrap_or(Value::Null)
            } else {
                at(&value, step["index"].as_i64().unwrap_or(0))
            };
        }
    } else if let Some(path) = path.as_str() {
        // Attribute paths have already been validated by the compiler.
        for part in path.split('.') {
            let mut pieces = part.split('[');
            if let Some(name) = pieces.next().filter(|name| !name.is_empty()) {
                value = value.get(name).cloned().unwrap_or(Value::Null);
            }
            for index in pieces {
                value = index
                    .trim_end_matches(']')
                    .parse()
                    .ok()
                    .map(|index| at(&value, index))
                    .unwrap_or(Value::Null);
            }
        }
    }
    value
}
fn at(value: &Value, index: i64) -> Value {
    value
        .as_array()
        .and_then(|values| {
            let index = if index < 0 {
                i64::try_from(values.len()).ok()?.checked_add(index)?
            } else {
                index
            };
            values.get(usize::try_from(index).ok()?)
        })
        .cloned()
        .unwrap_or(Value::Null)
}

fn invoke(name: &str, args: &[Value]) -> Result<Value, String> {
    let a = args.first().unwrap_or(&Value::Null);
    let b = args.get(1).unwrap_or(&Value::Null);
    Ok(match name {
        "isSet" => json!(!a.is_null()),
        "not" => json!(!a.as_bool().ok_or("endpoint not expects boolean")?),
        "booleanEquals" => json!(a.is_boolean() && b.is_boolean() && a == b),
        "stringEquals" => json!(a.is_string() && b.is_string() && a == b),
        "getAttr" => attribute(a.clone(), b),
        "isValidHostLabel" => json!(a.as_str().is_some_and(|value| host_label(value, b == true))),
        "uriEncode" => {
            let mut out = String::new();
            super::transport::encode(
                &mut out,
                a.as_str().ok_or("endpoint uriEncode expects string")?,
            );
            json!(out)
        }
        "substring" => match (a.as_str(), b.as_u64(), args.get(2).and_then(Value::as_u64)) {
            (Some(text), Some(start), Some(stop))
                if text.is_ascii() && start <= stop && stop <= text.len() as u64 =>
            {
                let (start, stop) = (start as usize, stop as usize);
                json!(if args.get(3) == Some(&Value::Bool(true)) {
                    &text[text.len() - stop..text.len() - start]
                } else {
                    &text[start..stop]
                })
            }
            _ => Value::Null,
        },
        "parseUrl" => match a.as_str().and_then(parse_url) {
            Some(url) => {
                let path = url.path();
                let mut authority = url.host_str().unwrap_or_default().to_owned();
                if let Some(port) = url.port() {
                    authority.push_str(&format!(":{port}"));
                }
                json!({"scheme":url.scheme(),"authority":authority,"path":path,"normalizedPath":if path.ends_with('/') {path.to_owned()} else {format!("{path}/")},"isIp":url.host_str().is_some_and(|host|host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok())})
            }
            None => Value::Null,
        },
        _ => return Err(format!("unknown endpoint function {name}")),
    })
}

fn expression(node: &Value, values: &serde_json::Map<String, Value>) -> Result<Value, String> {
    let arguments = |nodes: &Value| {
        nodes
            .as_array()
            .ok_or_else(|| "missing endpoint expression array".to_owned())?
            .iter()
            .map(|node| expression(node, values))
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(match string(node, "kind")? {
        "reference" => values
            .get(string(node, "name")?)
            .cloned()
            .unwrap_or(Value::Null),
        "boolean" | "integer" => node["value"].clone(),
        "array" => Value::Array(arguments(&node["values"])?),
        "object" => Value::Object(
            node["values"]
                .as_object()
                .ok_or("missing endpoint expression object")?
                .iter()
                .map(|(name, node)| Ok((name.clone(), expression(node, values)?)))
                .collect::<Result<_, String>>()?,
        ),
        "call" => invoke(string(node, "function")?, &arguments(&node["arguments"])?)?,
        "string" => {
            let mut text = String::new();
            for part in node["parts"]
                .as_array()
                .ok_or("missing endpoint string parts")?
            {
                let value = if part["kind"] == "literal" {
                    part["value"].clone()
                } else {
                    attribute(
                        values
                            .get(string(part, "name")?)
                            .cloned()
                            .unwrap_or(Value::Null),
                        &part["path"],
                    )
                };
                match value {
                    Value::String(value) => text.push_str(&value),
                    Value::Number(_) | Value::Bool(_) => text.push_str(&value.to_string()),
                    _ => {
                        return Err(
                            "endpoint template references missing or non-scalar value".into()
                        );
                    }
                }
            }
            json!(text)
        }
        other => return Err(format!("unknown endpoint expression {other}")),
    })
}

fn rules(
    nodes: &Value,
    values: &serde_json::Map<String, Value>,
) -> Result<Option<ResolvedEndpoint>, String> {
    for node in nodes.as_array().ok_or("missing endpoint rules")? {
        let mut scope = values.clone();
        let mut matches = true;
        for condition in node["conditions"]
            .as_array()
            .ok_or("missing endpoint conditions")?
        {
            let args = condition["arguments"]
                .as_array()
                .ok_or("missing endpoint arguments")?
                .iter()
                .map(|node| expression(node, &scope))
                .collect::<Result<Vec<_>, _>>()?;
            let value = invoke(string(condition, "function")?, &args)?;
            if value.is_null() || value == false {
                matches = false;
                break;
            }
            if let Some(name) = condition["assign"].as_str() {
                scope.insert(name.into(), value);
            }
        }
        if !matches {
            continue;
        }
        let result = &node["result"];
        match string(result, "kind")? {
            "tree" => {
                if let Some(endpoint) = rules(&result["rules"], &scope)? {
                    return Ok(Some(endpoint));
                }
            }
            "error" => {
                return Err(expression(&result["message"], &scope)?
                    .as_str()
                    .unwrap_or("endpoint rule failed")
                    .into());
            }
            "endpoint" => {
                let value = expression(&result["url"], &scope)?;
                let url = value
                    .as_str()
                    .filter(|url| parse_url(url).is_some())
                    .ok_or("endpoint rule returned invalid HTTP(S) URL")?
                    .to_owned();
                let mut headers = HeaderMap::new();
                for (name, values) in result["headers"]
                    .as_object()
                    .ok_or("missing endpoint headers")?
                {
                    for value in values.as_array().ok_or("endpoint headers must be arrays")? {
                        let value = expression(value, &scope)?;
                        headers.append(
                            HeaderName::from_bytes(name.as_bytes()).map_err(|e| e.to_string())?,
                            HeaderValue::from_str(
                                value.as_str().ok_or("endpoint headers must be strings")?,
                            )
                            .map_err(|e| e.to_string())?,
                        );
                    }
                }
                let properties = Value::Object(
                    result["properties"]
                        .as_object()
                        .ok_or("missing endpoint properties")?
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), expression(value, &scope)?)))
                        .collect::<Result<_, String>>()?,
                );
                return Ok(Some(ResolvedEndpoint {
                    url,
                    headers,
                    properties,
                }));
            }
            other => return Err(format!("unknown endpoint result {other}")),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_endpoint_functions_preserve_false_and_validate_urls() {
        assert_eq!(invoke("isSet", &[json!(false)]).unwrap(), true);
        assert_eq!(
            invoke("getAttr", &[json!({"x":[1,2]}), json!("x[-1]")]).unwrap(),
            2
        );
        assert_eq!(
            invoke(
                "substring",
                &[json!("abcdef"), json!(1), json!(3), json!(true)]
            )
            .unwrap(),
            "de"
        );
        assert!(
            invoke("substring", &[json!("\u{e9}"), json!(0), json!(1)])
                .unwrap()
                .is_null()
        );
        assert_eq!(
            invoke("uriEncode", &[json!("a b/+")]).unwrap(),
            "a%20b%2F%2B"
        );
        let url = invoke("parseUrl", &[json!("https://[::1]:8443/base")]).unwrap();
        assert_eq!(url["authority"], "[::1]:8443");
        assert_eq!(url["normalizedPath"], "/base/");
        assert_eq!(url["isIp"], true);
        for value in [
            "file:///path",
            "https://user:password@example.test",
            "https://example.test?q=x",
            "https://example.test#fragment",
            "https://evil\n.example",
        ] {
            assert!(parse_url(value).is_none(), "{value}");
        }
    }
}
