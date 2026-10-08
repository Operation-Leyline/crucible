use super::ToolDescriptor;
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Tool {
    descriptor: &'static ToolDescriptor,
    input: Value,
    output: Option<Value>,
    input_validator: jsonschema::Validator,
    output_validator: Option<jsonschema::Validator>,
}

/// Parsed descriptions and compiled validators, reusable by other MCP transports.
/// Dispatch uses the descriptor's operation name through the normal service pipeline.
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Tool>,
    list: Value,
}

impl ToolRegistry {
    pub fn new(descriptors: &'static [ToolDescriptor]) -> Result<Self, String> {
        let mut tools = BTreeMap::new();
        for descriptor in descriptors {
            if descriptor.name.is_empty()
                || descriptor.name.len() > 128
                || !descriptor
                    .name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
            {
                return Err(format!("invalid MCP tool name {:?}", descriptor.name));
            }
            let input: Value =
                serde_json::from_str(descriptor.input_schema).map_err(|e| e.to_string())?;
            if input["type"] != "object" {
                return Err("MCP inputSchema must have type object".into());
            }
            let output: Option<Value> = descriptor
                .output_schema
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| e.to_string())?;
            if output.as_ref().is_some_and(|s| s["type"] != "object") {
                return Err("MCP outputSchema must have type object".into());
            }
            let input_validator = validator(&input).map_err(|e| e.to_string())?;
            let output_validator = output
                .as_ref()
                .map(validator)
                .transpose()
                .map_err(|e| e.to_string())?;
            let tool = Tool {
                descriptor,
                input,
                output,
                input_validator,
                output_validator,
            };
            if tools.insert(descriptor.name, tool).is_some() {
                return Err(format!("duplicate MCP tool {}", descriptor.name));
            }
        }
        let list = json!({"tools": tools.values().map(|tool| {
            let d = tool.descriptor;
            let mut value = json!({"name":d.name, "description":d.description, "inputSchema":tool.input,
                "annotations":{"readOnlyHint":d.read_only,"idempotentHint":d.idempotent,"destructiveHint":d.destructive}});
            if let Some(output) = &tool.output { value["outputSchema"] = output.clone(); }
            value
        }).collect::<Vec<_>>()});
        Ok(Self { tools, list })
    }

    pub fn list(&self) -> &Value {
        &self.list
    }
    pub fn descriptor(&self, name: &str) -> Option<&'static ToolDescriptor> {
        self.tools.get(name).map(|t| t.descriptor)
    }
    pub fn arguments(&self, name: &str, arguments: Value) -> Result<Value, String> {
        let tool = self.tools.get(name).ok_or("unknown tool")?;
        tool.input_validator
            .validate(&arguments)
            .map_err(|e| e.to_string())?;
        Ok(project(&tool.input, &tool.input, arguments))
    }
    /// Publish only the External document members and enforce the advertised schema.
    pub fn output(&self, name: &str, output: Value) -> Result<Value, String> {
        let tool = self.tools.get(name).ok_or("unknown tool")?;
        let output = match &tool.output {
            Some(schema) => project(schema, schema, output),
            None => output,
        };
        if let Some(validator) = &tool.output_validator {
            validator.validate(&output).map_err(|e| e.to_string())?;
        }
        if !output.is_object() {
            return Err("structuredContent must be an object".into());
        }
        Ok(output)
    }
}

fn validator(schema: &Value) -> Result<jsonschema::Validator, String> {
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .should_validate_formats(true)
        .build(schema)
        .map_err(|e| e.to_string())
}

// Descriptors are a projection while generated server types retain internal
// members. Project JSON recursively so those members cannot enter or leave MCP.
fn project(root: &Value, schema: &Value, value: Value) -> Value {
    if let Some(reference) = schema["$ref"].as_str()
        && let Some(target) = reference.strip_prefix('#').and_then(|r| root.pointer(r))
    {
        return project(root, target, value);
    }
    if let Some(alternatives) = schema["anyOf"].as_array() {
        if value.is_null() {
            return value;
        }
        if let Some(schema) = alternatives.first() {
            return project(root, schema, value);
        }
    }
    if let Some(alternatives) = schema["oneOf"].as_array()
        && let Some(schema) = alternatives.iter().find(|s| {
            s["required"].as_array().is_some_and(|names| {
                names
                    .iter()
                    .all(|n| n.as_str().is_some_and(|n| value.get(n).is_some()))
            })
        })
    {
        return project(root, schema, value);
    }
    match value {
        Value::Object(object) => {
            if let Some(properties) = schema["properties"].as_object() {
                Value::Object(
                    object
                        .into_iter()
                        .filter_map(|(name, value)| {
                            properties
                                .get(&name)
                                .map(|s| (name, project(root, s, value)))
                        })
                        .collect(),
                )
            } else if schema["additionalProperties"].is_object() {
                Value::Object(
                    object
                        .into_iter()
                        .map(|(name, value)| {
                            (name, project(root, &schema["additionalProperties"], value))
                        })
                        .collect(),
                )
            } else {
                Value::Object(object)
            }
        }
        Value::Array(array) if schema["items"].is_object() => Value::Array(
            array
                .into_iter()
                .map(|v| project(root, &schema["items"], v))
                .collect(),
        ),
        value => value,
    }
}
