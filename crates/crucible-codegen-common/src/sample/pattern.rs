//! A string matching an ECMA-262 `pattern`, built deterministically from the
//! subset API models use: anchors, literals and escapes, `.`, character
//! classes (ranges, negation, `\d\w\s` and their complements), groups
//! (capturing and `(?:..)`), alternation, and the quantifiers `?`, `*`, `+`,
//! `{n}`, `{n,}` and `{n,m}` (lazy or not). Anything else (word boundaries,
//! back references, look-around, named groups) is refused with the reason,
//! so the caller reports it rather than emitting a value that fails.
//!
//! The caller still checks the result against the compiled pattern: this
//! module is a generator, not a second regex engine.

use std::iter::Peekable;
use std::str::Chars;

/// One parsed pattern node.
#[derive(Debug, Clone)]
enum Node {
    /// `^`, `$`: zero-width, satisfied at the ends of a whole-string match.
    Anchor,
    Literal(char),
    Class(Class),
    Concat(Vec<Node>),
    Alternation(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: usize,
        max: Option<usize>,
    },
}

/// A character set: ranges and shorthand sets, possibly negated.
#[derive(Debug, Clone, Default)]
struct Class {
    negated: bool,
    items: Vec<Item>,
}

#[derive(Debug, Clone)]
enum Item {
    Range(char, char),
    /// A complemented shorthand inside a class, such as `[\D]`.
    Not(Vec<(char, char)>),
}

const DIGIT: &[(char, char)] = &[('0', '9')];
const WORD: &[(char, char)] = &[('0', '9'), ('A', 'Z'), ('_', '_'), ('a', 'z')];
/// ECMA's `\s`: white space and line terminators.
const SPACE: &[(char, char)] = &[
    ('\t', '\r'),
    (' ', ' '),
    ('\u{a0}', '\u{a0}'),
    ('\u{1680}', '\u{1680}'),
    ('\u{2000}', '\u{200a}'),
    ('\u{2028}', '\u{2029}'),
    ('\u{202f}', '\u{202f}'),
    ('\u{205f}', '\u{205f}'),
    ('\u{3000}', '\u{3000}'),
    ('\u{feff}', '\u{feff}'),
];
const LINE_TERMINATORS: &[(char, char)] = &[('\n', '\n'), ('\r', '\r'), ('\u{2028}', '\u{2029}')];

/// The characters a sample prefers, most readable first.
const READABLE: &str = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-_.~";

impl Class {
    fn of(ranges: &[(char, char)], negated: bool) -> Self {
        Self {
            negated,
            items: ranges.iter().map(|&(a, b)| Item::Range(a, b)).collect(),
        }
    }

    fn contains(&self, c: char) -> bool {
        let inside = self.items.iter().any(|item| match item {
            Item::Range(a, b) => (*a..=*b).contains(&c),
            Item::Not(ranges) => !ranges.iter().any(|(a, b)| (*a..=*b).contains(&c)),
        });
        inside != self.negated
    }

    /// The readable characters in the class, in [`READABLE`] order, then the
    /// rest of printable ASCII; else the first character it holds at all.
    fn choices(&self) -> Vec<char> {
        let mut choices: Vec<char> = READABLE.chars().filter(|&c| self.contains(c)).collect();
        choices.extend(
            ('!'..='~')
                .chain([' '])
                .filter(|&c| !READABLE.contains(c) && self.contains(c)),
        );
        if choices.is_empty()
            && let Some(c) = ('\u{a1}'..=char::MAX).find(|&c| self.contains(c))
        {
            choices.push(c);
        }
        choices
    }
}

/// Parses `pattern`, or says which construct is outside the subset.
fn parse(pattern: &str) -> Result<Node, String> {
    let mut chars = pattern.chars().peekable();
    let node = alternation(&mut chars)?;
    match chars.next() {
        None => Ok(node),
        Some(c) => Err(format!("unbalanced `{c}`")),
    }
}

fn alternation(chars: &mut Peekable<Chars>) -> Result<Node, String> {
    let mut alternatives = vec![concat(chars)?];
    while chars.peek() == Some(&'|') {
        chars.next();
        alternatives.push(concat(chars)?);
    }
    Ok(if alternatives.len() == 1 {
        alternatives.pop().expect("one alternative")
    } else {
        Node::Alternation(alternatives)
    })
}

fn concat(chars: &mut Peekable<Chars>) -> Result<Node, String> {
    let mut nodes = Vec::new();
    while let Some(&c) = chars.peek() {
        if c == '|' || c == ')' {
            break;
        }
        let atom = atom(chars)?;
        nodes.push(quantified(chars, atom)?);
    }
    Ok(Node::Concat(nodes))
}

fn atom(chars: &mut Peekable<Chars>) -> Result<Node, String> {
    let c = chars.next().expect("peeked");
    Ok(match c {
        '^' | '$' => Node::Anchor,
        '.' => Node::Class(Class::of(LINE_TERMINATORS, true)),
        '(' => {
            if chars.peek() == Some(&'?') {
                chars.next();
                if chars.next() != Some(':') {
                    return Err("look-around, flags and named groups are outside the subset".into());
                }
            }
            let inner = alternation(chars)?;
            if chars.next() != Some(')') {
                return Err("an unclosed group".into());
            }
            inner
        }
        '[' => Node::Class(class(chars)?),
        '\\' => match escape(chars, false)? {
            Escaped::Char(c) => Node::Literal(c),
            Escaped::Set(class) => Node::Class(class),
        },
        '*' | '+' | '?' => return Err(format!("`{c}` repeats nothing")),
        '{' if quantifier_ahead(chars) => return Err("`{` repeats nothing".into()),
        c => Node::Literal(c),
    })
}

/// Whether `{` begins a well-formed `{n}`, `{n,}` or `{n,m}`; otherwise ECMA
/// reads it as a literal.
fn quantifier_ahead(chars: &Peekable<Chars>) -> bool {
    let rest: String = chars.clone().take_while(|&c| c != '}').collect();
    let closed = chars.clone().any(|c| c == '}');
    let (low, high) = rest.split_once(',').unwrap_or((&rest, &rest));
    closed
        && !low.is_empty()
        && low.chars().all(|c| c.is_ascii_digit())
        && high.chars().all(|c| c.is_ascii_digit())
}

fn quantified(chars: &mut Peekable<Chars>, node: Node) -> Result<Node, String> {
    let (min, max) = match chars.peek() {
        Some('*') => (0, None),
        Some('+') => (1, None),
        Some('?') => (0, Some(1)),
        Some('{') => {
            let mut ahead = chars.clone();
            ahead.next();
            if !quantifier_ahead(&ahead) {
                return Ok(node);
            }
            chars.next();
            let text: String = chars.by_ref().take_while(|&c| c != '}').collect();
            let number = |text: &str| {
                text.parse::<usize>()
                    .map_err(|_| format!("the quantifier {{{text}}} is too large"))
            };
            let bounds = match text.split_once(',') {
                None => (number(&text)?, Some(number(&text)?)),
                Some((low, "")) => (number(low)?, None),
                Some((low, high)) => (number(low)?, Some(number(high)?)),
            };
            if matches!(node, Node::Anchor) {
                return Err("a repeated anchor".into());
            }
            if chars.peek() == Some(&'?') {
                chars.next();
            }
            return Ok(Node::Repeat {
                node: Box::new(node),
                min: bounds.0,
                max: bounds.1,
            });
        }
        _ => return Ok(node),
    };
    chars.next();
    // A lazy quantifier matches the same strings.
    if chars.peek() == Some(&'?') {
        chars.next();
    }
    if matches!(node, Node::Anchor) {
        return Err("a repeated anchor".into());
    }
    Ok(Node::Repeat {
        node: Box::new(node),
        min,
        max,
    })
}

enum Escaped {
    Char(char),
    Set(Class),
}

fn escape(chars: &mut Peekable<Chars>, in_class: bool) -> Result<Escaped, String> {
    let c = chars.next().ok_or("a trailing `\\`")?;
    let hex = |chars: &mut Peekable<Chars>, count: usize| -> Result<char, String> {
        let digits: String = (0..count).filter_map(|_| chars.next()).collect();
        u32::from_str_radix(&digits, 16)
            .ok()
            .filter(|_| digits.len() == count)
            .and_then(char::from_u32)
            .ok_or_else(|| format!("the escape `\\{c}{digits}`"))
    };
    Ok(match c {
        'd' => Escaped::Set(Class::of(DIGIT, false)),
        'D' => Escaped::Set(Class::of(DIGIT, true)),
        'w' => Escaped::Set(Class::of(WORD, false)),
        'W' => Escaped::Set(Class::of(WORD, true)),
        's' => Escaped::Set(Class::of(SPACE, false)),
        'S' => Escaped::Set(Class::of(SPACE, true)),
        'b' if in_class => Escaped::Char('\u{8}'),
        'b' | 'B' => return Err("word boundaries are outside the subset".into()),
        'n' => Escaped::Char('\n'),
        'r' => Escaped::Char('\r'),
        't' => Escaped::Char('\t'),
        'f' => Escaped::Char('\u{c}'),
        'v' => Escaped::Char('\u{b}'),
        '0' if chars.peek().is_none_or(|c| !c.is_ascii_digit()) => Escaped::Char('\0'),
        'x' => Escaped::Char(hex(chars, 2)?),
        'u' => Escaped::Char(hex(chars, 4)?),
        'c' => {
            let letter = chars
                .next()
                .filter(char::is_ascii_alphabetic)
                .ok_or("a control escape without a letter")?;
            Escaped::Char(char::from(letter.to_ascii_uppercase() as u8 % 32))
        }
        c if c.is_ascii_alphanumeric() => {
            return Err(format!("the escape `\\{c}` is outside the subset"));
        }
        c => Escaped::Char(c),
    })
}

fn class(chars: &mut Peekable<Chars>) -> Result<Class, String> {
    let mut class = Class::default();
    if chars.peek() == Some(&'^') {
        chars.next();
        class.negated = true;
    }
    loop {
        let c = chars.next().ok_or("an unclosed character class")?;
        if c == ']' {
            return Ok(class);
        }
        let low = if c == '\\' {
            escape(chars, true)?
        } else {
            Escaped::Char(c)
        };
        let low = match low {
            Escaped::Set(set) => {
                if set.negated {
                    class.items.push(Item::Not(range_list(&set)));
                } else {
                    class.items.extend(range_items(&set));
                }
                continue;
            }
            Escaped::Char(c) => c,
        };
        let mut ahead = chars.clone();
        if ahead.next() == Some('-') && ahead.peek().is_some_and(|&c| c != ']') {
            chars.next();
            let c = chars.next().expect("peeked");
            let high = if c == '\\' {
                match escape(chars, true)? {
                    Escaped::Char(c) => c,
                    // `[a-\d]`: ECMA's Annex B reads the hyphen literally.
                    Escaped::Set(set) => {
                        class.items.push(Item::Range(low, low));
                        class.items.push(Item::Range('-', '-'));
                        class.items.extend(range_items(&set));
                        continue;
                    }
                }
            } else {
                c
            };
            if high < low {
                return Err(format!("the class range {low}-{high} is out of order"));
            }
            class.items.push(Item::Range(low, high));
        } else {
            class.items.push(Item::Range(low, low));
        }
    }
}

fn range_list(set: &Class) -> Vec<(char, char)> {
    set.items
        .iter()
        .filter_map(|item| match item {
            Item::Range(a, b) => Some((*a, *b)),
            Item::Not(_) => None,
        })
        .collect()
}

fn range_items(set: &Class) -> Vec<Item> {
    range_list(set)
        .into_iter()
        .map(|(a, b)| Item::Range(a, b))
        .collect()
}

/// The shortest and longest strings a node matches; `None` is unbounded.
fn bounds(node: &Node) -> (usize, Option<usize>) {
    match node {
        Node::Anchor => (0, Some(0)),
        Node::Literal(_) | Node::Class(_) => (1, Some(1)),
        Node::Concat(nodes) => nodes.iter().map(bounds).fold((0, Some(0)), |a, b| {
            (a.0 + b.0, a.1.zip(b.1).map(|(x, y)| x + y))
        }),
        Node::Alternation(nodes) => {
            let all: Vec<_> = nodes.iter().map(bounds).collect();
            (
                all.iter().map(|b| b.0).min().unwrap_or(0),
                all.iter().try_fold(0, |m, b| b.1.map(|x| m.max(x))),
            )
        }
        Node::Repeat { node, min, max } => {
            let (low, high) = bounds(node);
            let high = match (high, max) {
                (Some(0), _) => Some(0),
                (Some(h), Some(m)) => Some(h.saturating_mul(*m)),
                _ => None,
            };
            (low.saturating_mul(*min), high)
        }
    }
}

/// How many characters of the shortest match come from classes, which a
/// readable hint can fill, rather than from literals.
fn open_positions(node: &Node) -> usize {
    match node {
        Node::Anchor | Node::Literal(_) => 0,
        Node::Class(_) => 1,
        Node::Concat(nodes) => nodes.iter().map(open_positions).sum(),
        Node::Alternation(nodes) => nodes.iter().map(open_positions).min().unwrap_or(0),
        Node::Repeat { node, min, .. } => open_positions(node) * min,
    }
}

/// Deterministic choices: a readable hint where the pattern is open, and a
/// hash of the hint, seed and position where it fixes a run of characters
/// (an id's body or a hash), which reads better varied than spelled out.
struct Writer {
    hint: Vec<char>,
    cursor: usize,
    state: u64,
    /// Steps left: backtracking over nested repeats is bounded, and a
    /// pattern that exhausts it is reported rather than searched forever.
    budget: usize,
}

impl Writer {
    fn pick(&mut self, class: &Class, varied: bool) -> Option<char> {
        if !varied && !self.hint.is_empty() {
            let hinted = self.hint[self.cursor % self.hint.len()];
            self.cursor += 1;
            for c in [
                hinted,
                hinted.to_ascii_lowercase(),
                hinted.to_ascii_uppercase(),
            ] {
                if class.contains(c) {
                    return Some(c);
                }
            }
            return class.choices().first().copied();
        }
        let choices = class.choices();
        // FNV-1a step: the same model always yields the same characters.
        self.state = (self.state ^ 0x9e37_79b9).wrapping_mul(0x0100_0000_01b3);
        let index = (self.state >> 33) as usize % choices.len().max(1);
        choices.get(index).copied()
    }

    /// A string of exactly `len` characters matching `node`, if one is found.
    fn write(&mut self, node: &Node, len: usize, varied: bool) -> Option<String> {
        self.budget = self.budget.checked_sub(1)?;
        match node {
            Node::Anchor => (len == 0).then(String::new),
            Node::Literal(c) => (len == 1).then(|| c.to_string()),
            Node::Class(class) => {
                if len == 1 {
                    self.pick(class, varied).map(String::from)
                } else {
                    None
                }
            }
            Node::Alternation(nodes) => nodes.iter().find_map(|node| {
                let (low, high) = bounds(node);
                if len < low || high.is_some_and(|h| len > h) {
                    return None;
                }
                let saved = (self.cursor, self.state);
                let written = self.write(node, len, varied);
                if written.is_none() {
                    (self.cursor, self.state) = saved;
                }
                written
            }),
            Node::Concat(nodes) => {
                let all: Vec<_> = nodes.iter().map(bounds).collect();
                let least: usize = all.iter().map(|b| b.0).sum();
                if len < least {
                    return None;
                }
                // The extra length goes to the earliest children that can
                // take it; if that fails, to each one alone.
                let mut plans = vec![spread(&all, len - least)];
                for (index, b) in all.iter().enumerate() {
                    if b.1.is_none_or(|h| h - b.0 >= len - least) {
                        let mut plan: Vec<usize> = all.iter().map(|b| b.0).collect();
                        plan[index] += len - least;
                        plans.push(Some(plan));
                    }
                }
                plans.into_iter().flatten().find_map(|plan| {
                    let saved = (self.cursor, self.state);
                    let written = nodes
                        .iter()
                        .zip(plan)
                        .map(|(node, len)| self.write(node, len, varied))
                        .collect::<Option<String>>();
                    if written.is_none() {
                        (self.cursor, self.state) = saved;
                    }
                    written
                })
            }
            Node::Repeat { node, min, max } => {
                let (low, high) = bounds(node);
                let varied = varied || (max == &Some(*min) && *min >= 2);
                if len == 0 {
                    return (*min == 0 || low == 0)
                        .then(|| self.write_times(node, *min, 0, varied))
                        .flatten();
                }
                // Only counts whose copies can add up to `len` are tried.
                let first = (*min)
                    .max(1)
                    .max(high.filter(|&h| h > 0).map_or(1, |h| len.div_ceil(h)));
                let last = max
                    .unwrap_or(usize::MAX)
                    .min(len.checked_div(low).unwrap_or(len.max(first)));
                (first..=last).take(64).find_map(|count| {
                    if count * low > len || high.is_some_and(|h| count * h < len) {
                        return None;
                    }
                    self.write_times(node, count, len, varied)
                })
            }
        }
    }

    fn write_times(
        &mut self,
        node: &Node,
        count: usize,
        len: usize,
        varied: bool,
    ) -> Option<String> {
        let (low, high) = bounds(node);
        let each = vec![(low, high); count];
        let plan = spread(&each, len.checked_sub(low * count)?)?;
        let saved = (self.cursor, self.state);
        let written = plan
            .into_iter()
            .map(|len| self.write(node, len, varied))
            .collect::<Option<String>>();
        if written.is_none() {
            (self.cursor, self.state) = saved;
        }
        written
    }
}

/// Each child's length: its minimum, plus `extra` given to the earliest
/// children with room for it.
fn spread(all: &[(usize, Option<usize>)], mut extra: usize) -> Option<Vec<usize>> {
    let mut plan = Vec::with_capacity(all.len());
    for (low, high) in all {
        let room = high.map_or(extra, |h| (h - low).min(extra));
        plan.push(low + room);
        extra -= room;
    }
    (extra == 0).then_some(plan)
}

/// A string matching `pattern` whose length in characters is within
/// `min..=max`, readable where the pattern allows it: the open parts spell
/// out `hint` (a member name), fixed runs (`[0-9a-f]{32}`) vary. `seed`
/// selects another value of the same pattern, for distinct list items.
pub(crate) fn generate(
    pattern: &str,
    min: usize,
    max: Option<usize>,
    hint: &str,
    seed: u64,
) -> Result<String, String> {
    let node = parse(pattern)?;
    let (low, high) = bounds(&node);
    let from = low.max(min);
    let to = match (high, max) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    if to.is_some_and(|to| to < from) {
        return Err("no string matching the pattern fits the length bounds".into());
    }
    let hint: Vec<char> = if seed == 0 {
        hint.chars().collect()
    } else {
        format!("{hint}{seed}").chars().collect()
    };
    let preferred = low + hint.len().saturating_sub(open_positions(&node));
    let clamp = |len: usize| to.map_or(len, |to| len.min(to)).max(from);
    let mut lengths = vec![clamp(preferred)];
    lengths.extend(
        (from..)
            .take(64)
            .filter(|&len| to.is_none_or(|to| len <= to)),
    );
    // Varied runs follow the pattern and seed alone, not the member's name,
    // so one sample spells an id the same in a path and in its response.
    let state = pattern
        .chars()
        .fold(0xcbf2_9ce4_8422_2325_u64 ^ seed, |state, c| {
            (state ^ u64::from(c)).wrapping_mul(0x0100_0000_01b3)
        });
    for len in lengths {
        let mut writer = Writer {
            hint: hint.clone(),
            cursor: 0,
            state,
            budget: 20_000,
        };
        if let Some(text) = writer.write(&node, len, false) {
            return Ok(text);
        }
    }
    Err("no string matching the pattern was found".into())
}

#[cfg(test)]
mod tests {
    use super::generate;

    fn matches(pattern: &str, text: &str) -> bool {
        regex::Regex::new(pattern).unwrap().is_match(text)
    }

    #[test]
    fn typed_ids_keep_their_prefix_and_length() {
        let id = generate("^o_[0-9a-hjkmnp-tv-z]{26}$", 0, None, "organizationId", 0).unwrap();
        assert!(id.starts_with("o_") && id.len() == 28, "{id}");
        assert!(matches("^o_[0-9a-hjkmnp-tv-z]{26}$", &id));
        assert_eq!(
            id,
            generate("^o_[0-9a-hjkmnp-tv-z]{26}$", 0, None, "organizationId", 0).unwrap()
        );
        assert_ne!(
            id,
            generate("^o_[0-9a-hjkmnp-tv-z]{26}$", 0, None, "organizationId", 1).unwrap()
        );
    }

    #[test]
    fn open_parts_spell_the_hint() {
        assert_eq!(
            generate(
                "^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$",
                0,
                None,
                "graphName",
                0
            )
            .unwrap(),
            "graphname"
        );
        assert_eq!(
            generate("^[A-Za-z][A-Za-z0-9_]{0,62}$", 0, None, "propertyKey", 0).unwrap(),
            "propertyKey"
        );
    }

    #[test]
    fn the_subset_models_use_generates_matching_strings() {
        for pattern in [
            "^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$",
            "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
            "^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$",
            "^gbs-([a-z]{3}[0-9]-)?[0-9a-f]{32}$",
            "^arn:aws:iam::[0-9]{12}:role/([!-~]{1,510}/)?[A-Za-z0-9+=,.@_-]{1,64}$",
            "^[^\\x00-\\x1F\\x7F-\\x9F]+$",
            "^[^\\s\\x00-\\x1F\\x7F-\\x9F]([^\\x00-\\x1F\\x7F-\\x9F]*[^\\s\\x00-\\x1F\\x7F-\\x9F])?$",
            "^[^\\s@]+@[^\\s@]+$",
            "^[\\t\\x20-\\x7E]$",
            "^(0|[1-9][0-9]*)(\\.[0-9]{0,2}[1-9])?$",
            "^([*]|[ogh]_[0-9a-hjkmnp-tv-z]{26}|g_[0-9a-hjkmnp-tv-z]{26}_[a-z]{3})$",
            "^[a-zA-Z][a-zA-Z0-9]*([.][a-zA-Z][a-zA-Z0-9]*)+$",
            "^[A-Za-z0-9._/-]*(\\*[A-Za-z0-9._/-]*){0,4}$",
            "^[A-Za-z0-9+/]+={0,2}$",
            "^https://[^\\s]+$",
            "^a{2,}?b+?c*?$",
            "^\\d+\\.\\w+$",
            "^x{3}$",
            "^[\\D]$",
            "^[a-]$",
            "^\\u00e9\\x41\\cA$",
            "plain",
        ] {
            for seed in 0..3 {
                let text = generate(pattern, 0, None, "memberName", seed)
                    .unwrap_or_else(|e| panic!("{pattern}: {e}"));
                let rust = crucible_ir::ir::Constraints {
                    pattern: Some(pattern.into()),
                    ..Default::default()
                }
                .rust_pattern()
                .unwrap()
                .unwrap();
                assert!(matches(&rust, &text), "{pattern} -> {text:?}");
            }
        }
    }

    #[test]
    fn lengths_are_honoured_or_refused() {
        let text = generate("^[a-z]+$", 10, Some(12), "id", 0).unwrap();
        assert!((10..=12).contains(&text.len()), "{text}");
        // A long minimum lands in the open repeat.
        let token = generate(
            "^[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]{43}$",
            240,
            Some(1090),
            "token",
            0,
        )
        .unwrap();
        assert_eq!(token.len(), 240);
        assert!(matches("^[A-Za-z0-9_-]+\\.[A-Za-z0-9_-]{43}$", &token));
        assert!(generate("^[a-z]{3}$", 5, None, "id", 0).is_err());
    }

    #[test]
    fn constructs_outside_the_subset_are_refused() {
        for pattern in ["\\ba\\b", "^(?=a)a$", "^(a)\\1$", "^[]$", "^(?<n>a)$"] {
            assert!(generate(pattern, 0, None, "x", 0).is_err(), "{pattern}");
        }
    }
}
