//! Compile endpoint rule documents into portable expression and rule nodes.
use crate::ir::{
    EndpointCondition, EndpointExpression, EndpointFunction, EndpointParameter,
    EndpointParameterKind, EndpointResult, EndpointRule, EndpointRuleSet, EndpointTemplatePart,
};
use crate::lower_extensions::{optional_string, parse_member_path, string, template_parts};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn parse(value: &Value) -> Result<EndpointRuleSet, String> {
    let version = string(value, "version")?;
    if !matches!(version.as_str(), "1.0" | "1.1") {
        return Err(format!("unsupported endpoint rules version {version}"));
    }
    let values = value
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or("parameters must be an object")?;
    let mut parameters = BTreeMap::new();
    let mut names = BTreeSet::new();
    for (name, value) in values {
        if !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            || !name.bytes().all(|c| c.is_ascii_alphanumeric())
            || !names.insert(name.to_ascii_lowercase())
        {
            return Err(format!(
                "invalid or case-insensitively duplicated endpoint parameter {name}"
            ));
        }
        let kind = match string(value, "type")?.as_str() {
            "String" | "string" => EndpointParameterKind::String,
            "Boolean" | "boolean" => EndpointParameterKind::Boolean,
            "StringArray" | "stringArray" => EndpointParameterKind::StringArray,
            _ => return Err(format!("unsupported parameter type for {name}")),
        };
        let required = value
            .get("required")
            .map(|v| v.as_bool().ok_or("required must be boolean"))
            .transpose()?
            .unwrap_or(false);
        let default = value.get("default").cloned();
        if let Some(default) = &default {
            let correct = match kind {
                EndpointParameterKind::String => default.is_string(),
                EndpointParameterKind::Boolean => default.is_boolean(),
                EndpointParameterKind::StringArray => default
                    .as_array()
                    .is_some_and(|v| v.iter().all(Value::is_string)),
            };
            if !required || !correct {
                return Err(format!(
                    "parameter {name} default requires required=true and a matching type"
                ));
            }
        }
        parameters.insert(
            name.clone(),
            EndpointParameter {
                kind,
                required,
                default,
                built_in: optional_string(value, "builtIn")?,
                documentation: optional_string(value, "documentation")?,
            },
        );
    }
    Ok(EndpointRuleSet {
        version,
        parameters,
        rules: rules(value)?,
    })
}

fn rules(value: &Value) -> Result<Vec<EndpointRule>, String> {
    let rules = value
        .get("rules")
        .and_then(Value::as_array)
        .ok_or("rules must be an array")?;
    if rules.is_empty() {
        return Err("rules must not be empty".into());
    }
    rules.iter().map(rule).collect()
}

fn rule(value: &Value) -> Result<EndpointRule, String> {
    let conditions = value
        .get("conditions")
        .and_then(Value::as_array)
        .ok_or("conditions must be an array")?
        .iter()
        .map(|condition| {
            let (function, arguments) = function(condition)?;
            Ok(EndpointCondition {
                function,
                arguments,
                assign: optional_string(condition, "assign")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let result = match string(value, "type")?.as_str() {
        "tree" => EndpointResult::Tree {
            rules: rules(value)?,
        },
        "error" => EndpointResult::Error {
            message: expression(value.get("error").ok_or("error rule needs error")?)?,
        },
        "endpoint" => {
            let endpoint = value
                .get("endpoint")
                .ok_or("endpoint rule needs endpoint")?;
            let url = expression(endpoint.get("url").ok_or("endpoint needs url")?)?;
            let mut headers = BTreeMap::new();
            if let Some(values) = endpoint.get("headers") {
                for (name, values) in values
                    .as_object()
                    .ok_or("endpoint headers must be an object")?
                {
                    let values = values
                        .as_array()
                        .ok_or("endpoint header value must be an array")?
                        .iter()
                        .map(expression)
                        .collect::<Result<_, _>>()?;
                    headers.insert(name.clone(), values);
                }
            }
            let mut properties = BTreeMap::new();
            if let Some(values) = endpoint.get("properties") {
                for (name, value) in values
                    .as_object()
                    .ok_or("endpoint properties must be an object")?
                {
                    properties.insert(name.clone(), expression(value)?);
                }
            }
            EndpointResult::Endpoint {
                url,
                headers,
                properties,
            }
        }
        other => return Err(format!("unknown endpoint rule type {other}")),
    };
    Ok(EndpointRule { conditions, result })
}

fn expression(value: &Value) -> Result<EndpointExpression, String> {
    Ok(match value {
        Value::String(value) => {
            let parts = template_parts(value)?
                .into_iter()
                .map(|(reference, value)| {
                    if !reference {
                        return Ok(EndpointTemplatePart::Literal { value });
                    }
                    let (name, path) = value.split_once('#').unwrap_or((&value, ""));
                    let path = if path.is_empty() {
                        Vec::new()
                    } else {
                        parse_member_path(path)?
                    };
                    Ok(EndpointTemplatePart::Reference {
                        name: name.into(),
                        path,
                    })
                })
                .collect::<Result<_, String>>()?;
            EndpointExpression::String { parts }
        }
        Value::Bool(value) => EndpointExpression::Boolean { value: *value },
        Value::Number(value) => EndpointExpression::Integer {
            value: value
                .as_i64()
                .ok_or("endpoint expressions support integer numeric literals only")?,
        },
        Value::Array(values) => EndpointExpression::Array {
            values: values.iter().map(expression).collect::<Result<_, _>>()?,
        },
        Value::Object(values) => {
            if values.contains_key("ref") {
                if values.len() != 1 {
                    return Err("endpoint reference may only contain ref".into());
                }
                EndpointExpression::Reference {
                    name: string(value, "ref")?,
                }
            } else if values.contains_key("fn") {
                let (function, arguments) = function(value)?;
                EndpointExpression::Call {
                    function,
                    arguments,
                }
            } else {
                EndpointExpression::Object {
                    values: values
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), expression(v)?)))
                        .collect::<Result<_, String>>()?,
                }
            }
        }
        Value::Null => return Err("null endpoint expressions are not supported".into()),
    })
}

fn function(value: &Value) -> Result<(EndpointFunction, Vec<EndpointExpression>), String> {
    let name = string(value, "fn")?;
    let (function, arity) = match name.as_str() {
        "isSet" => (EndpointFunction::IsSet, 1),
        "not" => (EndpointFunction::Not, 1),
        "booleanEquals" => (EndpointFunction::BooleanEquals, 2),
        "stringEquals" => (EndpointFunction::StringEquals, 2),
        "getAttr" => (EndpointFunction::GetAttr, 2),
        "substring" => (EndpointFunction::Substring, 4),
        "isValidHostLabel" => (EndpointFunction::IsValidHostLabel, 2),
        "parseURL" => (EndpointFunction::ParseUrl, 1),
        "uriEncode" => (EndpointFunction::UriEncode, 1),
        _ => return Err(format!("unsupported endpoint function {name}")),
    };
    let args = value
        .get("argv")
        .and_then(Value::as_array)
        .ok_or("endpoint function argv must be an array")?;
    if args.len() != arity {
        return Err(format!(
            "endpoint function {name} expects {arity} arguments"
        ));
    }
    Ok((
        function,
        args.iter().map(expression).collect::<Result<_, _>>()?,
    ))
}
