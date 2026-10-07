use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Segment, Service, Shape};
use regex::Regex;

use crate::{Config, DocumentationTarget, NameCase, Report, Severity};

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
        let compile =
            |pattern: &Option<String>| pattern.as_deref().map(crate::config::naming_pattern);
        let operations = compile(&rule.operations_pattern);
        let shapes = compile(&rule.shapes_pattern);
        let members = compile(&rule.members_pattern);
        let enums = compile(&rule.enum_values_pattern);
        let mut name = |id: ShapeId,
                        text: &str,
                        case: Option<NameCase>,
                        pattern: &Option<Result<Regex, regex::Error>>,
                        rule_id: &str| {
            let valid_case = case.is_none_or(|case| case.matches(text));
            let valid_pattern = pattern.as_ref().is_none_or(|pattern| {
                pattern.as_ref().is_ok_and(|regex| {
                    regex
                        .find(text)
                        .is_some_and(|matched| matched.start() == 0 && matched.end() == text.len())
                })
            });
            if !valid_case || !valid_pattern {
                report.push(
                    rule_id,
                    id,
                    rule.severity,
                    format!("{text:?} violates the configured naming convention"),
                );
            }
        };
        name(
            service.id.clone(),
            service.id.name(),
            rule.shapes,
            &shapes,
            "naming.shapes",
        );
        for operation in &service.operations {
            if selected(&operation.id) {
                name(
                    operation.id.clone(),
                    operation.name(),
                    rule.operations,
                    &operations,
                    "naming.operations",
                );
            }
        }
        for (id, shape) in &service.shapes {
            if !selected(id) {
                continue;
            }
            name(id.clone(), id.name(), rule.shapes, &shapes, "naming.shapes");
            match shape {
                Shape::Structure(shape) | Shape::Union(shape) => {
                    for member in &shape.members {
                        name(
                            id.with_member(&member.name),
                            &member.name,
                            rule.members,
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
                            rule.enum_values,
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
                            rule.enum_values,
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
            if let Some(prefix) = &rule.prefix {
                if prefix != "/"
                    && path != prefix
                    && !path
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with('/'))
                {
                    report.push(
                        "paths.prefix",
                        &operation.id,
                        rule.severity,
                        format!("path {path:?} must start with {prefix:?} at a segment boundary"),
                    );
                }
            }
            for segment in &operation.uri.segments {
                let (text, case, rule_id) = match segment {
                    Segment::Literal(text) => (text, rule.segment_case, "paths.segments"),
                    Segment::Label(text) => (text, rule.parameter_case, "paths.parameters"),
                };
                if case.is_some_and(|case| !case.matches(text)) {
                    report.push(
                        rule_id,
                        &operation.id,
                        rule.severity,
                        format!("path segment {text:?} violates the configured naming convention"),
                    );
                }
            }
        }
    }
    if let Some(rule) = &config.rules.documentation {
        for target in &rule.targets {
            match target {
                DocumentationTarget::Service => docs(
                    &mut report,
                    &service.id,
                    service.docs.as_deref(),
                    rule.severity,
                ),
                DocumentationTarget::Operations => {
                    for operation in &service.operations {
                        if selected(&operation.id) {
                            docs(
                                &mut report,
                                &operation.id,
                                operation.docs.as_deref(),
                                rule.severity,
                            );
                        }
                    }
                }
                DocumentationTarget::Shapes | DocumentationTarget::Errors => {
                    for (id, shape) in &service.shapes {
                        if !selected(id) {
                            continue;
                        }
                        if matches!(target, DocumentationTarget::Errors)
                            && !matches!(shape, Shape::Structure(shape) if shape.error.is_some())
                        {
                            continue;
                        }
                        let text = match shape {
                            Shape::Unit => continue,
                            Shape::Simple(s) => &s.docs,
                            Shape::Enum(s) => &s.docs,
                            Shape::IntEnum(s) => &s.docs,
                            Shape::List(s) => &s.docs,
                            Shape::Map(s) => &s.docs,
                            Shape::Structure(s) | Shape::Union(s) => &s.docs,
                        };
                        docs(&mut report, id, text.as_deref(), rule.severity);
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
                                    rule.severity,
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
                                        rule.severity,
                                    );
                                }
                            }
                            Shape::IntEnum(shape) => {
                                for value in &shape.values {
                                    docs(
                                        &mut report,
                                        &id.with_member(&value.name),
                                        value.docs.as_deref(),
                                        rule.severity,
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
