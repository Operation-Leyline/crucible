//! Evaluating a checked condition on one subject.

use std::collections::BTreeSet;

use crucible_ir::ShapeId;
use crucible_ir::ir::{Bounds, Constraints, Shape, Traits};
use serde_json::Value as Json;

use super::metadata::MetaIndex;
use super::subject::{Class, Entity, Subject, World, parts, prelude_class};
use super::vocabulary::{TypeIs, type_is};
use crate::ast::{Expr, ExprKind, Operand, Operator, Path, Ref, Value};

/// A fact the model does not have for this subject, such as the constraints
/// of a shape no operation reaches. The clause skips the subject rather than
/// guess.
#[derive(Debug, Clone, Copy)]
pub(super) struct Unknown;

pub(super) type Tri = Result<bool, Unknown>;

/// A number as the model holds it: whole numbers stay exact, so two bounds
/// past 2^53 do not compare equal.
#[derive(Debug, Clone, Copy)]
enum Num {
    Int(i128),
    Float(f64),
}

impl Num {
    fn from_json(number: &serde_json::Number) -> Option<Num> {
        if let Some(i) = number.as_i64() {
            Some(Num::Int(i128::from(i)))
        } else if let Some(u) = number.as_u64() {
            Some(Num::Int(i128::from(u)))
        } else {
            number.as_f64().map(Num::Float)
        }
    }

    fn from_literal(text: &str) -> Option<Num> {
        text.parse::<i128>()
            .map(Num::Int)
            .or_else(|_| text.parse::<f64>().map(Num::Float))
            .ok()
    }

    fn compare(self, other: Num) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Num::Int(a), Num::Int(b)) => Some(a.cmp(&b)),
            (a, b) => a.float().partial_cmp(&b.float()),
        }
    }

    fn float(self) -> f64 {
        match self {
            Num::Int(i) => i as f64,
            Num::Float(f) => f,
        }
    }
}

enum Val {
    Str(String),
    Num(Num),
    Bool(bool),
}

pub(super) struct Evaluator<'a, 'w> {
    pub world: &'w World<'a>,
    pub meta: &'w MetaIndex,
    /// Each `matches` pattern, compiled once.
    pub patterns: std::cell::RefCell<std::collections::HashMap<String, Option<regex::Regex>>>,
}

impl Evaluator<'_, '_> {
    pub(super) fn eval(&self, subject: &Subject<'_>, expr: &Expr) -> Tri {
        match &expr.kind {
            // Three-valued: an unknown side does not hide what the other decides.
            ExprKind::Or(left, right) => {
                match (self.eval(subject, left), self.eval(subject, right)) {
                    (Ok(true), _) | (_, Ok(true)) => Ok(true),
                    (Ok(false), Ok(false)) => Ok(false),
                    _ => Err(Unknown),
                }
            }
            ExprKind::And(left, right) => {
                match (self.eval(subject, left), self.eval(subject, right)) {
                    (Ok(false), _) | (_, Ok(false)) => Ok(false),
                    (Ok(true), Ok(true)) => Ok(true),
                    _ => Err(Unknown),
                }
            }
            ExprKind::Not(inner) => Ok(!self.eval(subject, inner)?),
            ExprKind::Has(reference) => self.has(subject, reference),
            ExprKind::HasOneOf(references) => {
                let mut unknown = false;
                for reference in references {
                    match self.has(subject, reference) {
                        Ok(true) => return Ok(true),
                        Ok(false) => {}
                        Err(_) => unknown = true,
                    }
                }
                if unknown { Err(Unknown) } else { Ok(false) }
            }
            ExprKind::Addresses(resource) => Ok(match &subject.entity {
                Entity::Operation(operation) => operation
                    .resource
                    .as_ref()
                    .is_some_and(|bound| self.world.is_within(bound.name(), &resource.name)),
                _ => false,
            }),
            ExprKind::Flag(path) => self.flag(subject, path),
            ExprKind::Compare { path, op, operand } => self.compare(subject, path, *op, operand),
        }
    }

    fn traits<'s>(&self, subject: &'s Subject<'_>) -> Option<&'s Traits> {
        match &subject.entity {
            Entity::Service(service) => Some(&service.traits),
            Entity::Operation(operation) => Some(&operation.traits),
            Entity::Shape { shape, .. } => shape.and_then(|s| parts(s).traits),
            Entity::Member { member, .. } => Some(&member.traits),
            Entity::Resource { .. } => None,
        }
    }

    fn constraints<'s>(
        &self,
        subject: &'s Subject<'_>,
    ) -> Result<Option<&'s Constraints>, Unknown> {
        match &subject.entity {
            Entity::Member { member, .. } => Ok(Some(&member.constraints)),
            Entity::Shape { shape: None, .. } => Err(Unknown),
            Entity::Shape {
                shape: Some(shape), ..
            } => Ok(parts(shape).constraints),
            _ => Ok(None),
        }
    }

    fn meta_key(&self, reference: &Ref) -> Option<ShapeId> {
        self.meta
            .get(&reference.name.name)
            .map(|info| ShapeId::new(&info.ns, &reference.name.name))
    }

    fn has(&self, subject: &Subject<'_>, reference: &Ref) -> Tri {
        if reference.metadata {
            if matches!(subject.entity, Entity::Shape { shape: None, .. }) {
                return Err(Unknown);
            }
            let Some(key) = self.meta_key(reference) else {
                return Ok(self.built_in(subject, &reference.name.name));
            };
            return Ok(self.traits(subject).is_some_and(|t| t.contains_key(&key)));
        }
        let constraints = self.constraints(subject)?;
        Ok(
            constraints.is_some_and(|c| match reference.name.name.as_str() {
                "length" => c.length.is_some(),
                "range" => c.range.is_some(),
                "pattern" => c.pattern.is_some(),
                _ => false,
            }),
        )
    }

    /// `@token`, `@query`, `@header`, `@payload` and `@deprecated`, which the
    /// IR holds as typed fields.
    fn built_in(&self, subject: &Subject<'_>, name: &str) -> bool {
        match (&subject.entity, name) {
            (Entity::Member { member, .. }, "token") => member.idempotency_token,
            (Entity::Member { bindings, .. }, "query" | "header" | "payload") => {
                bindings.contains(&name)
            }
            (Entity::Member { member, .. }, "deprecated") => member.notes.deprecated.is_some(),
            (Entity::Operation(operation), "deprecated") => operation.notes.deprecated.is_some(),
            (
                Entity::Shape {
                    shape: Some(shape), ..
                },
                "deprecated",
            ) => shape
                .notes()
                .is_some_and(|notes| notes.deprecated.is_some()),
            (Entity::Service(service), "deprecated") => service.notes.deprecated.is_some(),
            _ => false,
        }
    }

    fn flag(&self, subject: &Subject<'_>, path: &Path) -> Tri {
        if path.head.metadata {
            return self.has(subject, &path.head);
        }
        let name = path.head.name.name.as_str();
        Ok(match (&subject.entity, name) {
            (Entity::Service(service), "documented") => service
                .docs
                .as_deref()
                .is_some_and(|d| !d.trim().is_empty()),
            (Entity::Resource { documented }, "documented") => *documented,
            (Entity::Operation(op), "documented") => {
                op.docs.as_deref().is_some_and(|d| !d.trim().is_empty())
            }
            (Entity::Shape { documented, .. }, "documented") => *documented,
            (Entity::Member { member, .. }, "documented") => {
                member.docs.as_deref().is_some_and(|d| !d.trim().is_empty())
            }
            (Entity::Operation(op), "internal") => op.internal,
            (Entity::Operation(op), "readonly") => op.readonly,
            (Entity::Operation(op), "idempotent") => op.idempotent,
            (Entity::Operation(op), "public") => op.public,
            (Entity::Operation(op), "paginated") => op.paginated.is_some(),
            (Entity::Shape { shape: None, .. }, "internal" | "sensitive") => return Err(Unknown),
            (
                Entity::Shape {
                    shape: Some(shape), ..
                },
                "internal",
            ) => parts(shape).internal,
            (
                Entity::Shape {
                    shape: Some(shape), ..
                },
                "sensitive",
            ) => parts(shape).sensitive,
            (Entity::Shape { class, .. }, "enum") => class.is_enum(),
            (Entity::Shape { referenced, .. }, "referenced") => *referenced,
            (Entity::Member { member, .. }, "internal") => member.internal,
            (Entity::Member { member, .. }, "sensitive") => member.sensitive,
            (Entity::Member { member, .. }, "optional") => !member.required,
            (Entity::Member { member, .. }, "required") => member.required,
            (Entity::Member { template, .. }, "template") => *template,
            _ => false,
        })
    }

    fn value(&self, subject: &Subject<'_>, path: &Path) -> Result<Option<Val>, Unknown> {
        if path.head.metadata {
            if matches!(subject.entity, Entity::Shape { shape: None, .. }) {
                return Err(Unknown);
            }
            let Some(key) = self.meta_key(&path.head) else {
                return Ok(None);
            };
            return Ok(self
                .traits(subject)
                .and_then(|t| t.get(&key))
                .and_then(|v| match v {
                    Json::String(s) => Some(Val::Str(s.clone())),
                    Json::Bool(b) => Some(Val::Bool(*b)),
                    Json::Number(n) => Num::from_json(n).map(Val::Num),
                    _ => None,
                }));
        }
        let rest: Vec<&str> = path.rest.iter().map(|r| r.name.as_str()).collect();
        let bound = |b: &Bounds<u64>, which: &str| match which {
            "min" => b.min.map(|n| Val::Num(Num::Int(i128::from(n)))),
            _ => b.max.map(|n| Val::Num(Num::Int(i128::from(n)))),
        };
        let range = |b: &Bounds<serde_json::Number>, which: &str| match which {
            "min" => b.min.as_ref().and_then(Num::from_json).map(Val::Num),
            _ => b.max.as_ref().and_then(Num::from_json).map(Val::Num),
        };
        Ok(match (path.head.name.name.as_str(), rest.as_slice()) {
            ("name", []) => Some(Val::Str(subject.name.clone())),
            ("role", []) => self
                .world
                .role(&subject.name)
                .map(|r| Val::Str(r.to_owned())),
            ("http", [field]) => match &subject.entity {
                Entity::Operation(op) => match *field {
                    "method" => Some(Val::Str(op.method.clone())),
                    "uri" => Some(Val::Str(op.uri.to_string())),
                    _ => Some(Val::Num(Num::Int(i128::from(op.status)))),
                },
                _ => None,
            },
            ("type", [which @ ("length" | "range"), bound_name]) => match &subject.entity {
                Entity::Member { member, .. } => {
                    let constraints = &member.constraints;
                    if *which == "length" {
                        constraints
                            .length
                            .as_ref()
                            .and_then(|b| bound(b, bound_name))
                    } else {
                        constraints
                            .range
                            .as_ref()
                            .and_then(|b| range(b, bound_name))
                    }
                }
                _ => None,
            },
            ("type", []) => match &subject.entity {
                Entity::Member { member, .. } => Some(Val::Str(member.target.name().to_owned())),
                _ => None,
            },
            ("length", [bound_name]) => self
                .constraints(subject)?
                .and_then(|c| c.length.as_ref())
                .and_then(|b| bound(b, bound_name)),
            ("range", [bound_name]) => self
                .constraints(subject)?
                .and_then(|c| c.range.as_ref())
                .and_then(|b| range(b, bound_name)),
            ("pattern", []) => self
                .constraints(subject)?
                .and_then(|c| c.pattern.clone())
                .map(Val::Str),
            _ => None,
        })
    }

    fn compare(&self, subject: &Subject<'_>, path: &Path, op: Operator, operand: &Operand) -> Tri {
        if op == Operator::Is {
            return Ok(self.is(subject, operand));
        }
        if matches!(path.head.name.name.as_str(), "input" | "output") && !path.head.metadata {
            return Ok(self.body_contains_sensitive(subject, &path.head.name.name));
        }
        let Operand::Value(value) = operand else {
            return Ok(false);
        };
        let left = self.value(subject, path)?;
        let Some(left) = left else {
            // Nothing to compare: not equal to anything, equal to nothing.
            return Ok(op == Operator::Ne);
        };
        Ok(match op {
            Operator::Eq => equals(&left, value),
            Operator::Ne => !equals(&left, value),
            Operator::In => match value {
                Value::List(items, _) => items.iter().any(|item| equals(&left, item)),
                _ => false,
            },
            Operator::Lt | Operator::Le | Operator::Gt | Operator::Ge => {
                use std::cmp::Ordering::{Equal, Greater, Less};
                let (Val::Num(left), Value::Number(right)) = (&left, value) else {
                    return Ok(false);
                };
                let Some(ordering) = Num::from_literal(&right.text).and_then(|r| left.compare(r))
                else {
                    return Ok(false);
                };
                match op {
                    Operator::Lt => ordering == Less,
                    Operator::Le => ordering != Greater,
                    Operator::Gt => ordering == Greater,
                    _ => ordering == Greater || ordering == Equal,
                }
            }
            Operator::StartsWith | Operator::EndsWith | Operator::Contains | Operator::Matches => {
                let (Val::Str(left), Value::Str(right)) = (&left, value) else {
                    return Ok(false);
                };
                match op {
                    Operator::StartsWith => left.starts_with(&right.value),
                    Operator::EndsWith => left.ends_with(&right.value),
                    Operator::Contains => left.contains(&right.value),
                    _ => self
                        .patterns
                        .borrow_mut()
                        .entry(right.value.clone())
                        .or_insert_with(|| regex::Regex::new(&right.value).ok())
                        .as_ref()
                        .is_some_and(|re| re.is_match(left)),
                }
            }
            Operator::Is => false,
        })
    }

    fn is(&self, subject: &Subject<'_>, operand: &Operand) -> bool {
        let Operand::Words(words) = operand else {
            return false;
        };
        let words: Vec<&str> = words.iter().map(|w| w.name.as_str()).collect();
        let Some(test) = type_is(&words) else {
            return false;
        };
        let Entity::Member { member, .. } = &subject.entity else {
            return false;
        };
        match test {
            TypeIs::InlinePrimitive => member.target.is_prelude() && member.target.name() != "Unit",
            TypeIs::Kind(kind) => if member.target.is_prelude() {
                // A built-in type is not among the service's shapes.
                prelude_class(member.target.name())
            } else {
                self.world
                    .service
                    .shapes
                    .get(&member.target)
                    .and_then(Class::of)
            }
            .is_some_and(|class| class.is(kind)),
        }
    }

    /// `input contains sensitive`: some member the operation's body reaches,
    /// through structures, unions, lists and maps, is sensitive.
    fn body_contains_sensitive(&self, subject: &Subject<'_>, side: &str) -> bool {
        let Entity::Operation(operation) = &subject.entity else {
            return false;
        };
        let root = if side == "input" {
            &operation.input
        } else {
            &operation.output
        };
        let Some(root) = root else {
            return false;
        };
        let service = self.world.service;
        let mut seen = BTreeSet::new();
        let mut pending = vec![root.clone()];
        while let Some(id) = pending.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(shape) = service.shapes.get(&id) else {
                continue;
            };
            if parts(shape).sensitive {
                return true;
            }
            let members: Vec<&crucible_ir::ir::Member> = match shape {
                Shape::Structure(s) | Shape::Union(s) => s.members.iter().collect(),
                Shape::List(l) => vec![&l.member],
                Shape::Map(m) => vec![&m.key, &m.value],
                _ => Vec::new(),
            };
            for member in members {
                if service.sensitive(member) {
                    return true;
                }
                pending.push(member.target.clone());
            }
        }
        false
    }
}

fn equals(left: &Val, right: &Value) -> bool {
    match (left, right) {
        (Val::Str(l), Value::Str(r)) => *l == r.value,
        (Val::Str(l), Value::Ident(r)) => *l == r.name,
        (Val::Num(l), Value::Number(r)) => Num::from_literal(&r.text)
            .and_then(|r| l.compare(r))
            .is_some_and(|ordering| ordering == std::cmp::Ordering::Equal),
        (Val::Bool(l), Value::Ident(r)) => {
            (r.name == "true") == *l && matches!(r.name.as_str(), "true" | "false")
        }
        _ => false,
    }
}
