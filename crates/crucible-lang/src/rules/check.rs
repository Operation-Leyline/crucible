//! Checking a rule against the vocabulary before it runs: a condition that
//! does not apply to its kind, an unknown metadata key, a value the key does
//! not have, an operator the value cannot take. A rule that matches nothing
//! silently is the failure this prevents (decision 46).

use crucible_ir::decimal::Decimal;

use super::metadata::{MetaIndex, MetaTy, applies_to};
use super::subject::World;
use super::vocabulary::{
    BUILT_IN, FLAGS, Kind, PathType, built_in_applies, flag_applies, flags_for, has_applies,
    path_type, type_is,
};
use crate::ast::{Expr, ExprKind, Operand, Operator, Path, Ref, Value};
use crate::diagnostic::Diagnostic;
use crate::span::Span;

pub(super) const ROLES: &[&str] = &[
    "creates", "puts", "reads", "updates", "deletes", "lists", "on",
];

pub(super) struct Checker<'a, 'w> {
    pub meta: &'w MetaIndex,
    pub world: &'w World<'a>,
    pub problems: Vec<Diagnostic>,
}

impl Checker<'_, '_> {
    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.problems.push(Diagnostic::new(span, message));
    }

    /// Checks `expression` as a condition on elements of `kind`.
    pub(super) fn expr(&mut self, kind: Kind, expression: &Expr) {
        match &expression.kind {
            ExprKind::Or(a, b) | ExprKind::And(a, b) => {
                self.expr(kind, a);
                self.expr(kind, b);
            }
            ExprKind::Not(inner) => self.expr(kind, inner),
            ExprKind::Has(reference) => self.has(kind, reference),
            ExprKind::HasOneOf(references) => {
                for reference in references {
                    self.has(kind, reference);
                }
            }
            ExprKind::Addresses(resource) => {
                if kind != Kind::Operation {
                    self.error(
                        expression.span,
                        format!(
                            "`addresses` reads the resource an operation is bound to, not {}",
                            kind.indefinite()
                        ),
                    );
                } else if !resource.name.is_empty() && !self.world.has_resource(&resource.name) {
                    self.error(
                        resource.span,
                        format!("`{}` is not a resource", resource.name),
                    );
                }
            }
            ExprKind::Flag(path) => self.flag(kind, path),
            ExprKind::Compare { path, op, operand } => self.compare(kind, path, *op, operand),
        }
    }

    /// The type of `@key` on `kind`, or an error naming what is wrong.
    fn metadata(&mut self, kind: Kind, reference: &Ref) -> Option<MetaTy> {
        if reference.name.name.is_empty() {
            return None;
        }
        let name = &reference.name.name;
        if BUILT_IN.contains(&name.as_str()) && !self.meta.contains_key(name) {
            if !built_in_applies(name, kind) {
                // `@deprecated` fits everything but a resource; the others
                // are member annotations.
                let message = if name == "deprecated" {
                    format!(
                        "`@deprecated` is not on {}: resources take no annotations",
                        kind.indefinite()
                    )
                } else {
                    format!("`@{name}` is on a member, not {}", kind.indefinite())
                };
                self.error(reference.name.span, message);
                return None;
            }
            return Some(MetaTy::Flag);
        }
        let Some(info) = self.meta.get(name) else {
            let known: Vec<&str> = self.meta.keys().map(String::as_str).collect();
            self.error(
                reference.name.span,
                format!(
                    "`@{name}` is not declared with `metadata`; the keys are {}",
                    if known.is_empty() {
                        "none".to_owned()
                    } else {
                        known.join(", ")
                    }
                ),
            );
            return None;
        };
        if kind == Kind::Resource {
            self.error(
                reference.name.span,
                "a rule cannot read metadata on a resource",
            );
            return None;
        }
        if !applies_to(&info.targets, kind) {
            let targets = info.targets.join(", ");
            self.error(
                reference.name.span,
                format!(
                    "`@{name}` is declared on {targets}, which {} is not",
                    kind.indefinite()
                ),
            );
            return None;
        }
        Some(info.ty.clone())
    }

    fn has(&mut self, kind: Kind, reference: &Ref) {
        if reference.name.name.is_empty() {
            return;
        }
        if reference.metadata {
            self.metadata(kind, reference);
        } else if !has_applies(&reference.name.name, kind) {
            self.error(
                reference.name.span,
                format!(
                    "`has {}` does not apply to {}: `has` takes `@metadata`, or `length`, `range` or `pattern` of a string, number, list, map, blob or member",
                    reference.name.name,
                    kind.indefinite()
                ),
            );
        }
    }

    fn flag(&mut self, kind: Kind, path: &Path) {
        if path.head.name.name.is_empty() {
            return;
        }
        if path.head.metadata {
            if path.rest.is_empty() {
                self.metadata(kind, &path.head);
            } else {
                self.error(path.span, "a metadata key has no parts to read");
            }
            return;
        }
        let name = path.head.name.name.as_str();
        if path.rest.is_empty() && FLAGS.contains(&name) {
            if !flag_applies(name, kind) {
                self.error(
                    path.span,
                    format!(
                        "`{name}` does not apply to {}; its flags are {}",
                        kind.indefinite(),
                        flags_for(kind)
                    ),
                );
            }
            return;
        }
        let rest: Vec<&str> = path.rest.iter().map(|r| r.name.as_str()).collect();
        match path_type(name, &rest, kind) {
            Ok(_) => self.error(
                path.span,
                format!(
                    "`{}` has a value, not a yes or no: compare it, such as `{} == ..`",
                    spelled(path),
                    spelled(path)
                ),
            ),
            Err(message) => self.error(path.span, message),
        }
    }

    fn compare(&mut self, kind: Kind, path: &Path, op: Operator, operand: &Operand) {
        if path.head.name.name.is_empty() {
            return;
        }
        let rest: Vec<&str> = path.rest.iter().map(|r| r.name.as_str()).collect();
        let ty = if path.head.metadata {
            if !path.rest.is_empty() {
                self.error(path.span, "a metadata key has no parts to read");
                return;
            }
            let Some(ty) = self.metadata(kind, &path.head) else {
                return;
            };
            match ty {
                MetaTy::Flag => {
                    self.error(
                        path.span,
                        format!(
                            "`@{}` has no value to compare; test it with `has @{0}`",
                            path.head.name.name
                        ),
                    );
                    return;
                }
                MetaTy::Other => {
                    self.error(
                        path.span,
                        format!(
                            "`@{}` is a structure; a rule can test that it is there with `has`, but not compare it",
                            path.head.name.name
                        ),
                    );
                    return;
                }
                MetaTy::Enum(values) => PathType::Enum(values),
                MetaTy::Str => PathType::Str,
                MetaTy::Num => PathType::Num,
                MetaTy::Bool => PathType::Bool,
            }
        } else {
            match path_type(&path.head.name.name, &rest, kind) {
                Ok(ty) => ty,
                Err(message) => {
                    self.error(path.span, message);
                    return;
                }
            }
        };
        if op == Operator::Is {
            self.is(&ty, path, operand);
            return;
        }
        let Operand::Value(value) = operand else {
            // The parser said a value is missing.
            return;
        };
        if let PathType::Body = ty {
            let sensitive = matches!(value, Value::Ident(i) if i.name == "sensitive");
            if op != Operator::Contains || !sensitive {
                self.error(
                    path.span,
                    format!(
                        "a body can only be tested with `{} contains sensitive`",
                        spelled(path)
                    ),
                );
            }
            return;
        }
        let allowed: &[Operator] = match &ty {
            PathType::Str | PathType::Type => &[
                Operator::Eq,
                Operator::Ne,
                Operator::In,
                Operator::StartsWith,
                Operator::EndsWith,
                Operator::Matches,
                Operator::Contains,
            ],
            PathType::Num => &[
                Operator::Eq,
                Operator::Ne,
                Operator::Lt,
                Operator::Le,
                Operator::Gt,
                Operator::Ge,
                Operator::In,
            ],
            PathType::Enum(_) | PathType::Bool => &[Operator::Eq, Operator::Ne, Operator::In],
            PathType::Body => &[],
        };
        if !allowed.contains(&op) {
            self.error(
                path.span,
                format!(
                    "`{}` cannot be compared with {}",
                    spelled(path),
                    operator_word(op)
                ),
            );
            return;
        }
        if op == Operator::In {
            match value {
                Value::List(items, _) => {
                    for item in items {
                        self.operand(&ty, path, item);
                    }
                }
                other => self.error(other.span(), "`in` takes a list, such as `[a, b]`"),
            }
            return;
        }
        let string_operator = matches!(
            op,
            Operator::StartsWith | Operator::EndsWith | Operator::Matches | Operator::Contains
        );
        if string_operator && !matches!(value, Value::Str(_)) {
            self.error(
                value.span(),
                format!("{} takes a string in quotes", operator_word(op)),
            );
            return;
        }
        if op == Operator::Matches
            && let Value::Str(pattern) = value
            && let Err(error) = regex::Regex::new(&pattern.value)
        {
            self.error(pattern.span, format!("the pattern does not run: {error}"));
        }
        if !string_operator {
            self.operand(&ty, path, value);
        }
    }

    /// One value on the right of `==`, `!=` or in an `in` list.
    fn operand(&mut self, ty: &PathType, path: &Path, value: &Value) {
        let word = match value {
            Value::Ident(i) => Some(i.name.as_str()),
            Value::Str(s) => Some(s.value.as_str()),
            _ => None,
        };
        match ty {
            PathType::Num => match value {
                Value::Number(n) if Decimal::from_text(&n.text).is_none() => self.error(
                    value.span(),
                    format!("`{}` is out of range: its exponent is too large", n.text),
                ),
                Value::Number(_) => {}
                _ => self.error(value.span(), format!("`{}` is a number", spelled(path))),
            },
            PathType::Enum(values) => {
                if !word.is_some_and(|w| values.iter().any(|v| v == w)) {
                    self.error(
                        value.span(),
                        format!("`{}` is one of {}", spelled(path), values.join(", ")),
                    );
                }
            }
            PathType::Bool => {
                if !matches!(word, Some("true" | "false")) {
                    self.error(
                        value.span(),
                        format!("`{}` is true or false", spelled(path)),
                    );
                }
            }
            PathType::Str | PathType::Type => {
                if word.is_none() {
                    self.error(
                        value.span(),
                        format!("`{}` is a word or a string", spelled(path)),
                    );
                } else if spelled(path) == "role"
                    && let Some(role) = word
                    && !ROLES.contains(&role)
                {
                    self.error(
                        value.span(),
                        format!("`{role}` is not a role; the roles are {}", ROLES.join(", ")),
                    );
                }
            }
            PathType::Body => {}
        }
    }

    fn is(&mut self, ty: &PathType, path: &Path, operand: &Operand) {
        if *ty != PathType::Type {
            self.error(
                path.span,
                format!("`is` tests a member's `type`, not `{}`", spelled(path)),
            );
            return;
        }
        let Operand::Words(words) = operand else {
            return;
        };
        let words: Vec<&str> = words.iter().map(|w| w.name.as_str()).collect();
        if !words.is_empty() && type_is(&words).is_none() {
            let span = path.span;
            self.error(
                span,
                format!(
                    "`type is {}` names no kind of type; use `inline primitive`, or one of {}",
                    words.join(" "),
                    Kind::all_words()
                ),
            );
        }
    }
}

/// A path as written.
fn spelled(path: &Path) -> String {
    let mut out = String::new();
    if path.head.metadata {
        out.push('@');
    }
    out.push_str(&path.head.name.name);
    for part in &path.rest {
        out.push('.');
        out.push_str(&part.name);
    }
    out
}

fn operator_word(op: Operator) -> &'static str {
    match op {
        Operator::Eq => "`==`",
        Operator::Ne => "`!=`",
        Operator::Lt => "`<`",
        Operator::Le => "`<=`",
        Operator::Gt => "`>`",
        Operator::Ge => "`>=`",
        Operator::In => "`in`",
        Operator::Is => "`is`",
        Operator::StartsWith => "`startsWith`",
        Operator::EndsWith => "`endsWith`",
        Operator::Matches => "`matches`",
        Operator::Contains => "`contains`",
    }
}
