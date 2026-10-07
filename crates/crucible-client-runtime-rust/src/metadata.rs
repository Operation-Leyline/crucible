//! Generated behavioral metadata, parsed and compiled once by a static LazyLock.
use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;

use crate::endpoint::{EndpointParameters, ResolvedEndpoint};
use crate::waiter::Waiter;

#[derive(Debug)]
pub struct ServiceMetadata {
    operations: BTreeMap<String, OperationMetadata>,
}

#[derive(Debug)]
pub struct OperationMetadata {
    shapes: Arc<Value>,
    rules: Arc<Value>,
    input: Option<String>,
    output: Option<String>,
    prefix: Value,
    parameters: EndpointParameters,
    pub(crate) waiters: BTreeMap<String, Waiter>,
}

impl ServiceMetadata {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let description: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let shapes = Arc::new(description["shapes"].clone());
        let rules = Arc::new(description["endpointRules"].clone());
        let mut operations = BTreeMap::new();
        for (name, operation) in description["operations"]
            .as_object()
            .ok_or("missing operation metadata")?
        {
            let mut waiters = BTreeMap::new();
            if let Some(descriptions) = operation["waiters"].as_object() {
                for (name, value) in descriptions {
                    waiters.insert(name.clone(), Waiter::new(value)?);
                }
            }
            operations.insert(
                name.clone(),
                OperationMetadata {
                    shapes: shapes.clone(),
                    rules: rules.clone(),
                    input: operation["input"].as_str().map(str::to_owned),
                    output: operation["output"].as_str().map(str::to_owned),
                    prefix: operation["endpoint"].clone(),
                    parameters: EndpointParameters::new(&operation["endpointParameters"])?,
                    waiters,
                },
            );
        }
        Ok(Self { operations })
    }

    pub fn operation(&self, name: &str) -> &OperationMetadata {
        self.operations
            .get(name)
            .expect("generated operation exists in metadata")
    }
}

impl OperationMetadata {
    pub(crate) fn input<I: serde::Serialize>(&self, input: &I) -> Result<Value, String> {
        self.normalize(self.input.as_deref(), input)
    }
    pub(crate) fn output<O: serde::Serialize>(&self, output: &O) -> Result<Value, String> {
        self.normalize(self.output.as_deref(), output)
    }
    fn normalize<T: serde::Serialize>(
        &self,
        target: Option<&str>,
        value: &T,
    ) -> Result<Value, String> {
        let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
        match target {
            Some(target) => normalize(&self.shapes, target, None, value),
            None => Ok(value),
        }
    }
    pub(crate) fn endpoint<I: serde::Serialize>(
        &self,
        input: &I,
        config: &crate::Config,
    ) -> Result<ResolvedEndpoint, String> {
        let input = self.input(input)?;
        let mut endpoint = if self.rules.is_null() {
            ResolvedEndpoint {
                url: config.endpoint.clone(),
                headers: http::HeaderMap::new(),
                properties: Value::Null,
            }
        } else {
            self.parameters.resolve(&self.rules, &input, config)?
        };
        if !self.prefix.is_null() && !config.disable_host_prefix {
            endpoint.url = crate::endpoint::host_prefix(&endpoint.url, &self.prefix, &input)?;
        }
        Ok(endpoint)
    }
}

fn normalize(
    shapes: &Value,
    target: &str,
    format: Option<&str>,
    value: Value,
) -> Result<Value, String> {
    if value.is_null() {
        return Ok(value);
    }
    let shape = &shapes[target];
    let member = |description: &Value, value| {
        normalize(
            shapes,
            description["target"]
                .as_str()
                .ok_or("missing query member target")?,
            description["format"].as_str(),
            value,
        )
    };
    match shape["kind"].as_str() {
        Some("timestamp") => {
            let format = format
                .or_else(|| shape["format"].as_str())
                .unwrap_or("date-time");
            let text = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            let timestamp = match format {
                "date-time" => crucible_wire::time::parse_date_time(&text),
                "http-date" => crucible_wire::time::http_date::parse(&text),
                _ => crucible_wire::time::epoch_seconds::parse(&text),
            }
            .ok_or("invalid query timestamp")?;
            Ok(Value::Number(
                crucible_wire::time::epoch_seconds::format(&timestamp)
                    .parse()
                    .map_err(|_| "invalid query timestamp number")?,
            ))
        }
        Some("structure" | "union") => {
            let mut result = serde_json::Map::new();
            if let Value::Object(mut values) = value {
                if let Some(members) = shape["members"].as_object() {
                    for (name, description) in members {
                        let wire = description["jsonName"].as_str().unwrap_or(name);
                        if let Some(value) = values.remove(wire) {
                            result.insert(name.clone(), member(description, value)?);
                        }
                    }
                }
                result.extend(values);
            }
            Ok(Value::Object(result))
        }
        Some("list") => match value {
            Value::Array(values) => Ok(Value::Array(
                values
                    .into_iter()
                    .map(|value| member(&shape["member"], value))
                    .collect::<Result<_, _>>()?,
            )),
            _ => Err("query list is not an array".into()),
        },
        Some("map") => match value {
            Value::Object(values) => Ok(Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, member(&shape["value"], value)?)))
                    .collect::<Result<_, String>>()?,
            )),
            _ => Err("query map is not an object".into()),
        },
        Some("blob") if shape["streaming"] == true => Ok(Value::Null),
        _ => Ok(value),
    }
}
