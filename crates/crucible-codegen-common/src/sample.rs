//! Canonical sample values: for any shape, a deterministic value that the
//! model's own validation accepts, built from the IR alone. The API
//! reference shows one where a model declares no `example`, so a reader
//! still sees typed ids with their prefix and length, timestamps in the
//! member's format and values within their bounds (docs/docs.md, "Generated
//! samples").
//!
//! Values are in the model form `example` blocks use (member names, RFC 3339
//! or epoch timestamps, base64 blobs); [`wire_member`] and the description's
//! wire messages encode them for a location exactly as they encode examples.
//!
//! What a value is, by shape: a string matches its `pattern` (the subset in
//! `sample::pattern`) within its length; numbers are 1, or 1.5, moved into
//! their range; an enum is its first value; a timestamp is 2026-01-15T09:30Z;
//! a blob is the member name's bytes; a document is `{}`; a list or map has
//! one entry, or as many as its minimum length (distinct for `unique`); a
//! union sets its first member; a member with a default takes the default.
//! The full sample includes every optional member, so it is a complete
//! picture; the minimal one only what the server requires. A member whose
//! value would close a cycle is left out, and past a few levels of nesting
//! only required members are, so a recursive model ends at its shortest
//! valid value. A pattern outside the subset, or bounds no value meets,
//! is reported as a [`Fallback`] at its path, with a readable placeholder in
//! its place, so the caller can tell the value would not pass.

mod pattern;

use std::collections::BTreeSet;

use base64::Engine as _;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Constraints, Member, Service, Shape, SimpleKind, TimestampFormat};
use serde_json::{Map, Number, Value, json};

/// Which members a structure's sample includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Every member, optional ones too.
    Full,
    /// Only the members the server requires: required and without a default.
    Minimal,
}

/// A value that does not pass the model's validation, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fallback {
    /// Where the value sits, written as example diagnostics write it:
    /// `input.name`, `input.tags[0]`, `input.labels key`.
    pub path: String,
    /// The member, or shape, whose constraints the value misses.
    pub shape: String,
    /// The pattern the value could not be generated for, if that was it.
    pub pattern: Option<String>,
    pub reason: String,
}

impl Fallback {
    /// The description's form: `{path, shape, pattern, reason}`.
    pub fn to_json(&self) -> Value {
        json!({"path": self.path, "shape": self.shape, "pattern": self.pattern,
            "reason": self.reason})
    }
}

/// A sample value in model form, and every place it falls short.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub value: Value,
    pub fallbacks: Vec<Fallback>,
}

/// An operation's input, output or error structure; `None` (Unit) is `{}`.
/// `path` names the value in fallbacks: `input`, `output`, `error`.
pub fn structure(service: &Service, id: Option<&ShapeId>, variant: Variant, path: &str) -> Sample {
    let mut sampler = Sampler::new(service, variant);
    let value = match id {
        Some(id) => {
            let slot = Slot::root(path, id.to_string(), service.name_of(id));
            sampler.value(id, &Constraints::default(), &slot)
        }
        None => Some(json!({})),
    };
    sampler.finish(value, path, id)
}

/// A value of the shape `id`, under the shape's own constraints.
pub fn shape(service: &Service, id: &ShapeId, variant: Variant) -> Sample {
    let mut sampler = Sampler::new(service, variant);
    let constraints = match service.shape(id) {
        Some(Shape::Simple(s)) => s.constraints.clone(),
        Some(Shape::List(s)) => s.constraints.clone(),
        Some(Shape::Map(s)) => s.constraints.clone(),
        _ => Constraints::default(),
    };
    let name = service.name_of(id);
    let slot = Slot::root(name, id.to_string(), name);
    let value = sampler.value(id, &constraints, &slot);
    sampler.finish(value, name, Some(id))
}

/// A value of `member` of `owner`: its effective constraints, its default
/// where it has one, and a string readable as its name.
pub fn member(service: &Service, owner: &ShapeId, member: &Member, variant: Variant) -> Sample {
    let mut sampler = Sampler::new(service, variant);
    let slot = Slot::root(
        &member.name,
        owner.with_member(&member.name).to_string(),
        &member.name,
    );
    let value = sampler.member(member, &slot);
    sampler.finish(value, &member.name, Some(&member.target))
}

/// [`member`] as the wire carries it: timestamps in the member's resolved
/// format, or `format` where an HTTP binding decides it.
pub fn wire_member(
    service: &Service,
    owner: &ShapeId,
    m: &Member,
    format: Option<TimestampFormat>,
    variant: Variant,
) -> Sample {
    let sample = member(service, owner, m, variant);
    Sample {
        value: service.wire_example(m, &sample.value, format),
        fallbacks: sample.fallbacks,
    }
}

/// Aggregates this deep take only their required members, so a sample of a
/// wide recursive model stays small.
const DEPTH: usize = 6;
/// 2026-01-15T09:30:00Z: a fixed instant, never a clock.
const INSTANT: i64 = 1_768_469_400;
/// A fixed UUID for idempotency tokens, which are conventionally one.
const TOKEN: &str = "3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b";

/// Where a value goes: its path, what constrains it, the name its strings
/// spell, and which of several distinct values it is.
#[derive(Clone)]
struct Slot {
    path: String,
    owner: String,
    hint: String,
    seed: u64,
    token: bool,
}

impl Slot {
    fn new(path: &str, owner: &str, hint: &str, seed: u64) -> Self {
        Self {
            path: path.to_owned(),
            owner: owner.to_owned(),
            hint: hint.to_owned(),
            seed,
            token: false,
        }
    }

    fn root(path: &str, owner: String, hint: &str) -> Self {
        Self::new(path, &owner, hint, 0)
    }
}

struct Sampler<'a> {
    service: &'a Service,
    variant: Variant,
    /// Aggregates being built, to stop where a cycle would close.
    stack: Vec<ShapeId>,
    fallbacks: BTreeSet<Fallback>,
}

impl<'a> Sampler<'a> {
    fn new(service: &'a Service, variant: Variant) -> Self {
        Self {
            service,
            variant,
            stack: Vec::new(),
            fallbacks: BTreeSet::new(),
        }
    }

    fn finish(mut self, value: Option<Value>, path: &str, id: Option<&ShapeId>) -> Sample {
        let value = value.unwrap_or_else(|| {
            self.fallback(
                &Slot::root(path, id.map(ToString::to_string).unwrap_or_default(), ""),
                None,
                "every value of the shape reaches itself".into(),
            );
            Value::Null
        });
        Sample {
            value,
            fallbacks: self.fallbacks.into_iter().collect(),
        }
    }

    fn fallback(&mut self, slot: &Slot, pattern: Option<&str>, reason: String) {
        self.fallbacks.insert(Fallback {
            path: slot.path.clone(),
            shape: slot.owner.clone(),
            pattern: pattern.map(str::to_owned),
            reason,
        });
    }

    /// Whether `value` passes the model's checks, as a default must to be used.
    fn valid(&self, target: &ShapeId, constraints: &Constraints, value: &Value) -> bool {
        crucible_ir::validate::value(&self.service.shapes, "value", target, constraints, value)
            .is_empty()
    }

    fn member(&mut self, m: &Member, slot: &Slot) -> Option<Value> {
        if slot.seed == 0
            && let Some(default) = m.default.as_ref().filter(|d| !d.is_null())
            && self.valid(&m.target, &m.constraints, default)
        {
            return Some(default.clone());
        }
        let slot = Slot {
            token: m.idempotency_token,
            ..slot.clone()
        };
        self.value(&m.target, &m.constraints, &slot)
    }

    /// A value of `target` under `constraints`; `None` when every value of
    /// it would close a cycle already being built.
    fn value(&mut self, target: &ShapeId, constraints: &Constraints, slot: &Slot) -> Option<Value> {
        let shape = self.service.shape(target)?;
        let aggregate = matches!(
            shape,
            Shape::List(_) | Shape::Map(_) | Shape::Structure(_) | Shape::Union(_)
        );
        if aggregate {
            if self.stack.contains(target) {
                return None;
            }
            self.stack.push(target.clone());
        }
        let value = self.shape_value(target, shape, constraints, slot);
        if aggregate {
            self.stack.pop();
        }
        value
    }

    /// Past [`DEPTH`], or in a minimal sample, optional parts are left out.
    fn brief(&self) -> bool {
        self.variant == Variant::Minimal || self.stack.len() > DEPTH
    }

    fn shape_value(
        &mut self,
        target: &ShapeId,
        shape: &'a Shape,
        constraints: &Constraints,
        slot: &Slot,
    ) -> Option<Value> {
        Some(match shape {
            Shape::Unit { .. } => json!({}),
            Shape::Simple(simple) => match simple.kind {
                SimpleKind::Boolean => json!(slot.seed % 2 == 1),
                SimpleKind::String => self.string(constraints, slot),
                SimpleKind::Byte => self.integer(constraints, slot, i8::MIN.into(), i8::MAX.into()),
                SimpleKind::Short => {
                    self.integer(constraints, slot, i16::MIN.into(), i16::MAX.into())
                }
                SimpleKind::Integer => {
                    self.integer(constraints, slot, i32::MIN.into(), i32::MAX.into())
                }
                SimpleKind::Long | SimpleKind::BigInteger => {
                    self.integer(constraints, slot, i64::MIN.into(), i64::MAX.into())
                }
                SimpleKind::Float | SimpleKind::Double | SimpleKind::BigDecimal => {
                    self.decimal(constraints, slot)
                }
                SimpleKind::Timestamp => json!(INSTANT + 3600 * slot.seed as i64),
                SimpleKind::Document => json!({}),
                SimpleKind::Blob => self.blob(constraints, slot),
            },
            Shape::Enum(e) => match e.values.len() {
                0 => self.empty_enum(slot),
                n => json!(e.values[slot.seed as usize % n].value),
            },
            Shape::IntEnum(e) => match e.values.len() {
                0 => self.empty_enum(slot),
                n => json!(e.values[slot.seed as usize % n].value),
            },
            Shape::List(list) => {
                let count = self.count(constraints);
                let mut items = Vec::new();
                for index in 0..count {
                    let item = Slot::new(
                        &format!("{}[{index}]", slot.path),
                        &target.with_member("member").to_string(),
                        &slot.hint,
                        slot.seed + index as u64,
                    );
                    match self.value(&list.member.target, &list.member.constraints, &item) {
                        Some(value) => items.push(value),
                        None if self.min(constraints) == 0 => return Some(json!([])),
                        None => return None,
                    }
                }
                if constraints.unique_items
                    && items
                        .iter()
                        .enumerate()
                        .any(|(i, v)| items[..i].contains(v))
                {
                    self.fallback(
                        slot,
                        None,
                        format!("{count} distinct items could not be made"),
                    );
                }
                Value::Array(items)
            }
            Shape::Map(map) => {
                let count = self.count(constraints);
                let mut entries = Map::new();
                let mut seed = slot.seed;
                // Keys must differ; a few more seeds find them if one repeats.
                while entries.len() < count && seed < slot.seed + 4 * count as u64 + 4 {
                    let key_slot = Slot::new(
                        &format!("{} key", slot.path),
                        &target.with_member("key").to_string(),
                        "key",
                        seed,
                    );
                    seed += 1;
                    let key = self.value(&map.key.target, &map.key.constraints, &key_slot)?;
                    let Some(key) = key.as_str().map(str::to_owned) else {
                        self.fallback(slot, None, "map keys are not strings".into());
                        break;
                    };
                    if entries.contains_key(&key) {
                        continue;
                    }
                    let value_slot = Slot::new(
                        &format!("{}.{key}", slot.path),
                        &target.with_member("value").to_string(),
                        &slot.hint,
                        seed - 1,
                    );
                    match self.value(&map.value.target, &map.value.constraints, &value_slot) {
                        Some(value) => {
                            entries.insert(key, value);
                        }
                        None if self.min(constraints) == 0 => return Some(json!({})),
                        None => return None,
                    }
                }
                if entries.len() < count {
                    self.fallback(
                        slot,
                        None,
                        format!("{count} distinct keys could not be made"),
                    );
                }
                Value::Object(entries)
            }
            Shape::Structure(structure) => {
                let brief = self.brief();
                let mut members = Map::new();
                for m in &structure.members {
                    let required = m.required && m.default.is_none();
                    if brief && !required {
                        continue;
                    }
                    let member_slot = Slot::new(
                        &format!("{}.{}", slot.path, m.name),
                        &target.with_member(&m.name).to_string(),
                        &m.name,
                        slot.seed,
                    );
                    match self.member(m, &member_slot) {
                        Some(value) => {
                            members.insert(m.name.clone(), value);
                        }
                        None if required => return None,
                        None => {}
                    }
                }
                Value::Object(members)
            }
            Shape::Union(union) => {
                // The first member, or the first that does not lead back.
                union.members.iter().find_map(|m| {
                    let arm = Slot::new(
                        &format!("{}.{}", slot.path, m.name),
                        &target.with_member(&m.name).to_string(),
                        &m.name,
                        slot.seed,
                    );
                    self.member(m, &arm)
                        .map(|value| Value::Object(Map::from_iter([(m.name.clone(), value)])))
                })?
            }
        })
    }

    fn empty_enum(&mut self, slot: &Slot) -> Value {
        self.fallback(slot, None, "the enum has no values".into());
        json!("")
    }

    fn min(&self, constraints: &Constraints) -> usize {
        constraints
            .length
            .as_ref()
            .and_then(|b| b.min)
            .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// One entry, or the minimum length, within the maximum; none past
    /// [`DEPTH`] where none is allowed.
    fn count(&self, constraints: &Constraints) -> usize {
        let min = self.min(constraints);
        let max = constraints
            .length
            .as_ref()
            .and_then(|b| b.max)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let preferred = if self.stack.len() > DEPTH { 0 } else { 1 };
        // A huge minimum is a model's business; a sample stops at 100.
        let count = preferred.max(min).min(100);
        max.map_or(count, |max| count.min(max))
    }

    fn string(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let min = self.min(constraints);
        let max = constraints
            .length
            .as_ref()
            .and_then(|b| b.max)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        if slot.token {
            let token = if slot.seed == 0 {
                TOKEN.to_owned()
            } else {
                format!("{}{:x}", &TOKEN[..TOKEN.len() - 1], slot.seed % 16)
            };
            if self.string_fits(constraints, &token) {
                return json!(token);
            }
        }
        let Some(pattern) = &constraints.pattern else {
            return json!(fit(&slot.hint, slot.seed, min, max));
        };
        let placeholder = fit(&slot.hint, slot.seed, min, max);
        match pattern::generate(pattern, min, max, &slot.hint, slot.seed) {
            Ok(text) if self.string_fits(constraints, &text) => json!(text),
            Ok(text) => {
                self.fallback(
                    slot,
                    Some(pattern),
                    format!("the generated {text:?} does not match the pattern"),
                );
                json!(placeholder)
            }
            Err(reason) => {
                self.fallback(slot, Some(pattern), reason);
                json!(placeholder)
            }
        }
    }

    /// Whether `text` is within the length and matches the pattern, as the
    /// model's validation checks them. A pattern no engine can run is a
    /// lowering diagnostic, not checked here either.
    fn string_fits(&self, constraints: &Constraints, text: &str) -> bool {
        let len = text.chars().count() as u64;
        let within = constraints.length.as_ref().is_none_or(|b| {
            b.min.is_none_or(|min| len >= min) && b.max.is_none_or(|max| len <= max)
        });
        let matches = match constraints.rust_pattern() {
            Ok(Some(pattern)) => regex::Regex::new(&pattern).map_or(true, |r| r.is_match(text)),
            _ => true,
        };
        within && matches
    }

    fn integer(&mut self, constraints: &Constraints, slot: &Slot, low: i128, high: i128) -> Value {
        let bound = |n: Option<&Number>, up: bool| {
            n.and_then(|n| {
                n.to_string().parse::<i128>().ok().or_else(|| {
                    n.as_f64()
                        .map(|f| if up { f.ceil() } else { f.floor() })
                        .filter(|f| f.abs() < 1e30)
                        .map(|f| f as i128)
                })
            })
        };
        let range = constraints.range.as_ref();
        let min = bound(range.and_then(|r| r.min.as_ref()), true);
        let max = bound(range.and_then(|r| r.max.as_ref()), false);
        let low = min.map_or(low, |m| m.max(low));
        let high = max.map_or(high, |m| m.min(high));
        if low > high {
            self.fallback(slot, None, "no integer is within the range".into());
            return json!(0);
        }
        let base = if (low..=high).contains(&1) {
            1
        } else if min.is_some() {
            low
        } else {
            high
        };
        let seed = i128::from(slot.seed);
        let value = if base + seed <= high {
            base + seed
        } else {
            (base - seed).max(low)
        };
        i64::try_from(value).map_or_else(|_| json!(base as i64), |v| json!(v))
    }

    fn decimal(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let range = constraints.range.as_ref();
        let min = range.and_then(|r| r.min.as_ref()).and_then(Number::as_f64);
        let max = range.and_then(|r| r.max.as_ref()).and_then(Number::as_f64);
        let low = min.unwrap_or(f64::MIN);
        let high = max.unwrap_or(f64::MAX);
        if low > high {
            self.fallback(slot, None, "no number is within the range".into());
            return json!(0);
        }
        let base = if (low..=high).contains(&1.5) {
            1.5
        } else if min.is_some() {
            low
        } else {
            high
        };
        let seed = slot.seed as f64;
        let value = if base + seed <= high {
            base + seed
        } else {
            (base - seed).max(low)
        };
        // A bound read from the model is a number already; prefer its
        // exact text over an f64 round trip where the sample is the bound.
        match range.and_then(|r| r.min.as_ref()) {
            Some(n) if value == low && n.as_f64() == Some(low) => Value::Number(n.clone()),
            _ => Number::from_f64(value).map_or(json!(0), Value::Number),
        }
    }

    fn blob(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let min = self.min(constraints);
        let max = constraints
            .length
            .as_ref()
            .and_then(|b| b.max)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX));
        let bytes = fit(&slot.hint, slot.seed, min, max);
        json!(base64::engine::general_purpose::STANDARD.encode(bytes))
    }
}

/// `hint` (with `seed` when it is not the first), repeated to `min`
/// characters and cut at `max`: a readable placeholder of the right length.
fn fit(hint: &str, seed: u64, min: usize, max: Option<usize>) -> String {
    let word = match (hint.is_empty(), seed) {
        (true, 0) => "value".to_owned(),
        (true, seed) => format!("value{seed}"),
        (false, 0) => hint.to_owned(),
        (false, seed) => format!("{hint}{seed}"),
    };
    let len = max.map_or(word.chars().count().max(min), |max| {
        word.chars().count().max(min).min(max)
    });
    word.chars().cycle().take(len).collect()
}

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn placeholders_fit_their_length() {
        assert_eq!(fit("name", 0, 0, None), "name");
        assert_eq!(fit("name", 0, 6, None), "namena");
        assert_eq!(fit("name", 0, 0, Some(2)), "na");
        assert_eq!(fit("name", 2, 0, None), "name2");
        assert_eq!(fit("", 0, 0, None), "value");
    }
}
