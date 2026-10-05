//! Adapt compiled Smithy JMESPath descriptions to the pinned native interpreter.
//! Numeric operators use exact decimals rather than its default f64 comparisons.
use std::cmp::Ordering;
use std::sync::LazyLock;

use crucible_wire::number::DecimalValue;
use jmespath::ast::{Ast, KeyValuePair};
use jmespath::functions::{ArgumentType as Type, CustomFunction, Signature};
use jmespath::{
    Context, ErrorReason, JmespathError, Rcvar, Runtime, RuntimeError, SearchResult, Variable,
    interpret,
};
use serde_json::Value;

#[derive(Debug)]
pub(crate) struct Query(Ast);

impl Query {
    pub(crate) fn new(description: &Value) -> Result<Self, String> {
        ast(description).map(Self)
    }
    pub(crate) fn search(&self, input: &Value) -> Result<Value, String> {
        // ToJmespath's generic serde adapter confuses arbitrary-precision numeric
        // tokens with objects. Direct Value conversion preserves their identity.
        let input = variable(input);
        let output = interpret(
            &input,
            &self.0,
            &mut Context::new("compiled Smithy expression", &RUNTIME),
        )
        .map_err(|error| error.to_string())?;
        serde_json::to_value(&*output).map_err(|error| error.to_string())
    }
}

fn variable(value: &Value) -> Rcvar {
    Rcvar::new(match value {
        Value::Null => Variable::Null,
        Value::Bool(value) => Variable::Bool(*value),
        Value::Number(value) => Variable::Number(value.clone()),
        Value::String(value) => Variable::String(value.clone()),
        Value::Array(values) => Variable::Array(values.iter().map(variable).collect()),
        Value::Object(values) => Variable::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), variable(value)))
                .collect(),
        ),
    })
}

fn ast(node: &Value) -> Result<Ast, String> {
    let text = |key: &str| {
        node[key]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("missing compiled JMESPath {key}"))
    };
    let child = |key: &str| ast(&node[key]).map(Box::new);
    let list = |key: &str| {
        node[key]
            .as_array()
            .ok_or_else(|| format!("missing compiled JMESPath {key}"))?
            .iter()
            .map(ast)
            .collect::<Result<Vec<_>, _>>()
    };
    let integer = |key: &str| {
        node[key]
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| format!("invalid compiled JMESPath {key}"))
    };
    Ok(match text("kind")?.as_str() {
        "identity" => Ast::Identity { offset: 0 },
        "field" => Ast::Field {
            offset: 0,
            name: text("name")?,
        },
        "index" => Ast::Index {
            offset: 0,
            idx: integer("index")?,
        },
        "literal" => Ast::Literal {
            offset: 0,
            value: variable(&node["value"]),
        },
        "subexpression" => Ast::Subexpr {
            offset: 0,
            lhs: child("left")?,
            rhs: child("right")?,
        },
        "projection" => Ast::Projection {
            offset: 0,
            lhs: child("left")?,
            rhs: child("right")?,
        },
        "and" => Ast::And {
            offset: 0,
            lhs: child("left")?,
            rhs: child("right")?,
        },
        "or" => Ast::Or {
            offset: 0,
            lhs: child("left")?,
            rhs: child("right")?,
        },
        "flatten" => Ast::Flatten {
            offset: 0,
            node: child("expression")?,
        },
        "objectValues" => Ast::ObjectValues {
            offset: 0,
            node: child("expression")?,
        },
        "not" => Ast::Not {
            offset: 0,
            node: child("expression")?,
        },
        "expressionReference" => Ast::Expref {
            offset: 0,
            ast: child("expression")?,
        },
        "slice" => Ast::Slice {
            offset: 0,
            start: if node["start"].is_null() {
                None
            } else {
                Some(integer("start")?)
            },
            stop: if node["stop"].is_null() {
                None
            } else {
                Some(integer("stop")?)
            },
            step: integer("step")?,
        },
        "multiList" => Ast::MultiList {
            offset: 0,
            elements: list("expressions")?,
        },
        "multiHash" => Ast::MultiHash {
            offset: 0,
            elements: node["entries"]
                .as_array()
                .ok_or("missing compiled entries")?
                .iter()
                .map(|entry| {
                    Ok(KeyValuePair {
                        key: entry["key"]
                            .as_str()
                            .ok_or("missing compiled key")?
                            .to_owned(),
                        value: ast(&entry["value"])?,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        "compare" => Ast::Function {
            offset: 0,
            name: format!("__crucible_{}", text("comparator")?),
            args: vec![ast(&node["left"])?, ast(&node["right"])?],
        },
        "condition" => Ast::Condition {
            offset: 0,
            predicate: child("predicate")?,
            then: child("then")?,
        },
        "function" => Ast::Function {
            offset: 0,
            name: text("function")?,
            args: list("arguments")?,
        },
        kind => return Err(format!("unknown compiled JMESPath node {kind}")),
    })
}

fn invalid(context: &Context<'_>, expected: &str) -> JmespathError {
    JmespathError::new(
        context.expression,
        context.offset,
        ErrorReason::Runtime(RuntimeError::InvalidType {
            expected: expected.into(),
            actual: "incompatible value".into(),
            position: 0,
        }),
    )
}
fn number(value: &Variable, context: &Context<'_>) -> Result<DecimalValue, JmespathError> {
    match value {
        Variable::Number(number) => number
            .to_string()
            .parse()
            .map_err(|_| invalid(context, "exact decimal")),
        _ => Err(invalid(context, "number")),
    }
}
fn numeric(value: DecimalValue, context: &Context<'_>) -> SearchResult {
    let value = value
        .to_string()
        .parse()
        .map_err(|_| invalid(context, "JSON number"))?;
    Ok(Rcvar::new(Variable::Number(value)))
}
fn equals(a: &Variable, b: &Variable, context: &Context<'_>) -> Result<bool, JmespathError> {
    Ok(match (a, b) {
        (Variable::Number(_), Variable::Number(_)) => number(a, context)? == number(b, context)?,
        (Variable::Array(a), Variable::Array(b)) if a.len() == b.len() => {
            for (a, b) in a.iter().zip(b) {
                if !equals(a, b, context)? {
                    return Ok(false);
                }
            }
            true
        }
        (Variable::Object(a), Variable::Object(b)) if a.len() == b.len() => {
            for (key, a) in a {
                let Some(b) = b.get(key) else {
                    return Ok(false);
                };
                if !equals(a, b, context)? {
                    return Ok(false);
                }
            }
            true
        }
        (Variable::Null, Variable::Null) => true,
        (Variable::Bool(a), Variable::Bool(b)) => a == b,
        (Variable::String(a), Variable::String(b)) => a == b,
        _ => false,
    })
}
fn function(
    runtime: &mut Runtime,
    name: &str,
    types: Vec<Type>,
    callback: impl Fn(&[Rcvar], &mut Context<'_>) -> SearchResult + Send + Sync + 'static,
) {
    runtime.register_function(
        name,
        Box::new(CustomFunction::new(
            Signature::new(types, None),
            Box::new(callback),
        )),
    );
}

static RUNTIME: LazyLock<Runtime> = LazyLock::new(|| {
    let mut runtime = Runtime::new();
    runtime.register_builtin_functions();
    for comparator in [
        "equal",
        "notEqual",
        "lessThan",
        "lessThanEqual",
        "greaterThan",
        "greaterThanEqual",
    ] {
        function(
            &mut runtime,
            &format!("__crucible_{comparator}"),
            vec![Type::Any, Type::Any],
            move |args, context| {
                let result = match comparator {
                    "equal" => equals(&args[0], &args[1], context)?,
                    "notEqual" => !equals(&args[0], &args[1], context)?,
                    _ if !args[0].is_number() || !args[1].is_number() => {
                        return Ok(Rcvar::new(Variable::Null));
                    }
                    _ => {
                        let order = number(&args[0], context)?.cmp(&number(&args[1], context)?);
                        match comparator {
                            "lessThan" => order.is_lt(),
                            "lessThanEqual" => order.is_le(),
                            "greaterThan" => order.is_gt(),
                            _ => order.is_ge(),
                        }
                    }
                };
                Ok(Rcvar::new(Variable::Bool(result)))
            },
        );
    }
    function(
        &mut runtime,
        "contains",
        vec![Type::Union(vec![Type::Array, Type::String]), Type::Any],
        |args, context| {
            let result = match &*args[0] {
                Variable::String(text) => text.contains(
                    args[1]
                        .as_string()
                        .ok_or_else(|| invalid(context, "string"))?,
                ),
                Variable::Array(values) => {
                    let mut found = false;
                    for value in values {
                        if equals(value, &args[1], context)? {
                            found = true;
                            break;
                        }
                    }
                    found
                }
                _ => unreachable!("validated function signature"),
            };
            Ok(Rcvar::new(Variable::Bool(result)))
        },
    );
    function(&mut runtime, "to_number", vec![Type::Any], |args, _| {
        Ok(match &*args[0] {
            Variable::Number(_) => args[0].clone(),
            Variable::String(text) => match serde_json::from_str::<Value>(text) {
                Ok(Value::Number(value)) => Rcvar::new(Variable::Number(value)),
                _ => Rcvar::new(Variable::Null),
            },
            _ => Rcvar::new(Variable::Null),
        })
    });
    for name in ["abs", "ceil", "floor"] {
        function(
            &mut runtime,
            name,
            vec![Type::Number],
            move |args, context| {
                let value = number(&args[0], context)?;
                let result = if name == "abs" {
                    value.abs()
                } else {
                    let rounded = value.with_scale(0);
                    if name == "ceil" && rounded < value {
                        rounded + 1
                    } else if name == "floor" && rounded > value {
                        rounded - 1
                    } else {
                        rounded
                    }
                };
                numeric(result, context)
            },
        );
    }
    for name in ["sum", "avg"] {
        function(
            &mut runtime,
            name,
            vec![Type::TypedArray(Box::new(Type::Number))],
            move |args, context| {
                let values = args[0].as_array().expect("validated array");
                if name == "avg" && values.is_empty() {
                    return Ok(Rcvar::new(Variable::Null));
                }
                let mut sum = DecimalValue::from(0);
                for value in values {
                    sum += number(value, context)?;
                }
                if name == "avg" {
                    sum = sum / DecimalValue::from(values.len() as u64);
                }
                numeric(sum, context)
            },
        );
    }
    for name in ["sort", "min", "max", "sort_by", "min_by", "max_by"] {
        let keyed = name.ends_with("_by");
        let mut signature = vec![Type::Array];
        if keyed {
            signature.push(Type::Expref);
        }
        function(&mut runtime, name, signature, move |args, context| {
            let values = args[0].as_array().expect("validated array");
            let mut entries = Vec::new();
            let mut numbers = None;
            for value in values {
                let key = if keyed {
                    interpret(
                        value,
                        args[1].as_expref().expect("validated expression reference"),
                        context,
                    )?
                } else {
                    value.clone()
                };
                let numeric_key = key.is_number();
                if (!numeric_key && !key.is_string())
                    || numbers.is_some_and(|previous| previous != numeric_key)
                {
                    return Err(invalid(context, "uniform string or number sort keys"));
                }
                numbers = Some(numeric_key);
                let key = if numeric_key {
                    SortKey::Number(number(&key, context)?)
                } else {
                    SortKey::String(key.as_string().unwrap().clone())
                };
                entries.push((key, value.clone()));
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            if name.starts_with("sort") {
                return Ok(Rcvar::new(Variable::Array(
                    entries.into_iter().map(|(_, value)| value).collect(),
                )));
            }
            let extreme = if name.starts_with("min") {
                entries.first()
            } else {
                entries.last()
            };
            Ok(extreme
                .and_then(|(key, _)| entries.iter().find(|(candidate, _)| candidate == key))
                .map_or_else(|| Rcvar::new(Variable::Null), |(_, value)| value.clone()))
        });
    }
    runtime
});

#[derive(PartialEq, Eq)]
enum SortKey {
    Number(DecimalValue),
    String(String),
}
impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a.cmp(b),
            (Self::String(a), Self::String(b)) => a.cmp(b),
            _ => unreachable!("sort key types validated"),
        }
    }
}
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Query;
    use serde_json::{Value, json};
    #[test]
    fn compound_literals_and_decimal_comparisons_remain_exact() {
        let description: Value = serde_json::from_str(r#"{"kind":"compare","comparator":"greaterThan","left":{"kind":"literal","value":12345678901234567890.000000001},"right":{"kind":"literal","value":12345678901234567890}}"#).unwrap();
        assert_eq!(
            Query::new(&description)
                .unwrap()
                .search(&Value::Null)
                .unwrap(),
            json!(true)
        );
        let input: Value = serde_json::from_str(
            r#"{"numbers":[1,2.5],"$serde_json::private::Number":"ordinary"}"#,
        )
        .unwrap();
        assert_eq!(
            Query::new(&json!({"kind":"identity"}))
                .unwrap()
                .search(&input)
                .unwrap(),
            input
        );
        let sort: Value = serde_json::from_str(r#"{"kind":"function","function":"sort","arguments":[{"kind":"literal","value":[9007199254740993,9007199254740992]}]}"#).unwrap();
        assert_eq!(
            Query::new(&sort)
                .unwrap()
                .search(&Value::Null)
                .unwrap()
                .to_string(),
            "[9007199254740992,9007199254740993]"
        );
    }
}
