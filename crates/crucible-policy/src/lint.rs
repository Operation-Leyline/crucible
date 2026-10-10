use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Segment, Service, Shape};
use regex::Regex;

use crate::{Check, Config, DocumentationTarget, NameCase, Report, Severity};

/// A compiled whole-name pattern, its source and its severity.
type Pattern = Option<(Result<Regex, regex::Error>, String, Severity)>;

pub fn check(service: &Service, config: &Config) -> Report {
    let mut report = Report::default();
    let selected = |id: &ShapeId| {
        if config.rules.namespaces.is_empty() {
            id.namespace() == service.id.namespace()
        } else {
            config
                .rules
                .namespaces
                .iter()
                .any(|namespace| namespace == id.namespace())
        }
    };
    if let Some(rule) = &config.rules.naming {
        let compile = |pattern: &Option<Check<String>>| {
            pattern.as_ref().map(|pattern| {
                (
                    crate::config::naming_pattern(&pattern.value),
                    pattern.value.clone(),
                    pattern.severity_in(rule.severity),
                )
            })
        };
        let operations = compile(&rule.operations_pattern);
        let shapes = compile(&rule.shapes_pattern);
        let members = compile(&rule.members_pattern);
        let enums = compile(&rule.enum_values_pattern);
        let mut name = |id: ShapeId,
                        text: &str,
                        what: &str,
                        case: Option<&Check<NameCase>>,
                        pattern: &Pattern,
                        rule_id: &str| {
            if let Some(case) = case
                && !case.value.matches(text)
            {
                report.push(
                    rule_id,
                    id.clone(),
                    case.severity_in(rule.severity),
                    format!("{what} name `{text}` is not {}", case.value.name()),
                );
            }
            if let Some((regex, source, severity)) = pattern {
                let matched = regex.as_ref().is_ok_and(|regex| {
                    regex
                        .find(text)
                        .is_some_and(|found| found.start() == 0 && found.end() == text.len())
                });
                if !matched {
                    report.push(
                        rule_id,
                        id,
                        *severity,
                        format!("{what} name `{text}` does not match the pattern `{source}`"),
                    );
                }
            }
        };
        name(
            service.id.clone(),
            service.id.name(),
            "service",
            rule.shapes.as_ref(),
            &shapes,
            "naming.shapes",
        );
        for operation in &service.operations {
            if selected(&operation.id) {
                name(
                    operation.id.clone(),
                    operation.name(),
                    "operation",
                    rule.operations.as_ref(),
                    &operations,
                    "naming.operations",
                );
            }
        }
        for (id, shape) in &service.shapes {
            if !selected(id) {
                continue;
            }
            name(
                id.clone(),
                id.name(),
                "shape",
                rule.shapes.as_ref(),
                &shapes,
                "naming.shapes",
            );
            match shape {
                Shape::Structure(shape) | Shape::Union(shape) => {
                    for member in &shape.members {
                        name(
                            id.with_member(&member.name),
                            &member.name,
                            "member",
                            rule.members.as_ref(),
                            &members,
                            "naming.members",
                        );
                    }
                }
                Shape::Enum(shape) => {
                    for value in &shape.values {
                        name(
                            id.with_member(&value.name),
                            &value.name,
                            "enum value",
                            rule.enum_values.as_ref(),
                            &enums,
                            "naming.enums",
                        );
                    }
                }
                Shape::IntEnum(shape) => {
                    for value in &shape.values {
                        name(
                            id.with_member(&value.name),
                            &value.name,
                            "enum value",
                            rule.enum_values.as_ref(),
                            &enums,
                            "naming.enums",
                        );
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(rule) = &config.rules.paths {
        for operation in &service.operations {
            if !selected(&operation.id) {
                continue;
            }
            let path = operation.uri.to_string();
            let path = path.split('?').next().unwrap_or(&path);
            if let Some(check) = &rule.prefix {
                let prefix = &check.value;
                if prefix != "/"
                    && path != prefix
                    && !path
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
                {
                    report.push(
                        "paths.prefix",
                        &operation.id,
                        check.severity_in(rule.severity),
                        format!("path {path:?} must start with {prefix:?} at a segment boundary"),
                    );
                }
            }
            for segment in &operation.uri.segments {
                let (text, case, rule_id, what) = match segment {
                    Segment::Literal(text) => {
                        (text, &rule.segment_case, "paths.segments", "path segment")
                    }
                    Segment::Label(text) => (
                        text,
                        &rule.parameter_case,
                        "paths.parameters",
                        "path parameter",
                    ),
                };
                if let Some(case) = case
                    && !case.value.matches(text)
                {
                    report.push(
                        rule_id,
                        &operation.id,
                        case.severity_in(rule.severity),
                        format!("{what} `{text}` is not {}", case.value.name()),
                    );
                }
            }
        }
    }
    if let Some(rule) = &config.rules.documentation {
        for target in &rule.targets {
            let severity = target.severity_in(rule.severity);
            match target.value {
                DocumentationTarget::Service => {
                    docs(&mut report, &service.id, service.docs.as_deref(), severity)
                }
                DocumentationTarget::Operations => {
                    for operation in &service.operations {
                        if selected(&operation.id) {
                            docs(
                                &mut report,
                                &operation.id,
                                operation.docs.as_deref(),
                                severity,
                            );
                        }
                    }
                }
                DocumentationTarget::Shapes | DocumentationTarget::Errors => {
                    for (id, shape) in &service.shapes {
                        if !selected(id) {
                            continue;
                        }
                        if matches!(target.value, DocumentationTarget::Errors)
                            && !matches!(shape, Shape::Structure(shape) if shape.error.is_some())
                        {
                            continue;
                        }
                        let text = match shape {
                            Shape::Unit { .. } => continue,
                            Shape::Simple(s) => &s.docs,
                            Shape::Enum(s) => &s.docs,
                            Shape::IntEnum(s) => &s.docs,
                            Shape::List(s) => &s.docs,
                            Shape::Map(s) => &s.docs,
                            Shape::Structure(s) | Shape::Union(s) => &s.docs,
                        };
                        docs(&mut report, id, text.as_deref(), severity);
                    }
                }
                DocumentationTarget::Members => {
                    for (id, shape) in &service.shapes {
                        if !selected(id) {
                            continue;
                        }
                        if let Shape::Structure(shape) | Shape::Union(shape) = shape {
                            for member in &shape.members {
                                docs(
                                    &mut report,
                                    &id.with_member(&member.name),
                                    member.docs.as_deref(),
                                    severity,
                                );
                            }
                        }
                    }
                }
                DocumentationTarget::Enums => {
                    for (id, shape) in &service.shapes {
                        if !selected(id) {
                            continue;
                        }
                        match shape {
                            Shape::Enum(shape) => {
                                for value in &shape.values {
                                    docs(
                                        &mut report,
                                        &id.with_member(&value.name),
                                        value.docs.as_deref(),
                                        severity,
                                    );
                                }
                            }
                            Shape::IntEnum(shape) => {
                                for value in &shape.values {
                                    docs(
                                        &mut report,
                                        &id.with_member(&value.name),
                                        value.docs.as_deref(),
                                        severity,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    report.finish(config)
}

fn docs(report: &mut Report, id: &ShapeId, text: Option<&str>, severity: Severity) {
    if text.is_none_or(|text| text.trim().is_empty()) {
        report.push(
            "documentation.required",
            id,
            severity,
            "documentation is required",
        );
    }
}
