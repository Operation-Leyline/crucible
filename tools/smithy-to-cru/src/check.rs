//! The self-check: compile the package, lower the Smithy model, compare the
//! two IRs field by field.

use std::path::Path;

use crucible_smithy::ir::{Service, Shape};
use serde_json::Value;

/// An operation's input or output shape the package names by its own rule.
struct IoRename {
    operation: String,
    input: bool,
    old: String,
    new: String,
}

#[derive(Debug, Clone)]
pub struct Difference {
    pub path: String,
    /// The Smithy side, after the deliberate normalizations.
    pub smithy: String,
    pub cru: String,
    /// Why the difference is deliberate, when it is.
    pub reason: Option<&'static str>,
}

#[derive(Debug, Default)]
pub struct Report {
    /// The package did not compile: the compiler's rendering.
    pub compile_errors: Option<String>,
    /// Warnings the compiler gave the package.
    pub warnings: Vec<String>,
    /// Normalizations applied to the Smithy IR before comparing, with counts.
    pub normalized: Vec<(String, usize)>,
    pub operations: usize,
    pub shapes: usize,
    pub expected: Vec<Difference>,
    pub unexpected: Vec<Difference>,
}

impl Report {
    pub fn is_clean(&self) -> bool {
        self.compile_errors.is_none() && self.unexpected.is_empty()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if let Some(e) = &self.compile_errors {
            out.push_str("the package does not compile:\n");
            out.push_str(e);
            return out;
        }
        out.push_str(&format!(
            "compared {} operations and {} shapes\n",
            self.operations, self.shapes
        ));
        for (what, n) in self.normalized.iter().filter(|(_, n)| *n > 0) {
            out.push_str(&format!("normalized on the Smithy side: {what} ({n})\n"));
        }
        let mut reasons: std::collections::BTreeMap<&str, usize> = Default::default();
        for d in &self.expected {
            *reasons.entry(d.reason.unwrap_or("")).or_default() += 1;
        }
        for (reason, n) in reasons {
            out.push_str(&format!("expected difference: {reason} ({n})\n"));
        }
        out.push_str(&format!("{} compiler warning(s)\n", self.warnings.len()));
        if self.unexpected.is_empty() {
            out.push_str("IR equal apart from the expected differences\n");
        } else {
            out.push_str(&format!(
                "{} UNEXPECTED difference(s):\n",
                self.unexpected.len()
            ));
            for d in &self.unexpected {
                out.push_str(&format!(
                    "  {}\n    smithy: {}\n    cru:    {}\n",
                    d.path,
                    clip(&d.smithy),
                    clip(&d.cru)
                ));
            }
        }
        out
    }
}

fn lower_smithy(json: &str, service: Option<&str>) -> Result<Service, String> {
    let model = crucible_smithy::Model::from_json(json).map_err(|e| e.to_string())?;
    let id = match service {
        Some(s) if s.contains('#') => Some(
            s.parse()
                .map_err(|e: crucible_smithy::ShapeIdError| e.to_string())?,
        ),
        Some(s) => model.services().into_iter().find(|id| id.name() == s),
        None => None,
    };
    let all = crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
    model
        .lower_with_capabilities(id.as_ref(), &all)
        .map_err(|e| e.to_string())
}

/// Compares the package in `package` with the Smithy model.
pub fn check(model_json: &str, package: &Path, service: Option<&str>) -> Result<Report, String> {
    let mut report = Report::default();
    let mut smithy = lower_smithy(model_json, service)?;
    let loaded = crucible_lang::load_package(package)?;
    let compiled =
        crucible_lang::compile(&loaded, service.map(|s| s.rsplit('#').next().unwrap_or(s)));
    report.warnings = compiled
        .render()
        .lines()
        .filter(|l| l.contains(": warning: "))
        .map(str::to_owned)
        .collect();
    let Some(cru) = compiled.service.clone() else {
        report.compile_errors = Some(compiled.render());
        return Ok(report);
    };
    report.operations = cru.operations.len();
    report.shapes = cru.shapes.len();
    // Input and output shapes the package names by its own rule.
    let mut renames = Vec::new();
    for op in &smithy.operations {
        if let Some(other) = cru.operations.iter().find(|o| o.id == op.id) {
            for (is_input, old, new) in [
                (true, &op.input, &other.input),
                (false, &op.output, &other.output),
            ] {
                if let (Some(old), Some(new)) = (old, new)
                    && old != new
                {
                    renames.push(IoRename {
                        operation: op.id.to_string(),
                        input: is_input,
                        old: old.to_string(),
                        new: new.to_string(),
                    });
                }
            }
        }
    }
    if !renames.is_empty() {
        report.normalized.push((
            "input/output shapes renamed by the package's naming rule".into(),
            renames.len(),
        ));
    }
    normalize(&mut smithy, &mut report.normalized);
    let mut expected = serde_json::to_value(&smithy).map_err(|e| e.to_string())?;
    let actual = serde_json::to_value(&cru).map_err(|e| e.to_string())?;
    apply_renames(&mut expected, &renames);
    let primitives = unprimitive(&mut expected);
    if primitives > 0 {
        report.normalized.push((
            "Primitive* prelude targets are the plain type with a default (decision 20)".into(),
            primitives,
        ));
    }
    let mut diffs = Vec::new();
    diff("", &expected, &actual, &mut diffs);
    for mut d in diffs {
        d.reason = classify(&d);
        if d.reason.is_some() {
            report.expected.push(d);
        } else {
            report.unexpected.push(d);
        }
    }
    Ok(report)
}

/// The deliberate front-end differences that can be applied to the Smithy IR
/// before comparing (decisions 20, 26 and 63).
fn normalize(s: &mut Service, notes: &mut Vec<(String, usize)>) {
    s.project_metadata_only();
    s.api_version = Some(s.version.clone());
    let mut client_optional = 0;
    for shape in s.shapes.values_mut() {
        if let Shape::Structure(st) | Shape::Union(st) = shape {
            for m in &mut st.members {
                if m.client_optional {
                    client_optional += 1;
                    m.client_optional = false;
                }
            }
        }
    }
    notes.push((
        "members of @input structures are required in clients, not client-optional (decision 20)"
            .into(),
        client_optional,
    ));
    let (mut tokened, mut verbs) = (0, 0);
    for op in &mut s.operations {
        if op.idempotent {
            continue;
        }
        if op.idempotency_token.is_some() {
            op.idempotent = true;
            tokened += 1;
        } else if matches!(op.method.as_str(), "PUT" | "DELETE") && !op.readonly {
            op.idempotent = true;
            verbs += 1;
        }
    }
    notes.push((
        "tokened operations are idempotent without the keyword (decision 26)".into(),
        tokened,
    ));
    notes.push((
        "PUT and DELETE operations must say `idempotent` in .cru (decision 26)".into(),
        verbs,
    ));
    // An operation's input is an `input` structure in .cru whether or not Smithy said @input.
    let inputs: Vec<_> = s
        .operations
        .iter()
        .filter_map(|o| o.input.clone())
        .collect();
    let mut flagged = 0;
    for id in inputs {
        if let Some(Shape::Structure(st)) = s.shapes.get_mut(&id)
            && !st.input
        {
            st.input = true;
            flagged += 1;
        }
    }
    notes.push((
        "operation input structures are inputs without @input (decision 25)".into(),
        flagged,
    ));
}

/// Input and output shapes the package names by its rule.
fn apply_renames(v: &mut Value, renames: &[IoRename]) {
    if renames.is_empty() {
        return;
    }
    let mut referenced_elsewhere: std::collections::BTreeSet<String> = Default::default();
    // Point each operation at its renamed shape, copying the shape.
    for r in renames {
        let shape = v["shapes"].get(&r.old).cloned();
        if let Some(shape) = shape {
            v["shapes"][&r.new] = shape;
        }
        if let Some(ops) = v["operations"].as_array_mut() {
            for op in ops {
                if op["id"].as_str() == Some(r.operation.as_str()) {
                    op[if r.input { "input" } else { "output" }] = Value::String(r.new.clone());
                }
            }
        }
    }
    // Requirements name members of the renamed shapes.
    if let Some(reqs) = v["requirements"].as_array_mut() {
        for req in reqs {
            if let Some(shape) = req["shape"].as_str().map(str::to_owned) {
                for r in renames {
                    if let Some(rest) = shape.strip_prefix(&format!("{}$", r.old)) {
                        req["shape"] = Value::String(format!("{}${rest}", r.new));
                    }
                }
            }
        }
    }
    // Keep the old shape only if something else still names it.
    fn scan(v: &Value, out: &mut std::collections::BTreeSet<String>) {
        match v {
            Value::String(s) => {
                out.insert(s.clone());
            }
            Value::Array(items) => items.iter().for_each(|i| scan(i, out)),
            Value::Object(map) => map.values().for_each(|val| scan(val, out)),
            _ => {}
        }
    }
    for (k, val) in v.as_object().into_iter().flatten() {
        if k != "shapes" {
            scan(val, &mut referenced_elsewhere);
        }
    }
    if let Some(shapes) = v["shapes"].as_object() {
        for shape in shapes.values() {
            scan(shape, &mut referenced_elsewhere);
        }
    }
    for r in renames {
        if !referenced_elsewhere.contains(&r.old)
            && let Some(shapes) = v["shapes"].as_object_mut()
        {
            shapes.remove(&r.old);
        }
    }
}

/// `smithy.api#PrimitiveLong` is `long = 0` in `.cru`: the plain prelude
/// shape, the default on the member. Returns how many references moved.
fn unprimitive(v: &mut Value) -> usize {
    const NAMES: &[&str] = &[
        "Boolean", "Byte", "Short", "Integer", "Long", "Float", "Double",
    ];
    fn walk(v: &mut Value, count: &mut usize) {
        match v {
            Value::String(s) => {
                if let Some(rest) = s.strip_prefix("smithy.api#Primitive")
                    && NAMES.contains(&rest)
                {
                    *s = format!("smithy.api#{rest}");
                    *count += 1;
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|i| walk(i, count)),
            Value::Object(map) => map.values_mut().for_each(|i| walk(i, count)),
            _ => {}
        }
    }
    let mut count = 0;
    if let Some(shapes) = v["shapes"].as_object_mut() {
        for name in NAMES {
            if let Some(shape) = shapes.remove(&format!("smithy.api#Primitive{name}")) {
                shapes.entry(format!("smithy.api#{name}")).or_insert(shape);
            }
        }
    }
    walk(v, &mut count);
    count
}

fn label(v: &Value, index: usize) -> String {
    for key in ["name", "id", "member"] {
        if let Some(s) = v.get(key).and_then(Value::as_str) {
            return s.to_owned();
        }
    }
    index.to_string()
}

fn short_value(v: &Value) -> String {
    v.to_string()
}

fn clip(text: &str) -> String {
    if text.len() > 240 {
        let mut end = 240;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &text[..end])
    } else {
        text.to_owned()
    }
}

fn diff(path: &str, a: &Value, b: &Value, out: &mut Vec<Difference>) {
    let join = |key: &str| {
        if path.is_empty() {
            key.to_owned()
        } else {
            format!("{path}.{key}")
        }
    };
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                diff(
                    &join(k),
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(&format!("{path}[{}]", label(p, i)), p, q, out);
            }
        }
        _ if a == b => {}
        _ => out.push(Difference {
            path: path.to_owned(),
            smithy: short_value(a),
            cru: short_value(b),
            reason: None,
        }),
    }
}

fn blanks(v: &str) -> String {
    v.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

fn classify(d: &Difference) -> Option<&'static str> {
    if d.path.ends_with(".mutability") && d.smithy == "null" && d.cru != "null" {
        return Some(
            "a member carrying a property's value takes the property's mutability (decision 34a)",
        );
    }
    if d.path.ends_with(".docs") && d.smithy.starts_with('"') && d.cru.starts_with('"') {
        let a: Value = serde_json::from_str(&d.smithy).unwrap_or_default();
        let b: Value = serde_json::from_str(&d.cru).unwrap_or_default();
        if let (Some(a), Some(b)) = (a.as_str(), b.as_str())
            && blanks(a) == blanks(b)
        {
            return Some("trailing whitespace in documentation lines");
        }
    }
    None
}
