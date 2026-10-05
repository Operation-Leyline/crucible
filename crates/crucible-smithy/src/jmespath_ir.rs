//! A portable JMESPath program compiled once, never re-parsed by client emitters.
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JmesExpression {
    Identity,
    Field {
        name: String,
    },
    Index {
        index: i32,
    },
    Literal {
        value: Value,
    },
    Subexpression {
        left: Box<Self>,
        right: Box<Self>,
    },
    Projection {
        left: Box<Self>,
        right: Box<Self>,
    },
    Flatten {
        expression: Box<Self>,
    },
    ObjectValues {
        expression: Box<Self>,
    },
    Slice {
        start: Option<i32>,
        stop: Option<i32>,
        step: i32,
    },
    MultiList {
        expressions: Vec<Self>,
    },
    MultiHash {
        entries: Vec<JmesEntry>,
    },
    And {
        left: Box<Self>,
        right: Box<Self>,
    },
    Or {
        left: Box<Self>,
        right: Box<Self>,
    },
    Not {
        expression: Box<Self>,
    },
    Compare {
        comparator: JmesComparator,
        left: Box<Self>,
        right: Box<Self>,
    },
    Condition {
        predicate: Box<Self>,
        then: Box<Self>,
    },
    ExpressionReference {
        expression: Box<Self>,
    },
    Function {
        function: JmesFunction,
        arguments: Vec<Self>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JmesEntry {
    pub key: String,
    pub value: JmesExpression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JmesComparator {
    Equal,
    NotEqual,
    LessThan,
    LessThanEqual,
    GreaterThan,
    GreaterThanEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JmesFunction {
    Abs,
    Avg,
    Ceil,
    Contains,
    EndsWith,
    Floor,
    Join,
    Keys,
    Length,
    Map,
    Max,
    MaxBy,
    Merge,
    Min,
    MinBy,
    NotNull,
    Reverse,
    Sort,
    SortBy,
    StartsWith,
    Sum,
    ToArray,
    ToNumber,
    ToString,
    Type,
    Values,
}

pub(crate) fn compile(source: &str) -> Result<JmesExpression, String> {
    let ast = jmespath::parse(source).map_err(|e| format!("invalid JMESPath expression: {e}"))?;
    lower(&ast, source)
}

fn lower(ast: &jmespath::ast::Ast, source: &str) -> Result<JmesExpression, String> {
    use JmesExpression as E;
    use jmespath::ast::{Ast, Comparator};
    let boxed = |ast: &Ast| lower(ast, source).map(Box::new);
    Ok(match ast {
        Ast::Identity { .. } => E::Identity,
        Ast::Field { name, .. } => E::Field { name: name.clone() },
        Ast::Index { idx, .. } => E::Index { index: *idx },
        Ast::Literal { value, offset } => E::Literal {
            value: if source.as_bytes().get(*offset) == Some(&b'`') {
                // jmespath's Variable deserializer interprets serde_json's arbitrary-precision
                // number token as an object. Read the already-validated literal source directly
                // so decimals, large integers, and genuine objects retain their distinct types.
                let mut text = String::new();
                let mut chars = source[offset + 1..].chars();
                while let Some(ch) = chars.next() {
                    if ch == '`' {
                        break;
                    }
                    if ch == '\\' {
                        if let Some(next) = chars.next() {
                            if next != '`' {
                                text.push(ch);
                            }
                            text.push(next);
                        }
                    } else {
                        text.push(ch);
                    }
                }
                serde_json::from_str(&text).map_err(|e| e.to_string())?
            } else {
                serde_json::to_value(value.as_ref()).map_err(|e| e.to_string())?
            },
        },
        Ast::Subexpr { lhs, rhs, .. } => E::Subexpression {
            left: boxed(lhs)?,
            right: boxed(rhs)?,
        },
        Ast::Projection { lhs, rhs, .. } => E::Projection {
            left: boxed(lhs)?,
            right: boxed(rhs)?,
        },
        Ast::Flatten { node, .. } => E::Flatten {
            expression: boxed(node)?,
        },
        Ast::ObjectValues { node, .. } => E::ObjectValues {
            expression: boxed(node)?,
        },
        Ast::Slice {
            start, stop, step, ..
        } => E::Slice {
            start: *start,
            stop: *stop,
            step: *step,
        },
        Ast::MultiList { elements, .. } => E::MultiList {
            expressions: elements
                .iter()
                .map(|node| lower(node, source))
                .collect::<Result<_, _>>()?,
        },
        Ast::MultiHash { elements, .. } => E::MultiHash {
            entries: elements
                .iter()
                .map(|entry| {
                    Ok(JmesEntry {
                        key: entry.key.clone(),
                        value: lower(&entry.value, source)?,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        Ast::And { lhs, rhs, .. } => E::And {
            left: boxed(lhs)?,
            right: boxed(rhs)?,
        },
        Ast::Or { lhs, rhs, .. } => E::Or {
            left: boxed(lhs)?,
            right: boxed(rhs)?,
        },
        Ast::Not { node, .. } => E::Not {
            expression: boxed(node)?,
        },
        Ast::Comparison {
            comparator,
            lhs,
            rhs,
            ..
        } => E::Compare {
            comparator: match comparator {
                Comparator::Equal => JmesComparator::Equal,
                Comparator::NotEqual => JmesComparator::NotEqual,
                Comparator::LessThan => JmesComparator::LessThan,
                Comparator::LessThanEqual => JmesComparator::LessThanEqual,
                Comparator::GreaterThan => JmesComparator::GreaterThan,
                Comparator::GreaterThanEqual => JmesComparator::GreaterThanEqual,
            },
            left: boxed(lhs)?,
            right: boxed(rhs)?,
        },
        Ast::Condition {
            predicate, then, ..
        } => E::Condition {
            predicate: boxed(predicate)?,
            then: boxed(then)?,
        },
        Ast::Expref { ast, .. } => E::ExpressionReference {
            expression: boxed(ast)?,
        },
        Ast::Function { name, args, .. } => E::Function {
            function: function(name)?,
            arguments: args
                .iter()
                .map(|node| lower(node, source))
                .collect::<Result<_, _>>()?,
        },
    })
}

fn function(name: &str) -> Result<JmesFunction, String> {
    use JmesFunction as F;
    Ok(match name {
        "abs" => F::Abs,
        "avg" => F::Avg,
        "ceil" => F::Ceil,
        "contains" => F::Contains,
        "ends_with" => F::EndsWith,
        "floor" => F::Floor,
        "join" => F::Join,
        "keys" => F::Keys,
        "length" => F::Length,
        "map" => F::Map,
        "max" => F::Max,
        "max_by" => F::MaxBy,
        "merge" => F::Merge,
        "min" => F::Min,
        "min_by" => F::MinBy,
        "not_null" => F::NotNull,
        "reverse" => F::Reverse,
        "sort" => F::Sort,
        "sort_by" => F::SortBy,
        "starts_with" => F::StartsWith,
        "sum" => F::Sum,
        "to_array" => F::ToArray,
        "to_number" => F::ToNumber,
        "to_string" => F::ToString,
        "type" => F::Type,
        "values" => F::Values,
        _ => return Err(format!("unsupported JMESPath function {name}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_literals_preserve_exact_json_types() {
        for literal in [
            "1.5",
            "-1.1",
            "18446744073709551617000001",
            "[1.5,-1.1]",
            r#"{"$serde_json::private::Number":"1.5"}"#,
            r#"{"backtick":"a\`b"}"#,
        ] {
            let expression = compile(&format!("`{literal}`")).unwrap();
            let expected: Value = serde_json::from_str(&literal.replace("\\`", "`")).unwrap();
            assert_eq!(expression, JmesExpression::Literal { value: expected });
        }
        let expression = compile("avg(`[1,2]`) == `1.5`").unwrap();
        let value = serde_json::to_value(expression).unwrap();
        assert_eq!(value["right"]["value"], serde_json::json!(1.5));
        assert!(compile("'é' == `1.5`").is_ok());
    }
    #[test]
    fn standard_operators_are_lowered_without_runtime_source_strings() {
        for source in [
            "Reservations[].Instances[].State.Name",
            "items[?status == 'ready'].id | length(@)",
            "items[0:4:2]",
            "sort_by(items, &name)[-1]",
            "{names: items[*].name, flag: active || `false`}",
            "foo.*.bar",
            "[foo,bar]",
            "!active && count > `2`",
            "map(&name, items)",
        ] {
            let expression = compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(
                serde_json::to_value(expression)
                    .unwrap()
                    .get("kind")
                    .is_some()
            );
        }
        assert!(
            compile("invented(foo)")
                .unwrap_err()
                .contains("unsupported JMESPath function")
        );
        assert!(compile("items[").is_err());
    }
}
