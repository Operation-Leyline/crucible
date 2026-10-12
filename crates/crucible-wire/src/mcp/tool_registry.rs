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
            .map_err(|e| schema_failure(&tool.input, &e))?;
        Ok(project(&tool.input, &tool.input, arguments))
    }
    /// Publish only the External document members and enforce the advertised schema.
    pub fn output(&self, name: &str, output: Value) -> Result<Value, String> {
        let tool = self.tools.get(name).ok_or("unknown tool")?;
        let output = match &tool.output {
            Some(schema) => project(schema, schema, output),
            None => output,
        };
        // A validator is compiled exactly when there is an output schema.
        if let (Some(validator), Some(schema)) = (&tool.output_validator, &tool.output) {
            validator
                .validate(&output)
                .map_err(|e| schema_failure(schema, &e))?;
        }
        if !output.is_object() {
            return Err("structuredContent must be an object".into());
        }
        Ok(output)
    }
}

/// Where a value broke its schema and which keyword it broke. jsonschema's
/// own message quotes the value (`"hunter2" is not of type "integer"`), and
/// this text becomes a client's rejection or an internal error's logged
/// cause, where a `@sensitive` member's value must not appear. The location
/// is the instance path with every map key replaced (`[key]`): a key is
/// client data, and the schema cannot say whether it is sensitive. Declared
/// property names and list indexes come from the model and stay.
fn schema_failure(root: &Value, error: &jsonschema::ValidationError<'_>) -> String {
    let schema_path = error.schema_path.to_string();
    let keyword = schema_path.rsplit('/').next().unwrap_or_default();
    let at = public_path(root, error.instance_path.as_str());
    let at = if at.is_empty() { "/" } else { at.as_str() };
    format!("{at} does not satisfy the schema's `{keyword}`")
}

/// `pointer` (an instance path, as a JSON Pointer) walked through `root`:
/// a segment the schema declares as a property, or an index into an array
/// schema's items, is kept; anything else (an `additionalProperties` key, or
/// a segment past where the schema can be followed) becomes `[key]`.
fn public_path(root: &Value, pointer: &str) -> String {
    let mut schema = Some(root);
    let mut out = String::new();
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        let step = schema.and_then(|schema| child(root, schema, &segment, 0));
        out.push('/');
        match step {
            Some((true, next)) => {
                out.push_str(&segment.replace('~', "~0").replace('/', "~1"));
                schema = Some(next);
            }
            Some((false, next)) => {
                out.push_str("[key]");
                schema = Some(next);
            }
            None => {
                out.push_str("[key]");
                schema = None;
            }
        }
    }
    out
}

/// The schema `segment` leads to from `schema`, and whether the segment is
/// the model's (a declared property or an array index) rather than the
/// client's (a map key). `$ref` and the alternatives of `anyOf`, `oneOf` and
/// `allOf` are followed; `depth` bounds a cycle of references.
fn child<'a>(
    root: &'a Value,
    schema: &'a Value,
    segment: &str,
    depth: usize,
) -> Option<(bool, &'a Value)> {
    if depth > 64 {
        return None;
    }
    if let Some(target) = schema["$ref"]
        .as_str()
        .and_then(|reference| reference.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
    {
        return child(root, target, segment, depth + 1);
    }
    for keyword in ["anyOf", "oneOf", "allOf"] {
        if let Some(found) = schema[keyword]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|alternative| child(root, alternative, segment, depth + 1))
        {
            return Some(found);
        }
    }
    if let Some(property) = schema["properties"].get(segment) {
        return Some((true, property));
    }
    if schema["additionalProperties"].is_object() {
        return Some((false, &schema["additionalProperties"]));
    }
    if schema["items"].is_object() && segment.parse::<usize>().is_ok() {
        return Some((true, &schema["items"]));
    }
    None
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
