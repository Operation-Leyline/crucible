//! `examples`: checked against the lowered input, output and error shapes, so
//! an example is held to the same member, type and constraint rules as a value
//! on the wire.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde_json::{Map, Number, Value};

use crate::decimal::{Decimal, MAX_BIG_INTEGER_DIGITS};
use crate::ir::{
    Constraints, Member, Service, Shape, Simple, SimpleKind, Structure, TimestampFormat,
};
use crate::shape_id::ShapeId;

/// Checks model values against lowered shapes: examples and defaults.
pub(crate) struct Check<'a> {
    pub(crate) shapes: &'a BTreeMap<ShapeId, Shape>,
    pub(crate) allow_constraint_errors: bool,
    pub(crate) problems: Vec<String>,
}

impl Check<'_> {
    /// An operation's input, output or error, where no shape means Unit.
    pub(crate) fn io(&mut self, path: &str, shape: Option<&ShapeId>, value: &Value) {
        if let Some(id) = shape
            && matches!(self.shapes.get(id), Some(Shape::Union(_)))
        {
            self.value(path, id, &Constraints::default(), value);
            return;
        }
        let Value::Object(members) = value else {
            self.problems
                .push(format!("{path} must be an object, not {}", kind(value)));
            return;
        };
        match shape.and_then(|shape| self.shapes.get(shape)) {
            Some(Shape::Structure(structure)) => self.structure(path, structure, members),
            _ if members.is_empty() => {}
            _ => self
                .problems
                .push(format!("{path} must be empty: the operation declares none")),
        }
    }

    /// Violations `allowConstraintErrors` permits.
    fn constraint(&mut self, problem: String) {
        if !self.allow_constraint_errors {
            self.problems.push(problem);
        }
    }

    fn structure(&mut self, path: &str, shape: &Structure, members: &Map<String, Value>) {
        for (name, value) in members {
            match shape.member(name) {
                // A document's null is a value of its own, distinct on the
                // wire from leaving the member out. Nullability is the
                // server's: an example is a request the server must accept,
                // and it refuses null for a member it requires.
                Some(member)
                    if value.is_null()
                        && member.optional_on_server()
                        && self.is_document(&member.target) => {}
                Some(member) => self.member(&format!("{path}.{name}"), member, value, false),
                None => self.problems.push(format!("{path}.{name} is not a member")),
            }
        }
        for member in &shape.members {
            if member.required_without_default() && !members.contains_key(&member.name) {
                self.constraint(format!("{path}.{} is required", member.name));
            }
        }
    }

    fn is_document(&self, target: &ShapeId) -> bool {
        matches!(
            self.shapes.get(target),
            Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Document
        )
    }

    fn member(&mut self, path: &str, member: &Member, value: &Value, sparse: bool) {
        if value.is_null() {
            if !sparse {
                self.problems.push(format!("{path} must not be null"));
            }
            return;
        }
        self.value(path, &member.target, &member.constraints, value);
    }

    pub(crate) fn value(
        &mut self,
        path: &str,
        target: &ShapeId,
        constraints: &Constraints,
        value: &Value,
    ) {
        let Some(shape) = self.shapes.get(target) else {
            return;
        };
        match shape {
            Shape::Unit { .. } => {
                if value.as_object().is_none_or(|o| !o.is_empty()) {
                    self.problems
                        .push(format!("{path} must be an empty object"));
                }
            }
            Shape::Simple(simple) => self.simple(path, simple, constraints, value),
            Shape::Enum(enumeration) => match value.as_str() {
                Some(text) if enumeration.values.iter().any(|v| v.value == text) => {}
                Some(text) => self
                    .problems
                    .push(format!("{path}: {text:?} is not a value of {target}")),
                None => self.mismatch(path, "string", value),
            },
            Shape::IntEnum(enumeration) => match value.as_i64() {
                Some(n) if enumeration.values.iter().any(|v| i64::from(v.value) == n) => {}
                Some(n) => self
                    .problems
                    .push(format!("{path}: {n} is not a value of {target}")),
                None => self.mismatch(path, "integer", value),
            },
            Shape::List(list) => {
                let Some(items) = value.as_array() else {
                    return self.mismatch(path, "array", value);
                };
                self.length(path, constraints, items.len() as u64, "items");
                if constraints.unique_items
                    && items
                        .iter()
                        .enumerate()
                        .any(|(i, item)| items[..i].contains(item))
                {
                    self.constraint(format!("{path} must not repeat items"));
                }
                for (index, item) in items.iter().enumerate() {
                    self.member(&format!("{path}[{index}]"), &list.member, item, list.sparse);
                }
            }
            Shape::Map(map) => {
                let Some(entries) = value.as_object() else {
                    return self.mismatch(path, "object", value);
                };
                self.length(path, constraints, entries.len() as u64, "entries");
                for (key, entry) in entries {
                    self.value(
                        &format!("{path} key {key:?}"),
                        &map.key.target,
                        &map.key.constraints,
                        &Value::String(key.clone()),
                    );
                    self.member(&format!("{path}.{key}"), &map.value, entry, map.sparse);
                }
            }
            Shape::Structure(structure) => match value.as_object() {
                Some(members) => self.structure(path, structure, members),
                None => self.mismatch(path, "object", value),
            },
            Shape::Union(union) => {
                let Some(members) = value.as_object() else {
                    return self.mismatch(path, "object", value);
                };
                if members.len() != 1 {
                    self.problems
                        .push(format!("{path} must set exactly one union member"));
                }
                for (name, value) in members {
                    match union.member(name) {
                        Some(member) => {
                            self.member(&format!("{path}.{name}"), member, value, false)
                        }
                        None => self.problems.push(format!("{path}.{name} is not a member")),
                    }
                }
            }
        }
    }

    fn simple(&mut self, path: &str, simple: &Simple, constraints: &Constraints, value: &Value) {
        match simple.kind {
            SimpleKind::Document => {}
            SimpleKind::Boolean if value.is_boolean() => {}
            SimpleKind::Boolean => self.mismatch(path, "boolean", value),
            SimpleKind::String => {
                let Some(text) = value.as_str() else {
                    return self.mismatch(path, "string", value);
                };
                self.length(path, constraints, text.chars().count() as u64, "characters");
                // A pattern the regex engine cannot run is diagnosed where the
                // pattern is used, so only this check is skipped.
                if let Ok(Some(pattern)) = constraints.rust_pattern()
                    && let Ok(pattern) = regex::Regex::new(&pattern)
                    && !pattern.is_match(text)
                {
                    self.constraint(format!(
                        "{path} does not match the pattern {:?}",
                        constraints.pattern.as_deref().unwrap_or_default()
                    ));
                }
            }
            SimpleKind::Blob => {
                let Some(text) = value.as_str() else {
                    return self.mismatch(path, "base64 string", value);
                };
                match base64::engine::general_purpose::STANDARD.decode(text) {
                    Ok(bytes) => self.length(path, constraints, bytes.len() as u64, "bytes"),
                    Err(_) => self.problems.push(format!("{path} must be valid base64")),
                }
            }
            SimpleKind::Timestamp => {
                let valid = matches!(value, Value::Number(_) | Value::String(_))
                    && crate::timestamp_default::wire_value(value, TimestampFormat::DateTime)
                        .is_some();
                if !valid {
                    self.problems.push(format!(
                        "{path} must be an RFC 3339 string or epoch seconds"
                    ));
                }
            }
            SimpleKind::Float | SimpleKind::Double => {
                let float = simple.kind == SimpleKind::Float;
                let read = match value {
                    // A body number is read as serde_json reads an `f64`,
                    // then narrowed for a `float`; either past its range
                    // is refused.
                    Value::Number(number) => serde_json::from_str::<f64>(&number.to_string())
                        .ok()
                        .map(|read| if float { f64::from(read as f32) } else { read })
                        .filter(|read| read.is_finite()),
                    // The server reads only JSON numbers: `"NaN"`,
                    // `"Infinity"` and `"-Infinity"` are refused too.
                    Value::String(_) => {
                        return self
                            .problems
                            .push(format!("{path} must be a finite number, not {value}"));
                    }
                    _ => return self.mismatch(path, "number", value),
                };
                match read {
                    Some(read) => self.float_range(path, float, constraints, read, value),
                    None => self
                        .problems
                        .push(format!("{path}: {value} is outside the range of the type")),
                }
            }
            SimpleKind::BigDecimal => match value {
                Value::Number(number) => self.range(path, constraints, number),
                _ => self.mismatch(path, "number", value),
            },
            // The server's `BigInteger` reads any whole number, `1e2`,
            // `100e-2` and `1.0` included, of up to 10,000 digits.
            SimpleKind::BigInteger => match value {
                Value::Number(number) => match Decimal::parse(number) {
                    Some(integer) if !integer.is_integer() => self.mismatch(path, "integer", value),
                    Some(integer) if integer.integer_digits() <= MAX_BIG_INTEGER_DIGITS => {
                        self.range(path, constraints, number)
                    }
                    _ => self.problems.push(format!(
                        "{path}: {value} is outside the range of the type \
                         ({MAX_BIG_INTEGER_DIGITS} digits at most)"
                    )),
                },
                _ => self.mismatch(path, "integer", value),
            },
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                let (low, high) = match simple.kind {
                    SimpleKind::Byte => (i128::from(i8::MIN), i128::from(i8::MAX)),
                    SimpleKind::Short => (i128::from(i16::MIN), i128::from(i16::MAX)),
                    SimpleKind::Integer => (i128::from(i32::MIN), i128::from(i32::MAX)),
                    _ => (i128::from(i64::MIN), i128::from(i64::MAX)),
                };
                // The server reads an integer kind only from plain digits.
                // (A model's `-0`, which it reads as the float -0.0, never
                // gets here: the IR's numbers hold it as `0`.)
                let integer = match value {
                    Value::Number(number) if !number.to_string().contains(['.', 'e', 'E']) => {
                        Decimal::parse(number)
                    }
                    _ => None,
                };
                let Some(integer) = integer else {
                    return self.mismatch(path, "integer", value);
                };
                if integer < Decimal::integer(low) || integer > Decimal::integer(high) {
                    self.problems.push(format!(
                        "{path}: {value} is outside the range of the type ({low} to {high})"
                    ));
                } else if let Value::Number(number) = value {
                    self.range(path, constraints, number);
                }
            }
        }
    }

    fn length(&mut self, path: &str, constraints: &Constraints, actual: u64, unit: &str) {
        let Some(bounds) = &constraints.length else {
            return;
        };
        if bounds.min.is_some_and(|min| actual < min) || bounds.max.is_some_and(|max| actual > max)
        {
            self.constraint(format!(
                "{path} has {actual} {unit}, outside its length bounds ({}..{})",
                bounds.min.map_or(String::new(), |n| n.to_string()),
                bounds.max.map_or(String::new(), |n| n.to_string())
            ));
        }
    }

    /// An exact kind against its range: every integer kind and
    /// `bigDecimal`, which the server compares exactly. An integer kind's
    /// server bounds are [`crate::ir::Bounds::integer_bounds`], which
    /// accept exactly the integers these do.
    fn range(&mut self, path: &str, constraints: &Constraints, number: &Number) {
        let Some(bounds) = &constraints.range else {
            return;
        };
        // Only an exponent past what any decimal holds fails to parse.
        let Some(value) = Decimal::parse(number) else {
            return self
                .problems
                .push(format!("{path}: {number} is outside the range of the type"));
        };
        let bound = |bound: &Option<Number>| bound.as_ref().and_then(Decimal::parse);
        let below = bound(&bounds.min).is_some_and(|min| value < min);
        let above = bound(&bounds.max).is_some_and(|max| value > max);
        if below || above {
            self.constraint(format!("{path}: {number} is outside its range"));
        }
    }

    /// A finite `float` or `double` against its range, in the server's
    /// IEEE arithmetic: a `float` is compared in `f32` with each bound
    /// narrowed, a `double` in `f64`.
    fn float_range(
        &mut self,
        path: &str,
        float: bool,
        constraints: &Constraints,
        read: f64,
        value: &Value,
    ) {
        let Some(bounds) = &constraints.range else {
            return;
        };
        let (min, max) = bounds.float_bounds();
        let outside = if float {
            let read = read as f32;
            min.is_some_and(|min| read < min as f32) || max.is_some_and(|max| read > max as f32)
        } else {
            min.is_some_and(|min| read < min) || max.is_some_and(|max| read > max)
        };
        if outside {
            self.constraint(format!("{path}: {value} is outside its range"));
        }
    }

    fn mismatch(&mut self, path: &str, expected: &str, value: &Value) {
        self.problems
            .push(format!("{path} must be a {expected}, not {}", kind(value)));
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

impl Service {
    /// A validated example value for `member`, as a document carries it: JSON
    /// names for members and the member's timestamp format. `format` is the
    /// HTTP binding's timestamp format, which overrides it at the top level.
    pub fn wire_example(
        &self,
        member: &Member,
        value: &Value,
        format: Option<TimestampFormat>,
    ) -> Value {
        let Some(shape) = self.shape(&member.target) else {
            return value.clone();
        };
        match (shape, value) {
            // A `bigInteger` may be written `1e2`; documents and requests
            // carry its digits.
            (Shape::Simple(simple), Value::Number(number))
                if simple.kind == SimpleKind::BigInteger =>
            {
                Decimal::parse(number)
                    .and_then(|integer| integer.integer_text())
                    .and_then(|text| text.parse().ok())
                    .map_or_else(|| value.clone(), Value::Number)
            }
            (Shape::Simple(simple), _) if simple.kind == SimpleKind::Timestamp => {
                let format = format.unwrap_or_else(|| self.document_timestamp_format(member));
                crate::timestamp_default::wire_value(value, format).unwrap_or_else(|| value.clone())
            }
            (Shape::List(list), Value::Array(items)) => Value::Array(
                items
                    .iter()
                    .map(|item| self.wire_example(&list.member, item, format))
                    .collect(),
            ),
            (Shape::Map(map), Value::Object(entries)) => Value::Object(
                entries
                    .iter()
                    .map(|(key, entry)| (key.clone(), self.wire_example(&map.value, entry, None)))
                    .collect(),
            ),
            (Shape::Structure(structure) | Shape::Union(structure), Value::Object(members)) => {
                Value::Object(
                    members
                        .iter()
                        .map(|(name, value)| match structure.member(name) {
                            Some(member) => {
                                (member.name.clone(), self.wire_example(member, value, None))
                            }
                            None => (name.clone(), value.clone()),
                        })
                        .collect(),
                )
            }
            _ => value.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Bounds;

    /// Problems with `value` as a `kind` member ranged `min..max`.
    fn problems(
        kind: SimpleKind,
        min: Option<&str>,
        max: Option<&str>,
        value: &str,
    ) -> Vec<String> {
        let target = ShapeId::new("test", "Number");
        let shape = Shape::Simple(Simple {
            internal: false,
            sensitive: false,
            kind,
            constraints: Constraints::default(),
            timestamp_format: None,
            docs: None,
            notes: Default::default(),
            traits: Default::default(),
        });
        let shapes = BTreeMap::from([(target.clone(), shape)]);
        let number = |text: Option<&str>| text.map(|text| text.parse::<Number>().unwrap());
        let constraints = Constraints {
            range: Some(Bounds {
                min: number(min),
                max: number(max),
            }),
            ..Constraints::default()
        };
        let value: Value = serde_json::from_str(value).unwrap();
        crate::validate::value(&shapes, "value", &target, &constraints, &value)
    }

    type Case<'a> = (SimpleKind, Option<&'a str>, Option<&'a str>, &'a str, bool);

    /// Each case: kind, minimum, maximum, value, and whether the generated
    /// server accepts it.
    fn assert_cases(cases: &[Case<'_>]) {
        for &(kind, min, max, value, accepted) in cases {
            let problems = problems(kind, min, max, value);
            assert_eq!(
                problems.is_empty(),
                accepted,
                "{kind:?} {min:?}..{max:?} with {value}: {problems:?}"
            );
        }
    }

    #[test]
    fn big_decimal_compares_exactly() {
        use SimpleKind::BigDecimal as D;
        let max = Some("0.29999999999999999999");
        let min = Some("-0.30000000000000000001");
        assert_cases(&[
            (D, None, max, "0.29999999999999999999", true),
            (D, None, max, "0.29999999999999999998", true),
            (D, None, max, "2.9999999999999999999e-1", true),
            (D, None, max, "0.3", false),
            (D, None, max, "3e-1", false),
            (D, None, max, "0.29999999999999999999000000001", false),
            (D, min, None, "-0.30000000000000000001", true),
            (D, min, None, "-0.3", true),
            (D, min, None, "-0.30000000000000000002", false),
            (D, min, None, "-3.0000000000000000002E-1", false),
            (D, Some("1e-10"), None, "1e-10", true),
            (D, Some("1e-10"), None, "0.0000000001", true),
            (D, Some("1e-10"), None, "1.00000000000000000001e-10", true),
            (D, Some("1e-10"), None, "0.00000000009999999999999", false),
            (D, None, Some("2.5E3"), "2500", true),
            (D, None, Some("2.5E3"), "2.4999999999999999999e3", true),
            (D, None, Some("2.5E3"), "2500.0000000000000000001", false),
            (D, Some("9007199254740993"), None, "9007199254740993", true),
            (
                D,
                Some("9007199254740993"),
                None,
                "9007199254740992.99999999",
                false,
            ),
            (D, Some("1e999999999"), None, "1e999999999", true),
            (D, Some("1e999999999"), None, "9.99999999e999999998", false),
            // Equal values in other spellings are within.
            (D, Some("1.0"), Some("1"), "1.00", true),
            (D, Some("1.0"), Some("1"), "1.000000000000000000001", false),
            // No decimal holds this exponent; the server cannot read it.
            (D, None, None, "1e99999999999999999999", false),
        ]);
    }

    #[test]
    fn big_integer_compares_exactly_past_i128() {
        use SimpleKind::BigInteger as I;
        let max = Some("170141183460469231731687303715884105728");
        let min = Some("-1e40");
        assert_cases(&[
            (
                I,
                None,
                max,
                "170141183460469231731687303715884105728",
                true,
            ),
            (
                I,
                None,
                max,
                "170141183460469231731687303715884105727",
                true,
            ),
            (
                I,
                None,
                max,
                "170141183460469231731687303715884105729",
                false,
            ),
            (
                I,
                min,
                None,
                "-10000000000000000000000000000000000000000",
                true,
            ),
            (
                I,
                min,
                None,
                "-10000000000000000000000000000000000000001",
                false,
            ),
            (I, None, Some("2.5E3"), "2500", true),
            (I, None, Some("2.5E3"), "2501", false),
            (I, Some("0.5"), None, "1", true),
            (I, Some("0.5"), None, "0", false),
            // The server's BigInteger reads any whole number, in any
            // notation, of up to 10,000 digits.
            (I, None, Some("100"), "1e2", true),
            (I, None, Some("99"), "1e2", false),
            (I, None, None, "100e-2", true),
            (I, None, None, "1.0", true),
            (I, None, None, "-0", true),
            (I, None, None, "1e9999", true),
            (I, None, None, "1e10000", false),
            (I, None, None, "1.5", false),
            (I, None, None, "1e-999999999", false),
            (I, None, None, "1e99999999999999999999", false),
        ]);
    }

    #[test]
    fn integer_kinds_compare_exactly_with_fractional_and_exponent_bounds() {
        use SimpleKind::{Byte, Integer, Long, Short};
        let long_min = Some("-9223372036854775807.99999999999999999999");
        let int_max = Some("2147483646.00000000000000000001");
        assert_cases(&[
            // f64 rounds both 9007199254740993 and 9007199254740992.5 to
            // 9007199254740992, and would accept the value.
            (
                Long,
                None,
                Some("9007199254740992.5"),
                "9007199254740992",
                true,
            ),
            (
                Long,
                None,
                Some("9007199254740992.5"),
                "9007199254740993",
                false,
            ),
            (Long, long_min, None, "-9223372036854775807", true),
            (Long, long_min, None, "-9223372036854775808", false),
            (Long, None, Some("1e2"), "100", true),
            (Long, None, Some("1e2"), "101", false),
            (Long, None, None, "9223372036854775807", true),
            (Long, None, None, "9223372036854775808", false),
            (Integer, Some("0.99999999999999999999"), None, "1", true),
            (Integer, Some("0.99999999999999999999"), None, "0", false),
            (Integer, None, int_max, "2147483646", true),
            (Integer, None, int_max, "2147483647", false),
            (Short, None, Some("-1.5"), "-2", true),
            (Short, None, Some("-1.5"), "-1", false),
            (Byte, None, Some("1e999999999"), "127", true),
            (Byte, Some("1e999999999"), None, "127", false),
            (Byte, Some("-1e999999999"), None, "-128", true),
            // Zero at any scale is zero, not a bound past i128.
            (Long, Some("0e40"), None, "5", true),
            (Long, Some("0e40"), None, "-1", false),
            (Long, None, Some("-0e50"), "0", true),
            (Long, None, Some("-0e50"), "1", false),
            // An integer kind takes plain digits only. A model's `-0` is
            // `0` by the time it is checked, and documents carry `0`.
            (Integer, None, None, "-0", true),
            (Integer, None, None, "0", true),
            (Integer, None, None, "1e2", false),
            (Integer, None, None, "1.0", false),
        ]);
        let bounds = |min: &str, max: &str| Bounds {
            min: Some(min.parse::<Number>().unwrap()),
            max: Some(max.parse::<Number>().unwrap()),
        };
        assert_eq!(
            bounds("0.99999999999999999999", "-1e999999999").integer_bounds(),
            (Some(1), Some(i128::MIN))
        );
        assert_eq!(
            bounds("-1.5", "2.5E3").integer_bounds(),
            (Some(-1), Some(2500))
        );
        assert_eq!(bounds("0e40", "-0e50").integer_bounds(), (Some(0), Some(0)));
        assert_eq!(
            bounds("1e999999999", "-0.5").integer_bounds(),
            (Some(i128::MAX), Some(-1))
        );
    }

    #[test]
    fn floats_keep_ieee_semantics() {
        use SimpleKind::{Double, Float};
        assert_cases(&[
            // The server's bound is the f64 nearest 0.29999999999999999999,
            // which is 0.3's.
            (Double, None, Some("0.29999999999999999999"), "0.3", true),
            (
                Double,
                None,
                Some("0.29999999999999999999"),
                "0.30000000000000004",
                false,
            ),
            (Double, Some("1e-10"), None, "1e-10", true),
            (Double, Some("1e-10"), None, "9.999999999999999e-11", false),
            (Double, None, Some("0.1"), "0.100000005", false),
            (Double, Some("-2.5E3"), None, "-2500", true),
            (Double, Some("-2.5E3"), None, "-2500.0000000000005", false),
            (Double, None, Some("1e400"), "1e308", true),
            // The server reads only JSON numbers, so never these.
            (Double, None, None, "\"Infinity\"", false),
            (Double, None, None, "\"-Infinity\"", false),
            (Double, None, None, "\"NaN\"", false),
            (Double, None, None, "-0", true),
            (Double, None, None, "1e400", false),
            // 0.100000005 narrows to the f32 nearest 0.1, the bound's own.
            (Float, None, Some("0.1"), "0.100000005", true),
            (Float, None, Some("0.1"), "0.10000001", false),
            (Float, Some("0.29999999999999999999"), None, "0.3", true),
            (Float, None, None, "\"Infinity\"", false),
            (Float, None, None, "\"NaN\"", false),
            (Float, None, None, "1e39", false),
            (Float, None, None, "3.4028235e38", true),
        ]);
    }
}
