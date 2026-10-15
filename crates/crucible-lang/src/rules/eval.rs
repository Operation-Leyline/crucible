//! Evaluating a checked condition on one subject.

use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Bounds, Constraints, Shape, Traits};
use serde_json::Value as Json;

use super::metadata::MetaIndex;
use super::subject::{Class, Entity, Subject, World, parts};
use super::vocabulary::{TypeIs, type_is};
use crate::ast::{Expr, ExprKind, Operand, Operator, Path, Ref, Value};

/// A fact the model does not have for this subject, such as the constraints
/// of a shape no operation reaches. The clause skips the subject rather than
/// guess.
#[derive(Debug, Clone, Copy)]
pub(super) struct Unknown;

pub(super) type Tri = Result<bool, Unknown>;

enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
}

pub(super) struct Evaluator<'a, 'w> {
    pub world: &'w World<'a>,
    pub meta: &'w MetaIndex,
}

impl Evaluator<'_, '_> {
    pub(super) fn eval(&self, subject: &Subject<'_>, expr: &Expr) -> Tri {
        match &expr.kind {
            ExprKind::Or(left, right) => {
                Ok(self.eval(subject, left)? || self.eval(subject, right)?)
            }
            ExprKind::And(left, right) => {
                Ok(self.eval(subject, left)? && self.eval(subject, right)?)
            }
            ExprKind::Not(inner) => Ok(!self.eval(subject, inner)?),
            ExprKind::Has(reference) => self.has(subject, reference),
            ExprKind::HasOneOf(references) => {
                for reference in references {
                    if self.has(subject, reference)? {
                        return Ok(true);
                    }
                }
                Ok(false)
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
                return Ok(false);
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
                    Json::Number(n) => n.as_f64().map(Val::Num),
                    _ => None,
                }));
        }
        let rest: Vec<&str> = path.rest.iter().map(|r| r.name.as_str()).collect();
        let bound = |b: &Bounds<u64>, which: &str| match which {
            "min" => b.min.map(|n| Val::Num(n as f64)),
            _ => b.max.map(|n| Val::Num(n as f64)),
        };
        let range = |b: &Bounds<serde_json::Number>, which: &str| match which {
            "min" => b.min.as_ref().and_then(|n| n.as_f64()).map(Val::Num),
            _ => b.max.as_ref().and_then(|n| n.as_f64()).map(Val::Num),
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
                    _ => Some(Val::Num(f64::from(op.status))),
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
                let (Val::Num(left), Value::Number(right)) = (&left, value) else {
                    return Ok(false);
                };
                let Ok(right) = right.text.parse::<f64>() else {
                    return Ok(false);
                };
                match op {
                    Operator::Lt => *left < right,
                    Operator::Le => *left <= right,
                    Operator::Gt => *left > right,
                    _ => *left >= right,
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
                    _ => regex::Regex::new(&right.value).is_ok_and(|re| re.is_match(left)),
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
            TypeIs::Kind(kind) => self
                .world
                .service
                .shapes
                .get(&member.target)
                .and_then(Class::of)
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
            let members: Vec<&crucible_smithy::ir::Member> = match shape {
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
        (Val::Num(l), Value::Number(r)) => r
            .text
            .parse::<f64>()
            .is_ok_and(|r| (*l - r).abs() < f64::EPSILON),
        (Val::Bool(l), Value::Ident(r)) => {
            (r.name == "true") == *l && matches!(r.name.as_str(), "true" | "false")
        }
        _ => false,
    }
}
