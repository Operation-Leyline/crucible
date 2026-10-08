//! Constraint checking. Generated `Validate` impls walk an input and record
//! every violation; a request with any is rejected before an implementation
//! sees it.

use std::collections::BTreeSet;
use std::fmt;

use serde::Serialize;

/// One way a request broke the model, at a `/`-separated path of member
/// names, list indexes and map keys (or a parameter or header name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub path: String,
    pub message: String,
    pub kind: ViolationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// Not readable as the model's types: bad JSON, a word where a number
    /// goes, an unknown enum value.
    Malformed,
    /// Readable, but outside the model: a required member missing, a length,
    /// range or pattern broken.
    Constraint,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

/// A request that could not become an operation's input, with every
/// violation found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    pub violations: Vec<Violation>,
}

impl Rejection {
    pub fn constraint(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::of(ViolationKind::Constraint, path, message)
    }

    pub fn malformed(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::of(ViolationKind::Malformed, path, message)
    }

    fn of(kind: ViolationKind, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            violations: vec![Violation {
                path: path.into(),
                message: message.into(),
                kind,
            }],
        }
    }

    pub fn is_malformed(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.kind == ViolationKind::Malformed)
    }

    /// One line for an error message: the first violation, and how many more.
    pub fn summary(&self) -> String {
        match self.violations.as_slice() {
            [] => "the request is invalid".to_owned(),
            [only] => only.to_string(),
            [first, rest @ ..] => format!("{first} (and {} more)", rest.len()),
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary())
    }
}

impl std::error::Error for Rejection {}

/// A value that checks its own constraints and its members'.
pub trait Validate {
    fn validate(&self, validator: &mut Validator);
}

impl<T: Validate> Validate for Box<T> {
    fn validate(&self, validator: &mut Validator) {
        (**self).validate(validator);
    }
}

/// One step of a violation's path: a member name from the model, a list
/// index, or a map key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Member(&'static str),
    Index(usize),
    Key(String),
}

impl From<&'static str> for Step {
    fn from(name: &'static str) -> Self {
        Self::Member(name)
    }
}

impl From<usize> for Step {
    fn from(index: usize) -> Self {
        Self::Index(index)
    }
}

impl From<&String> for Step {
    fn from(key: &String) -> Self {
        Self::Key(key.clone())
    }
}

impl From<String> for Step {
    fn from(key: String) -> Self {
        Self::Key(key)
    }
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Member(name) => f.write_str(name),
            Self::Index(index) => write!(f, "{index}"),
            Self::Key(key) => f.write_str(key),
        }
    }
}

/// Collects violations while walking a value. The path is built into text
/// only when a violation needs it, so a valid input costs no strings.
#[derive(Default)]
pub struct Validator {
    path: Vec<Step>,
    violations: Vec<Violation>,
}

impl Validator {
    pub fn check<T: Validate>(value: &T) -> Result<(), Rejection> {
        let mut validator = Self::default();
        value.validate(&mut validator);
        if validator.violations.is_empty() {
            Ok(())
        } else {
            Err(Rejection {
                violations: validator.violations,
            })
        }
    }

    pub fn at(&mut self, step: impl Into<Step>, check: impl FnOnce(&mut Self)) {
        self.path.push(step.into());
        check(self);
        self.path.pop();
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        let path = self
            .path
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("/");
        self.violations.push(Violation {
            path,
            message: message.into(),
            kind: ViolationKind::Constraint,
        });
    }

    pub fn length(&mut self, length: usize, min: Option<u64>, max: Option<u64>) {
        let length = length as u64;
        match (min, max) {
            (Some(min), Some(max)) if length < min || length > max => {
                self.fail(format!("length must be between {min} and {max}, inclusive"))
            }
            (Some(min), None) if length < min => {
                self.fail(format!("length must be at least {min}"))
            }
            (None, Some(max)) if length > max => self.fail(format!("length must be at most {max}")),
            _ => {}
        }
    }

    pub fn range_integer(&mut self, value: i128, min: Option<i128>, max: Option<i128>) {
        if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
            self.fail(range_message(
                min.map(|m| m.to_string()),
                max.map(|m| m.to_string()),
            ));
        }
    }

    pub fn range_decimal(
        &mut self,
        value: &crate::number::DecimalValue,
        min: Option<&crate::number::DecimalValue>,
        max: Option<&crate::number::DecimalValue>,
    ) {
        if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
            self.fail(range_message(
                min.map(ToString::to_string),
                max.map(ToString::to_string),
            ));
        }
    }

    pub fn range_float(&mut self, value: f64, min: Option<f64>, max: Option<f64>) {
        if min.is_some_and(|min| value < min) || max.is_some_and(|max| value > max) {
            self.fail(range_message(
                min.map(|m| m.to_string()),
                max.map(|m| m.to_string()),
            ));
        }
    }

    pub fn range_float32(&mut self, value: f32, min: Option<f64>, max: Option<f64>) {
        // A finite modeled bound can round to infinity in f32; that must not
        // make an explicitly infinite value satisfy the constraint.
        if !value.is_finite() {
            self.range_float(f64::from(value), min, max);
            return;
        }
        // Bounds and values must have the same precision so a decimal bound
        // accepts its own f32 representation. Keep the modeled text in errors.
        if min.is_some_and(|min| value < min as f32) || max.is_some_and(|max| value > max as f32) {
            self.fail(range_message(
                min.map(|m| m.to_string()),
                max.map(|m| m.to_string()),
            ));
        }
    }

    /// Check a regex compiled from the shared IR's `Constraints::rust_pattern`.
    /// Keep the modeled ECMA pattern in the error; translation is build-time work.
    pub fn pattern(&mut self, text: &str, regex: &regex::Regex, pattern: &str) {
        if !regex.is_match(text) {
            self.fail(format!("must match the pattern {pattern}"));
        }
    }

    pub fn unique<T: Serialize>(&mut self, items: &[T]) {
        let mut seen = BTreeSet::new();
        for item in items {
            if !seen.insert(crate::codec::key(item).unwrap_or_default()) {
                self.fail("items must be unique");
                return;
            }
        }
    }
}

fn range_message(min: Option<String>, max: Option<String>) -> String {
    match (min, max) {
        (Some(min), Some(max)) => format!("must be between {min} and {max}, inclusive"),
        (Some(min), None) => format!("must be at least {min}"),
        (None, Some(max)) => format!("must be at most {max}"),
        (None, None) => "is out of range".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::Validator;

    #[test]
    fn pattern_reports_ecma_violations_with_the_original_pattern() {
        for (pattern, accepted, rejected) in [
            (r"^\d+$", "12", "١٢"),
            (r"^\w+$", "a_2", "é"),
            (r"^a.b$", "aéb", "a\rb"),
            (r"^a.b$", "aéb", "a\u{2028}b"),
            (r"^\s$", "\u{feff}", "\u{85}"),
            (r"\ba\b", "éaé", "bab"),
        ] {
            let constraints = crucible_smithy::ir::Constraints {
                pattern: Some(pattern.into()),
                ..Default::default()
            };
            let regex = regex::Regex::new(&constraints.rust_pattern().unwrap().unwrap()).unwrap();
            let mut validator = Validator::default();
            validator.at("value", |v| v.pattern(accepted, &regex, pattern));
            assert!(validator.violations.is_empty());
            validator.at("value", |v| v.pattern(rejected, &regex, pattern));
            assert_eq!(validator.violations.len(), 1);
            assert_eq!(validator.violations[0].path, "value");
            assert_eq!(
                validator.violations[0].message,
                format!("must match the pattern {pattern}")
            );
        }
    }

    #[test]
    fn f32_range_accepts_rounded_bounds_and_rejects_adjacent_values() {
        let mut validator = Validator::default();
        validator.range_float32(0.1, None, Some(0.1));
        validator.range_float32(0.7, Some(0.7), None);
        assert!(validator.violations.is_empty());
        validator.range_float32(f32::from_bits(0.1f32.to_bits() + 1), None, Some(0.1));
        validator.range_float32(f32::from_bits(0.7f32.to_bits() - 1), Some(0.7), None);
        assert_eq!(validator.violations.len(), 2);
        assert_eq!(validator.violations[0].message, "must be at most 0.1");
        assert_eq!(validator.violations[1].message, "must be at least 0.7");
        let mut double = Validator::default();
        double.range_float(f64::from_bits(0.1f64.to_bits() + 1), None, Some(0.1));
        double.range_float(f64::from_bits(0.7f64.to_bits() - 1), Some(0.7), None);
        assert_eq!(double.violations.len(), 2);
        let mut non_finite = Validator::default();
        non_finite.range_float32(f32::INFINITY, None, Some(1e100));
        non_finite.range_float32(f32::NEG_INFINITY, Some(-1e100), None);
        assert_eq!(non_finite.violations.len(), 2);
    }
}
