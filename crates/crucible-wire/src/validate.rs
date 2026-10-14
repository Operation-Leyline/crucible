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
/// violation found, up to [`MAX_VIOLATIONS`]. Build one with
/// [`Rejection::new`] or a [`Violations`] collector, which keep that cap.
///
/// The violations are read through [`Rejection::violations`]; a literal
/// cannot build one around the cap:
///
/// ```compile_fail,E0451
/// let rejection = crucible_wire::validate::Rejection { violations: Vec::new() };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    // Private so that every rejection is built through the cap: a public
    // field let a literal or a `push` list any number of violations, and
    // the response, log record and field list copy all of them.
    violations: Vec<Violation>,
}

impl Rejection {
    /// A rejection listing `violations`: the first [`MAX_VIOLATIONS`], then
    /// one more counting the rest.
    pub fn new(violations: impl IntoIterator<Item = Violation>) -> Self {
        let mut collected = Violations::default();
        collected.extend(violations);
        collected.finish()
    }

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

    /// The violations listed: at most [`MAX_VIOLATIONS`], then, when there
    /// were more, one with an empty path counting the rest.
    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    /// The listed violations, owned. See [`Rejection::violations`].
    pub fn into_violations(self) -> Vec<Violation> {
        self.violations
    }

    pub fn is_malformed(&self) -> bool {
        self.violations
            .iter()
            .any(|v| v.kind == ViolationKind::Malformed)
    }

    /// How many violations the request had: those listed, and those the
    /// last line counts when the list reached [`MAX_VIOLATIONS`].
    pub fn count(&self) -> usize {
        match self.violations.split_last() {
            // Only a full list ends with the count of the rest.
            Some((last, listed)) if listed.len() == MAX_VIOLATIONS => match unlisted(last) {
                Some(unlisted) => listed.len() + unlisted,
                None => self.violations.len(),
            },
            _ => self.violations.len(),
        }
    }

    /// One line for an error message: the first violation, and how many more.
    pub fn summary(&self) -> String {
        match self.violations.first() {
            None => "the request is invalid".to_owned(),
            Some(first) => match self.count() {
                0 | 1 => first.to_string(),
                count => format!("{first} (and {} more)", count - 1),
            },
        }
    }
}

/// The words of the line that ends a capped list, after its count.
const UNLISTED: &str = " more violations are not listed";

/// The violations an overflow line counts, when `violation` is one.
fn unlisted(violation: &Violation) -> Option<usize> {
    if !violation.path.is_empty() || violation.kind != ViolationKind::Constraint {
        return None;
    }
    violation.message.strip_suffix(UNLISTED)?.parse().ok()
}

/// Violations as they are found, at most [`MAX_VIOLATIONS`] of them listed
/// and the rest counted. Every producer of a [`Rejection`] collects through
/// one, so no request can make a rejection (and the response, log record
/// and field list that copy it) grow with the request.
#[derive(Debug, Default)]
pub struct Violations {
    listed: Vec<Violation>,
    unlisted: usize,
}

impl Violations {
    /// Whether another violation would only be counted: a caller can skip
    /// building its path and message.
    pub fn is_full(&self) -> bool {
        self.listed.len() >= MAX_VIOLATIONS
    }

    pub fn push(&mut self, violation: Violation) {
        if self.is_full() {
            self.unlisted += 1;
        } else {
            self.listed.push(violation);
        }
    }

    /// Records the violation `make` builds, or, once the list is full,
    /// only counts it: a request past the cap costs no more strings.
    pub fn push_with(&mut self, make: impl FnOnce() -> Violation) {
        if self.is_full() {
            self.unlisted += 1;
        } else {
            self.listed.push(make());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.listed.is_empty()
    }

    /// The violations listed so far.
    pub fn listed(&self) -> &[Violation] {
        &self.listed
    }

    /// The rejection these violations make, or `None` for none.
    pub fn into_rejection(self) -> Option<Rejection> {
        (!self.is_empty()).then(|| self.finish())
    }

    fn finish(mut self) -> Rejection {
        if self.unlisted > 0 {
            self.listed.push(Violation {
                path: String::new(),
                message: format!("{}{UNLISTED}", self.unlisted),
                kind: ViolationKind::Constraint,
            });
        }
        Rejection {
            violations: self.listed,
        }
    }
}

impl Extend<Violation> for Violations {
    fn extend<I: IntoIterator<Item = Violation>>(&mut self, violations: I) {
        for violation in violations {
            self.push(violation);
        }
    }
}

impl FromIterator<Violation> for Violations {
    fn from_iter<I: IntoIterator<Item = Violation>>(violations: I) -> Self {
        let mut collected = Self::default();
        collected.extend(violations);
        collected
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
    path: Steps,
    violations: Violations,
    /// How many [`Validator::sensitive`] scopes enclose the value checked.
    sensitive: usize,
}

/// The step a map key takes inside a sensitive scope.
const HIDDEN_KEY: &str = "[key]";

/// The violations a rejection lists. Each is a path and a message that the
/// response, the service log and the error's field list all copy, and one
/// broken constraint on a list's items fails once per item: a 10 MiB body of
/// empty strings would otherwise list millions. Past this many, a rejection
/// ends with one more violation counting the rest.
pub const MAX_VIOLATIONS: usize = 100;

/// Steps kept inline before spilling to the heap: deeper than most inputs go.
const INLINE_STEPS: usize = 8;

/// The path to the value being checked. Its first steps live inline, so
/// walking a valid input allocates nothing for the path either.
#[derive(Default)]
struct Steps {
    inline: [Option<Step>; INLINE_STEPS],
    len: usize,
    spill: Vec<Step>,
}

impl Steps {
    fn push(&mut self, step: Step) {
        if self.len < INLINE_STEPS {
            self.inline[self.len] = Some(step);
        } else {
            self.spill.push(step);
        }
        self.len += 1;
    }

    fn pop(&mut self) {
        self.len -= 1;
        if self.len < INLINE_STEPS {
            self.inline[self.len] = None;
        } else {
            self.spill.pop();
        }
    }

    fn iter(&self) -> impl Iterator<Item = &Step> {
        self.inline.iter().flatten().chain(&self.spill)
    }
}

impl Validator {
    pub fn check<T: Validate>(value: &T) -> Result<(), Rejection> {
        let mut validator = Self::default();
        value.validate(&mut validator);
        match validator.violations.into_rejection() {
            None => Ok(()),
            Some(rejection) => Err(rejection),
        }
    }

    pub fn at(&mut self, step: impl Into<Step>, check: impl FnOnce(&mut Self)) {
        let step = match step.into() {
            Step::Key(_) if self.sensitive > 0 => Step::Member(HIDDEN_KEY),
            step => step,
        };
        self.path.push(step);
        check(self);
        self.path.pop();
    }

    /// Checks a sensitive value: a map key below it is client data, so a
    /// violation's path names it `[key]`. Member names and list indexes come
    /// from the model and the value's shape, and stay. Generated code opens
    /// this for a `@sensitive` member, a sensitive structure's members and
    /// the entries of a map whose keys are sensitive; a structure reached
    /// through such a member is covered too, since the scope is the
    /// validator's, not the structure's.
    pub fn sensitive(&mut self, check: impl FnOnce(&mut Self)) {
        self.sensitive += 1;
        check(self);
        self.sensitive -= 1;
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        let path = &self.path;
        // The path is built into text only for a violation that is listed.
        self.violations.push_with(|| Violation {
            path: path
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("/"),
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
            assert!(validator.violations.listed().is_empty());
            validator.at("value", |v| v.pattern(rejected, &regex, pattern));
            assert_eq!(validator.violations.listed().len(), 1);
            assert_eq!(validator.violations.listed()[0].path, "value");
            assert_eq!(
                validator.violations.listed()[0].message,
                format!("must match the pattern {pattern}")
            );
        }
    }

    #[test]
    fn a_rejection_lists_at_most_max_violations_and_counts_the_rest() {
        struct Items(usize);
        impl super::Validate for Items {
            fn validate(&self, validator: &mut Validator) {
                for index in 0..self.0 {
                    validator.at("items", |v| v.at(index, |v| v.length(0, Some(1), None)));
                }
            }
        }
        let rejection = Validator::check(&Items(super::MAX_VIOLATIONS)).unwrap_err();
        assert_eq!(rejection.violations().len(), super::MAX_VIOLATIONS);
        assert_eq!(rejection.violations()[99].path, "items/99");

        let rejection = Validator::check(&Items(1_000_000)).unwrap_err();
        assert_eq!(rejection.violations().len(), super::MAX_VIOLATIONS + 1);
        assert_eq!(rejection.violations()[0].path, "items/0");
        assert_eq!(
            rejection.violations()[0].message,
            "length must be at least 1"
        );
        let last = rejection.violations().last().unwrap();
        assert_eq!(last.path, "");
        assert_eq!(last.message, "999900 more violations are not listed");
        assert_eq!(rejection.count(), 1_000_000);
        assert_eq!(
            rejection.summary(),
            "items/0: length must be at least 1 (and 999999 more)"
        );
    }

    #[test]
    fn map_keys_inside_a_sensitive_scope_are_not_named() {
        let mut validator = Validator::default();
        let key = "hunter2".to_owned();
        validator.at("public", |v| v.at(&key, |v| v.fail("broken")));
        validator.at("private", |v| {
            v.sensitive(|v| v.at(&key, |v| v.at(3, |v| v.at("name", |v| v.fail("broken")))));
        });
        // The scope closes with its closure.
        validator.at("after", |v| v.at(&key, |v| v.fail("broken")));
        let paths: Vec<_> = validator
            .violations
            .listed()
            .iter()
            .map(|v| v.path.as_str())
            .collect();
        assert_eq!(
            paths,
            ["public/hunter2", "private/[key]/3/name", "after/hunter2"]
        );
    }

    #[test]
    fn every_rejection_is_capped_and_its_summary_counts_the_unlisted() {
        let violation = |index: usize| super::Violation {
            path: format!("p{index}"),
            message: "broken".to_owned(),
            kind: super::ViolationKind::Malformed,
        };
        let rejection = super::Rejection::new((0..150).map(violation));
        assert_eq!(rejection.violations().len(), super::MAX_VIOLATIONS + 1);
        assert_eq!(rejection.count(), 150);
        assert_eq!(rejection.summary(), "p0: broken (and 149 more)");
        let last = rejection.violations().last().unwrap();
        assert_eq!(last.message, "50 more violations are not listed");

        let rejection = super::Rejection::new((0..3).map(violation));
        assert_eq!(rejection.count(), 3);
        assert_eq!(rejection.summary(), "p0: broken (and 2 more)");
        let rejection = super::Rejection::new((0..1).map(violation));
        assert_eq!(rejection.summary(), "p0: broken");
        assert_eq!(
            super::Rejection::new([]).summary(),
            "the request is invalid"
        );

        let collected: super::Violations = (0..super::MAX_VIOLATIONS).map(violation).collect();
        assert!(collected.is_full());
        assert!(super::Violations::default().into_rejection().is_none());
    }

    #[test]
    fn f32_range_accepts_rounded_bounds_and_rejects_adjacent_values() {
        let mut validator = Validator::default();
        validator.range_float32(0.1, None, Some(0.1));
        validator.range_float32(0.7, Some(0.7), None);
        assert!(validator.violations.listed().is_empty());
        validator.range_float32(f32::from_bits(0.1f32.to_bits() + 1), None, Some(0.1));
        validator.range_float32(f32::from_bits(0.7f32.to_bits() - 1), Some(0.7), None);
        assert_eq!(validator.violations.listed().len(), 2);
        assert_eq!(
            validator.violations.listed()[0].message,
            "must be at most 0.1"
        );
        assert_eq!(
            validator.violations.listed()[1].message,
            "must be at least 0.7"
        );
        let mut double = Validator::default();
        double.range_float(f64::from_bits(0.1f64.to_bits() + 1), None, Some(0.1));
        double.range_float(f64::from_bits(0.7f64.to_bits() - 1), Some(0.7), None);
        assert_eq!(double.violations.listed().len(), 2);
        let mut non_finite = Validator::default();
        non_finite.range_float32(f32::INFINITY, None, Some(1e100));
        non_finite.range_float32(f32::NEG_INFINITY, Some(-1e100), None);
        assert_eq!(non_finite.violations.listed().len(), 2);
    }
}
