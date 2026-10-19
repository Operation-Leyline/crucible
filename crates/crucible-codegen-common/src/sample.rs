//! Canonical sample values: for any shape, a deterministic value that the
//! model's own validation accepts, built from the IR alone. The API
//! reference shows one where a model declares no `example`, so a reader
//! still sees typed ids with their prefix and length, timestamps in the
//! member's format and values within their bounds (docs/docs.md, "Generated
//! samples"). A sample knows only the model: a value the service checks
//! further (a query language, a reference to something that exists) is
//! only as good as the model's constraints, which is why the reference
//! labels it as generated and the `examples` policy asks for real ones.
//!
//! Values are in the model form `example` blocks use (member names, RFC 3339
//! or epoch timestamps, base64 blobs); the description's wire messages
//! encode them for a location exactly as they encode examples.
//!
//! What a value is, by shape: a string matches its `pattern` (the subset in
//! `sample::pattern`) within its length; numbers are 1, or 1.5, moved into
//! their range, exactly at a bound the model writes; an enum is its first
//! value; a timestamp is 2026-01-15T09:30Z; a blob is the member name's
//! bytes; a document is `{}`; a list or map has one entry, or as many as its
//! minimum length (distinct for `unique` and for keys); a union sets its
//! first member; a member with a default takes the default. The full sample
//! includes every optional member, so it is a complete picture; the minimal
//! one only what the server requires. A member whose value would close a
//! cycle is left out, and past a few levels of nesting only required
//! members are, so a recursive model ends at its shortest valid value.
//!
//! What cannot be made is reported as a [`Fallback`] at its path, with a
//! readable placeholder in its place, so the caller can tell the value would
//! not pass: a pattern outside the subset, bounds no value meets, a minimum
//! too large to show ([`MAX_ITEMS`] entries, [`MAX_LENGTH`] characters or
//! bytes), or a header, label or query value that would need a control
//! character or surrounding white space, which HTTP cannot carry intact.

mod pattern;

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use crucible_ir::ShapeId;
use crucible_ir::decimal::Decimal;
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

/// The most entries a sample list or map shows; a larger minimum is a
/// fallback.
pub const MAX_ITEMS: usize = 100;
/// The longest string, in characters, or blob, in bytes, a sample shows.
pub const MAX_LENGTH: usize = 1024;

/// An operation's input, output or error structure; `None` (Unit) is `{}`.
/// `path` names the value in fallbacks: `input`, `output`, `error`. `text`
/// names the members bound to a header, a path label or the query string,
/// whose values must survive HTTP's handling of text.
pub fn structure(
    service: &Service,
    id: Option<&ShapeId>,
    variant: Variant,
    path: &str,
    text: &[&str],
) -> Sample {
    let mut sampler = Sampler::new(service, variant);
    sampler.text = text.iter().map(|name| (*name).to_owned()).collect();
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
#[doc(hidden)]
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
#[doc(hidden)]
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
#[doc(hidden)]
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
/// Seeds tried for a header, label or query string that came out with a
/// control character or surrounding white space.
const RETRIES: u64 = 8;

/// Where a value goes: its path, what constrains it, the name its strings
/// spell, which of several distinct values it is, and how it travels.
#[derive(Clone)]
struct Slot {
    path: String,
    owner: String,
    hint: String,
    seed: u64,
    /// Extra length for a string, to tell apart list items that came out
    /// alike.
    stretch: usize,
    token: bool,
    /// In a header, label or query string.
    text: bool,
}

impl Slot {
    /// A part of `self`: its location and seed, with its own path and name.
    fn child(&self, path: String, owner: String, hint: &str) -> Self {
        Self {
            path,
            owner,
            hint: hint.to_owned(),
            token: false,
            ..self.clone()
        }
    }

    fn root(path: &str, owner: String, hint: &str) -> Self {
        Self {
            path: path.to_owned(),
            owner,
            hint: hint.to_owned(),
            seed: 0,
            stretch: 0,
            token: false,
            text: false,
        }
    }
}

struct Sampler<'a> {
    service: &'a Service,
    variant: Variant,
    /// The top-level members bound to text locations.
    text: BTreeSet<String>,
    /// Aggregates being built, to stop where a cycle would close.
    stack: Vec<ShapeId>,
    fallbacks: BTreeSet<Fallback>,
    /// Each pattern compiled once, `None` where the engine cannot run it.
    patterns: BTreeMap<String, Option<regex::Regex>>,
}

impl<'a> Sampler<'a> {
    fn new(service: &'a Service, variant: Variant) -> Self {
        Self {
            service,
            variant,
            text: BTreeSet::new(),
            stack: Vec::new(),
            fallbacks: BTreeSet::new(),
            patterns: BTreeMap::new(),
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
                SimpleKind::Byte => self.integer(
                    constraints,
                    slot,
                    Some(i8::MIN.into()),
                    Some(i8::MAX.into()),
                ),
                SimpleKind::Short => self.integer(
                    constraints,
                    slot,
                    Some(i16::MIN.into()),
                    Some(i16::MAX.into()),
                ),
                SimpleKind::Integer => self.integer(
                    constraints,
                    slot,
                    Some(i32::MIN.into()),
                    Some(i32::MAX.into()),
                ),
                SimpleKind::Long => self.integer(
                    constraints,
                    slot,
                    Some(i64::MIN.into()),
                    Some(i64::MAX.into()),
                ),
                SimpleKind::BigInteger => self.integer(constraints, slot, None, None),
                SimpleKind::Float | SimpleKind::Double => self.float(constraints, slot),
                SimpleKind::BigDecimal => self.big_decimal(constraints, slot),
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
                let count = self.count(constraints, slot);
                let mut items: Vec<Value> = Vec::new();
                for index in 0..count {
                    let mut item = slot.child(
                        format!("{}[{index}]", slot.path),
                        target.with_member("member").to_string(),
                        &slot.hint,
                    );
                    item.seed = slot.seed + index as u64;
                    let mut value =
                        self.value(&list.member.target, &list.member.constraints, &item);
                    // An item that repeats an earlier one is made longer,
                    // which distinguishes strings a pattern allows little of.
                    while constraints.unique_items
                        && item.stretch < slot.stretch + 8
                        && value.as_ref().is_some_and(|v| items.contains(v))
                    {
                        item.stretch += 1;
                        value = self.value(&list.member.target, &list.member.constraints, &item);
                    }
                    match value {
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
                let count = self.count(constraints, slot);
                let mut entries = Map::new();
                let mut seed = slot.seed;
                // Keys must differ; a few more seeds find them if one repeats.
                while entries.len() < count && seed < slot.seed + 4 * count as u64 + 4 {
                    let mut key_slot = slot.child(
                        format!("{} key", slot.path),
                        target.with_member("key").to_string(),
                        "key",
                    );
                    key_slot.seed = seed;
                    seed += 1;
                    let key = self.value(&map.key.target, &map.key.constraints, &key_slot)?;
                    let Some(key) = key.as_str().map(str::to_owned) else {
                        self.fallback(slot, None, "map keys are not strings".into());
                        break;
                    };
                    if entries.contains_key(&key) {
                        continue;
                    }
                    let mut value_slot = slot.child(
                        format!("{}.{key}", slot.path),
                        target.with_member("value").to_string(),
                        &slot.hint,
                    );
                    value_slot.seed = seed - 1;
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
                // The operation's own structure is the one at the bottom.
                let top = self.stack.len() == 1;
                let mut members = Map::new();
                for m in &structure.members {
                    let required = m.required_without_default();
                    if brief && !required {
                        continue;
                    }
                    let mut member_slot = slot.child(
                        format!("{}.{}", slot.path, m.name),
                        target.with_member(&m.name).to_string(),
                        &m.name,
                    );
                    member_slot.text |= top && self.text.contains(&m.name);
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
                    let arm = slot.child(
                        format!("{}.{}", slot.path, m.name),
                        target.with_member(&m.name).to_string(),
                        &m.name,
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

    fn max(&self, constraints: &Constraints) -> Option<usize> {
        constraints
            .length
            .as_ref()
            .and_then(|b| b.max)
            .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// One entry, or the minimum length, within the maximum and
    /// [`MAX_ITEMS`]; none past [`DEPTH`] where none is allowed.
    fn count(&mut self, constraints: &Constraints, slot: &Slot) -> usize {
        let min = self.min(constraints);
        if min > MAX_ITEMS {
            self.fallback(
                slot,
                None,
                format!("a minimum of {min} items is too many to show; the sample has {MAX_ITEMS}"),
            );
        }
        let preferred = if self.stack.len() > DEPTH { 0 } else { 1 };
        let count = preferred.max(min).min(MAX_ITEMS);
        self.max(constraints).map_or(count, |max| count.min(max))
    }

    /// The length bounds a sample string or blob can show: the model's,
    /// capped at [`MAX_LENGTH`], with a fallback for a larger minimum.
    fn lengths(
        &mut self,
        constraints: &Constraints,
        slot: &Slot,
        unit: &str,
    ) -> (usize, Option<usize>) {
        let min = self.min(constraints);
        if min > MAX_LENGTH {
            self.fallback(
                slot,
                None,
                format!(
                    "a minimum of {min} {unit} is too long to show; the sample has {MAX_LENGTH}"
                ),
            );
        }
        let max = self
            .max(constraints)
            .map_or(MAX_LENGTH, |max| max.min(MAX_LENGTH));
        (min.min(max), Some(max))
    }

    fn string(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let (min, max) = self.lengths(constraints, slot, "characters");
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
        let placeholder = fit(&slot.hint, slot.seed, min + slot.stretch, max);
        let Some(pattern) = &constraints.pattern else {
            return json!(placeholder);
        };
        let mut problem = None;
        // A text location retries other seeds for a value HTTP keeps intact.
        let tries = if slot.text { RETRIES } else { 1 };
        for retry in 0..tries {
            match pattern::generate(
                pattern,
                min,
                max,
                &slot.hint,
                slot.seed + retry,
                slot.stretch,
            ) {
                Ok(text) if !self.string_fits(constraints, &text) => {
                    problem = Some(format!("the generated {text:?} does not match the pattern"));
                    break;
                }
                Ok(text) if slot.text && !travels_as_text(&text) => {
                    problem = Some(
                        "a header, label or query value cannot carry control characters or surrounding white space".into(),
                    );
                }
                Ok(text) => return json!(text),
                Err(reason) => {
                    problem = Some(reason);
                    break;
                }
            }
        }
        self.fallback(slot, Some(pattern), problem.expect("a failed try says why"));
        json!(placeholder)
    }

    /// Whether `text` is within the length and matches the pattern, as the
    /// model's validation checks them. A pattern no engine can run is a
    /// lowering diagnostic, not checked here either.
    fn string_fits(&mut self, constraints: &Constraints, text: &str) -> bool {
        let len = text.chars().count() as u64;
        let within = constraints.length.as_ref().is_none_or(|b| {
            b.min.is_none_or(|min| len >= min) && b.max.is_none_or(|max| len <= max)
        });
        let Ok(Some(pattern)) = constraints.rust_pattern() else {
            return within;
        };
        let regex = self
            .patterns
            .entry(pattern)
            .or_insert_with_key(|pattern| regex::Regex::new(pattern).ok());
        within && regex.as_ref().is_none_or(|r| r.is_match(text))
    }

    /// An integer within the model's range and the kind's (`None` for
    /// `bigInteger`, which has no limit): exact, so a bound past `i64`
    /// stays one.
    fn integer(
        &mut self,
        constraints: &Constraints,
        slot: &Slot,
        kind_low: Option<i128>,
        kind_high: Option<i128>,
    ) -> Value {
        let range = constraints.range.as_ref();
        let min_bound = range.and_then(|r| r.min.as_ref());
        let max_bound = range.and_then(|r| r.max.as_ref());
        // A bound past an i128 is itself the sample.
        fn rounded(
            bound: Option<&Number>,
            round: fn(&Decimal) -> Option<i128>,
        ) -> Option<(&Number, Option<i128>)> {
            bound.map(|bound| (bound, Decimal::parse(bound).and_then(|d| round(&d))))
        }
        let (min, max) = (
            rounded(min_bound, Decimal::ceil),
            rounded(max_bound, Decimal::floor),
        );
        for (bound, rounded) in [min, max].into_iter().flatten() {
            if rounded.is_none() {
                return Value::Number(bound.clone());
            }
        }
        let min = min.and_then(|(_, rounded)| rounded);
        let max = max.and_then(|(_, rounded)| rounded);
        let low = match (min, kind_low) {
            (Some(m), Some(k)) => m.max(k),
            (Some(m), None) => m,
            (None, k) => k.unwrap_or(i128::MIN),
        };
        let high = match (max, kind_high) {
            (Some(m), Some(k)) => m.min(k),
            (Some(m), None) => m,
            (None, k) => k.unwrap_or(i128::MAX),
        };
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
        let value = match base.checked_add(seed) {
            Some(up) if up <= high => up,
            _ => base.saturating_sub(seed).max(low),
        };
        Value::Number(Decimal::integer(value).to_number())
    }

    /// A `float` or `double`: 1.5 moved into the range; at a bound, the
    /// bound as the model writes it.
    fn float(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let range = constraints.range.as_ref();
        let min_bound = range.and_then(|r| r.min.as_ref());
        let max_bound = range.and_then(|r| r.max.as_ref());
        let min = min_bound.and_then(Number::as_f64);
        let max = max_bound.and_then(Number::as_f64);
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
        match (min_bound, max_bound) {
            (Some(n), _) if value == low => Value::Number(n.clone()),
            (_, Some(n)) if value == high => Value::Number(n.clone()),
            _ => Number::from_f64(value).map_or(json!(0), Value::Number),
        }
    }

    /// A `bigDecimal`, in exact decimal arithmetic: the server compares it
    /// exactly, so `..0.29999999999999999999` must not round to 0.3.
    fn big_decimal(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let range = constraints.range.as_ref();
        let min_bound = range.and_then(|r| r.min.as_ref());
        let max_bound = range.and_then(|r| r.max.as_ref());
        let (min, max) = (
            min_bound.and_then(Decimal::parse),
            max_bound.and_then(Decimal::parse),
        );
        if let (Some(min), Some(max)) = (&min, &max)
            && max < min
        {
            self.fallback(slot, None, "no number is within the range".into());
            return json!(0);
        }
        let within = |d: &Decimal| {
            min.as_ref().is_none_or(|m| d >= m) && max.as_ref().is_none_or(|m| m >= d)
        };
        let base = match (&min, &max) {
            _ if within(&Decimal::new(15, 1)) => Decimal::new(15, 1),
            (Some(min), _) => min.clone(),
            (None, Some(max)) => max.clone(),
            (None, None) => Decimal::new(15, 1),
        };
        let seed = Decimal::integer(i128::from(slot.seed));
        // Arithmetic past a thousand digits is refused, leaving the bound.
        let value = [base.add(&seed), base.sub(&seed)]
            .into_iter()
            .flatten()
            .find(|d| within(d))
            .unwrap_or(base);
        // At a bound, the bound's own digits.
        if min.as_ref() == Some(&value) {
            return Value::Number(min_bound.expect("a minimum").clone());
        }
        if max.as_ref() == Some(&value) {
            return Value::Number(max_bound.expect("a maximum").clone());
        }
        Value::Number(value.to_number())
    }

    fn blob(&mut self, constraints: &Constraints, slot: &Slot) -> Value {
        let (min, max) = self.lengths(constraints, slot, "bytes");
        let bytes = fit(&slot.hint, slot.seed, min, max);
        json!(base64::engine::general_purpose::STANDARD.encode(bytes))
    }
}

/// Whether an HTTP header, path label or query string carries `text`
/// unchanged: no control characters, and no white space at either end,
/// which header parsing trims.
fn travels_as_text(text: &str) -> bool {
    !text.chars().any(char::is_control) && text.trim() == text
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
    use super::{fit, travels_as_text};

    #[test]
    fn placeholders_fit_their_length() {
        assert_eq!(fit("name", 0, 0, None), "name");
        assert_eq!(fit("name", 0, 6, None), "namena");
        assert_eq!(fit("name", 0, 0, Some(2)), "na");
        assert_eq!(fit("name", 2, 0, None), "name2");
        assert_eq!(fit("", 0, 0, None), "value");
    }

    #[test]
    fn text_locations_refuse_controls_and_surrounding_space() {
        assert!(travels_as_text("a b"));
        assert!(!travels_as_text(" a"));
        assert!(!travels_as_text("a\t"));
        assert!(!travels_as_text("a\u{1}b"));
    }
}
