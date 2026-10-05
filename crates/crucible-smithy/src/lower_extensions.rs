use std::collections::BTreeMap;

use crate::ast::{self, Kind, Traits};
use crate::ir::{
    ApiKeyLocation, AuthScheme, EndpointHostPrefix, EndpointRuleSet, HostPrefixPart, PathStep,
    Waiter, WaiterAcceptor, WaiterComparator, WaiterMatcher, WaiterPath, WaiterState,
};
use crate::lower::Lowerer;
use crate::{Feature, ShapeId};
use serde_json::Value;

impl Lowerer<'_> {
    pub(crate) fn resolve_endpoint_parameters(
        &mut self,
        service_id: &ShapeId,
        service: &ast::Shape,
        operations: &mut [crate::ir::Operation],
        rules: Option<&EndpointRuleSet>,
    ) {
        use crate::ir::EndpointParameterSource as Source;
        let client_id = ShapeId::new("smithy.rules", "clientContextParams");
        let clients = service.traits.get(&client_id).and_then(Value::as_object);
        if service.traits.contains_key(&client_id) {
            if clients.is_none() {
                self.diagnostics.push(
                    service_id,
                    "@smithy.rules#clientContextParams must be an object",
                );
            }
            self.require(service_id, Feature::EndpointRules, client_id);
        }
        if let Some(clients) = clients {
            for (name, value) in clients {
                let parameter = rules.and_then(|rules| rules.parameters.get(name));
                if parameter.is_none() {
                    self.diagnostics.push(
                        service_id,
                        format!(
                            "client endpoint parameter {name} is not defined in endpointRuleSet"
                        ),
                    );
                }
                if let Some(parameter) = parameter {
                    let expected = match parameter.kind {
                        crate::ir::EndpointParameterKind::String => "string",
                        crate::ir::EndpointParameterKind::Boolean => "boolean",
                        crate::ir::EndpointParameterKind::StringArray => "stringArray",
                    };
                    if value.get("type").and_then(Value::as_str) != Some(expected) {
                        self.diagnostics.push(
                            service_id,
                            format!("client endpoint parameter {name} must have type {expected}"),
                        );
                    }
                }
            }
        }
        for operation in operations {
            let mut sources: BTreeMap<String, Vec<Source>> = BTreeMap::new();
            let op = &self.model[&operation.id];
            let static_id = ShapeId::new("smithy.rules", "staticContextParams");
            if let Some(values) = op.traits.get(&static_id) {
                self.require(&operation.id, Feature::EndpointRules, static_id);
                if let Some(values) = values.as_object() {
                    for (name, value) in values {
                        if let Some(value) = value.get("value") {
                            sources
                                .entry(name.clone())
                                .or_default()
                                .push(Source::Static {
                                    value: value.clone(),
                                });
                        } else {
                            self.diagnostics.push(
                                &operation.id,
                                format!("static endpoint parameter {name} requires value"),
                            );
                        }
                    }
                } else {
                    self.diagnostics
                        .push(&operation.id, "staticContextParams must be a parameter map");
                }
            }
            if let Some(input) = operation
                .input
                .as_ref()
                .and_then(|input| self.model.get(input))
            {
                for (member, definition) in &input.members {
                    let trait_id = ShapeId::new("smithy.rules", "contextParam");
                    if let Some(value) = definition.traits.get(&trait_id) {
                        let member_id = operation.input.as_ref().unwrap().with_member(member);
                        self.require(&member_id, Feature::EndpointRules, trait_id);
                        match string(value, "name") {
                            Ok(name) => {
                                sources.entry(name).or_default().push(Source::InputMember {
                                    member: member.clone(),
                                })
                            }
                            Err(message) => self.diagnostics.push(&member_id, message),
                        }
                    }
                }
            }
            let path_id = ShapeId::new("smithy.rules", "operationContextParams");
            if let Some(values) = op.traits.get(&path_id) {
                self.require(&operation.id, Feature::EndpointRules, path_id);
                if let Some(values) = values.as_object() {
                    for (name, value) in values {
                        match string(value, "path").and_then(|path| parse_path(&path)) {
                            Ok(path) => sources
                                .entry(name.clone())
                                .or_default()
                                .push(Source::InputPath { path }),
                            Err(message) => self.diagnostics.push(&operation.id, message),
                        }
                    }
                } else {
                    self.diagnostics.push(
                        &operation.id,
                        "operationContextParams must be a parameter map",
                    );
                }
            }
            if let Some(clients) = clients {
                for name in clients.keys() {
                    sources
                        .entry(name.clone())
                        .or_default()
                        .push(Source::Client { name: name.clone() });
                }
            }
            for (name, entries) in &sources {
                let parameter = rules.and_then(|rules| rules.parameters.get(name));
                if let Some(parameter) = parameter {
                    for source in entries {
                        if let Source::Static { value } = source {
                            if !parameter_value_matches(parameter.kind, value) {
                                self.diagnostics.push(
                                    &operation.id,
                                    format!("static endpoint parameter {name} has the wrong type"),
                                );
                            }
                        } else if let Source::InputMember { member } = source {
                            let target = operation
                                .input
                                .as_ref()
                                .and_then(|id| self.model.get(id))
                                .and_then(|input| input.members.get(member))
                                .and_then(|member| self.model.get(&member.target));
                            let correct = target.is_some_and(|target| match parameter.kind {
                                crate::ir::EndpointParameterKind::String => {
                                    matches!(target.kind, Kind::String | Kind::Enum)
                                }
                                crate::ir::EndpointParameterKind::Boolean => {
                                    target.kind == Kind::Boolean
                                }
                                crate::ir::EndpointParameterKind::StringArray => {
                                    target.kind == Kind::List
                                        && !target
                                            .traits
                                            .contains_key(&ShapeId::new("smithy.api", "sparse"))
                                        && target
                                            .member
                                            .as_ref()
                                            .and_then(|member| self.model.get(&member.target))
                                            .is_some_and(|target| {
                                                matches!(target.kind, Kind::String | Kind::Enum)
                                            })
                                }
                            });
                            if !correct {
                                self.diagnostics.push(&operation.input.as_ref().unwrap().with_member(member),format!("endpoint parameter {name} has a different type from its input member"));
                            }
                        }
                    }
                } else {
                    self.diagnostics.push(
                        &operation.id,
                        format!("endpoint parameter {name} is not defined in endpointRuleSet"),
                    );
                }
            }
            if let Some(rules) = rules {
                for (name, parameter) in &rules.parameters {
                    let entries = sources.entry(name.clone()).or_default();
                    if let Some(name) = &parameter.built_in {
                        entries.push(Source::BuiltIn { name: name.clone() });
                    }
                    if let Some(value) = &parameter.default {
                        entries.push(Source::Default {
                            value: value.clone(),
                        });
                    }
                }
            }
            operation.endpoint_parameters = sources;
        }
    }
    pub(crate) fn auth_configs(
        &mut self,
        id: &ShapeId,
        shape: &ast::Shape,
        schemes: &[ShapeId],
    ) -> BTreeMap<ShapeId, AuthScheme> {
        let mut result = BTreeMap::new();
        for scheme in schemes {
            let parsed = match (scheme.namespace(), scheme.name()) {
                ("smithy.api", "httpBearerAuth") => Ok(AuthScheme::Bearer),
                ("smithy.api", "httpBasicAuth") => {
                    self.require(id, Feature::BasicAuth, scheme.clone());
                    Ok(AuthScheme::Basic)
                }
                ("smithy.api", "httpApiKeyAuth") => {
                    self.require(id, Feature::ApiKeyAuth, scheme.clone());
                    api_key(&shape.traits[scheme])
                }
                ("aws.auth", "sigv4") => {
                    self.require(id, Feature::SigV4, scheme.clone());
                    string(&shape.traits[scheme], "name")
                        .map(|signing_name| AuthScheme::SigV4 { signing_name })
                }
                _ => Err(format!("auth scheme {scheme} is not supported")),
            };
            match parsed {
                Ok(config) => {
                    result.insert(scheme.clone(), config);
                }
                Err(message) => self.diagnostics.push(id, message),
            }
        }
        result
    }

    pub(crate) fn endpoint_host_prefix(
        &mut self,
        id: &ShapeId,
        shape: &ast::Shape,
        input: Option<&ShapeId>,
    ) -> Option<EndpointHostPrefix> {
        let trait_id = ShapeId::new("smithy.api", "endpoint");
        let value = shape.traits.get(&trait_id)?;
        self.require(id, Feature::EndpointHostPrefix, trait_id);
        let parsed = string(value, "hostPrefix").and_then(|prefix| {
            let parts = template_parts(&prefix)?;
            let mut result = Vec::new();
            for (label, text) in parts {
                if label {
                    let member = input.and_then(|input|self.model.get(input)).and_then(|input|input.members.get(&text)).ok_or_else(||format!("endpoint host label {text:?} has no input member"))?;
                    if !member.traits.contains_key(&ShapeId::new("smithy.api","hostLabel")) || !member.traits.contains_key(&ShapeId::new("smithy.api","required")) || !self.model.get(&member.target).is_some_and(|s|matches!(s.kind,Kind::String|Kind::Enum)) {
                        return Err(format!("endpoint host label {text:?} must reference a required @hostLabel string member"));
                    }
                    result.push(HostPrefixPart::Label { member: text });
                } else { result.push(HostPrefixPart::Literal { value: text }); }
            }
            Ok(EndpointHostPrefix { parts: result })
        });
        match parsed {
            Ok(prefix) => Some(prefix),
            Err(message) => {
                self.diagnostics.push(id, message);
                None
            }
        }
    }

    pub(crate) fn waiters(
        &mut self,
        id: &ShapeId,
        traits: &Traits,
        has_output: bool,
    ) -> BTreeMap<String, Waiter> {
        let trait_id = ShapeId::new("smithy.waiters", "waitable");
        let Some(value) = traits.get(&trait_id) else {
            return BTreeMap::new();
        };
        self.require(id, Feature::Waiters, trait_id);
        let Some(values) = value.as_object() else {
            self.diagnostics
                .push(id, "@smithy.waiters#waitable must be a waiter map");
            return BTreeMap::new();
        };
        let mut result = BTreeMap::new();
        for (name, value) in values {
            let parsed = waiter(value, has_output).and_then(|waiter| {
                if !name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                    || !name.bytes().all(|c| c.is_ascii_alphanumeric())
                {
                    return Err(
                        "waiter names must be UpperCamelCase alphanumeric identifiers".into(),
                    );
                }
                Ok(waiter)
            });
            match parsed {
                Ok(waiter) => {
                    result.insert(name.clone(), waiter);
                }
                Err(message) => self
                    .diagnostics
                    .push(id, format!("waiter {name}: {message}")),
            }
        }
        result
    }

    pub(crate) fn endpoint_rules(
        &mut self,
        id: &ShapeId,
        traits: &Traits,
    ) -> Option<EndpointRuleSet> {
        let trait_id = ShapeId::new("smithy.rules", "endpointRuleSet");
        let value = traits.get(&trait_id)?;
        self.require(id, Feature::EndpointRules, trait_id);
        match crate::lower_endpoints::parse(value) {
            Ok(rules) => Some(rules),
            Err(message) => {
                self.diagnostics
                    .push(id, format!("endpoint rules: {message}"));
                None
            }
        }
    }
}

fn parameter_value_matches(kind: crate::ir::EndpointParameterKind, value: &Value) -> bool {
    match kind {
        crate::ir::EndpointParameterKind::String => value.is_string(),
        crate::ir::EndpointParameterKind::Boolean => value.is_boolean(),
        crate::ir::EndpointParameterKind::StringArray => value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string)),
    }
}

fn api_key(value: &Value) -> Result<AuthScheme, String> {
    let name = string(value, "name")?;
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err("API key name must be nonempty without control characters".into());
    }
    let location = match string(value, "in")?.as_str() {
        "header" => ApiKeyLocation::Header,
        "query" => ApiKeyLocation::Query,
        _ => return Err("API key location must be header or query".into()),
    };
    let scheme = optional_string(value, "scheme")?;
    if location == ApiKeyLocation::Query && scheme.is_some() {
        return Err("API key scheme is only valid for headers".into());
    }
    if scheme
        .as_ref()
        .is_some_and(|scheme| scheme.is_empty() || scheme.chars().any(char::is_whitespace))
    {
        return Err("API key scheme must be a nonempty token".into());
    }
    Ok(AuthScheme::ApiKey {
        name,
        location,
        scheme,
    })
}

fn waiter(value: &Value, has_output: bool) -> Result<Waiter, String> {
    let min_delay = integer_default(value, "minDelay", 2)?;
    let max_delay = integer_default(value, "maxDelay", 120)?;
    if min_delay == 0 || max_delay < min_delay {
        return Err("waiter delays must satisfy 1 <= minDelay <= maxDelay".into());
    }
    let acceptors = value
        .get("acceptors")
        .and_then(Value::as_array)
        .ok_or("waiter needs acceptors")?
        .iter()
        .map(|value| acceptor(value, has_output))
        .collect::<Result<Vec<_>, _>>()?;
    if !acceptors.iter().any(|a| a.state == WaiterState::Success) {
        return Err("waiter needs a success acceptor".into());
    }
    let tags = match value.get("tags") {
        None => Vec::new(),
        Some(Value::Array(tags)) => tags
            .iter()
            .map(|t| {
                t.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "waiter tags must be strings".into())
            })
            .collect::<Result<_, String>>()?,
        Some(_) => return Err("waiter tags must be an array".into()),
    };
    Ok(Waiter {
        min_delay,
        max_delay,
        acceptors,
        documentation: optional_string(value, "documentation")?,
        deprecated: value
            .get("deprecated")
            .map(|v| v.as_bool().ok_or("waiter deprecated must be boolean"))
            .transpose()?
            .unwrap_or(false),
        tags,
    })
}

fn acceptor(value: &Value, has_output: bool) -> Result<WaiterAcceptor, String> {
    let state = match string(value, "state")?.as_str() {
        "success" => WaiterState::Success,
        "failure" => WaiterState::Failure,
        "retry" => WaiterState::Retry,
        _ => return Err("unknown waiter acceptor state".into()),
    };
    let matcher = value
        .get("matcher")
        .and_then(Value::as_object)
        .ok_or("acceptor needs matcher object")?;
    if matcher.len() != 1 {
        return Err("waiter matcher must contain exactly one variant".into());
    }
    let (kind, body) = matcher.iter().next().unwrap();
    let matcher = match kind.as_str() {
        "success" => WaiterMatcher::Success {
            expected: body.as_bool().ok_or("success matcher must be boolean")?,
        },
        "errorType" => {
            let name = body.as_str().ok_or("errorType must be a string")?;
            let name = if name.contains('#') {
                name.parse::<ShapeId>()
                    .map_err(|e| e.to_string())?
                    .name()
                    .to_owned()
            } else {
                name.to_owned()
            };
            WaiterMatcher::ErrorType { name }
        }
        "output" | "inputOutput" => {
            if kind == "output" && !has_output {
                return Err("output matcher requires an operation output".into());
            }
            let path = parse_path(&string(body, "path")?)?;
            let comparator = match string(body, "comparator")?.as_str() {
                "stringEquals" => WaiterComparator::StringEquals,
                "booleanEquals" => WaiterComparator::BooleanEquals,
                "allStringEquals" => WaiterComparator::AllStringEquals,
                "anyStringEquals" => WaiterComparator::AnyStringEquals,
                _ => return Err("unknown waiter path comparator".into()),
            };
            let expected = string(body, "expected")?;
            if comparator == WaiterComparator::BooleanEquals
                && !matches!(expected.as_str(), "true" | "false")
            {
                return Err("booleanEquals expected must be true or false".into());
            }
            if kind == "output" {
                WaiterMatcher::Output {
                    path,
                    comparator,
                    expected,
                }
            } else {
                WaiterMatcher::InputOutput {
                    path,
                    comparator,
                    expected,
                }
            }
        }
        _ => return Err(format!("unknown waiter matcher {kind}")),
    };
    let message = optional_string(value, "message")?
        .map(|text| parse_path(&text))
        .transpose()?;
    if message.is_some() && state != WaiterState::Failure {
        return Err("waiter message is only valid on failure acceptors".into());
    }
    Ok(WaiterAcceptor {
        state,
        matcher,
        message,
    })
}

pub(crate) fn parse_path(text: &str) -> Result<WaiterPath, String> {
    crate::jmespath_ir::compile(text).map(|expression| WaiterPath { expression })
}

pub(crate) fn parse_member_path(text: &str) -> Result<Vec<PathStep>, String> {
    if text == "@" {
        return Ok(Vec::new());
    }
    let mut remaining = text;
    let mut steps = Vec::new();
    while !remaining.is_empty() {
        let length = remaining
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
            .count();
        if length == 0 || remaining.as_bytes()[0].is_ascii_digit() {
            return Err(format!(
                "JMESPath expression {text:?} uses an unsupported operator; supported paths currently use member names and indexes"
            ));
        }
        steps.push(PathStep::Member {
            name: remaining[..length].into(),
        });
        remaining = &remaining[length..];
        while let Some(tail) = remaining.strip_prefix('[') {
            let (index, rest) = tail.split_once(']').ok_or("unclosed path index")?;
            steps.push(PathStep::Index {
                index: index
                    .parse()
                    .map_err(|_| format!("unsupported JMESPath projection or index {index:?}"))?,
            });
            remaining = rest;
        }
        if remaining.is_empty() {
            break;
        }
        remaining = remaining
            .strip_prefix('.')
            .ok_or_else(|| format!("unsupported JMESPath expression {text:?}"))?;
        if remaining.is_empty() {
            return Err("path cannot end with a dot".into());
        }
    }
    if steps.is_empty() {
        return Err("path must not be empty".into());
    }
    Ok(steps)
}

pub(crate) fn template_parts(text: &str) -> Result<Vec<(bool, String)>, String> {
    let mut result = Vec::new();
    let mut rest = text;
    while let Some((literal, tail)) = rest.split_once('{') {
        if literal.contains('}') {
            return Err("unmatched template brace".into());
        }
        if !literal.is_empty() {
            result.push((false, literal.into()));
        }
        let (name, tail) = tail.split_once('}').ok_or("unclosed template label")?;
        if name.is_empty() || name.contains('{') {
            return Err("invalid template label".into());
        }
        result.push((true, name.into()));
        rest = tail;
    }
    if rest.contains('}') {
        return Err("unmatched template brace".into());
    }
    if !rest.is_empty() {
        result.push((false, rest.into()));
    }
    Ok(result)
}

pub(crate) fn string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} must be a string"))
}
pub(crate) fn optional_string(value: &Value, key: &str) -> Result<Option<String>, String> {
    value.get(key).map(|_| string(value, key)).transpose()
}
fn integer_default(value: &Value, key: &str, default: u64) -> Result<u64, String> {
    value
        .get(key)
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| format!("{key} must be a nonnegative integer"))
        })
        .transpose()
        .map(|n| n.unwrap_or(default))
}
