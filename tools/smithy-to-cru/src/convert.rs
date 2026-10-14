//! Smithy JSON AST to `.cru` text.
//!
//! The converter reads what the author wrote (the AST), not the lowered IR,
//! and writes the idiom the language has for it: operations with inline
//! input and output, one file per operation, resources with role verbs and
//! `$name` elision, metadata declarations for custom traits. Anything `.cru`
//! cannot say becomes a [`Diagnostic`] naming the shape; nothing is dropped
//! silently.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::{
    self, Smithy, doc_of, get_trait, has_trait, kind, members, refs, short, target,
};
use crate::text::{self, is_ident, quote};

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The service to convert, as `namespace#Name` or `Name`, when the model has several.
    pub service: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The package would not mean the same thing.
    Error,
    /// Something was dropped or changed that does not touch the wire contract.
    Warning,
    /// A choice the converter made, for the reviewer.
    Note,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub shape: String,
    pub message: String,
}

#[derive(Debug)]
pub struct Converted {
    pub namespace: String,
    pub service: String,
    /// Package-relative path to file text, formatted.
    pub files: BTreeMap<String, String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn convert(json: &str, options: &Options) -> Result<Converted, String> {
    let smithy = Smithy::parse(json)?;
    let mut cx = Cx::new(&smithy);
    cx.run(options)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Creates,
    Puts,
    Reads,
    Updates,
    Deletes,
    Lists,
    On,
}

impl Role {
    fn word(self) -> &'static str {
        match self {
            Role::Creates => "creates",
            Role::Puts => "puts",
            Role::Reads => "reads",
            Role::Updates => "updates",
            Role::Deletes => "deletes",
            Role::Lists => "lists",
            Role::On => "on",
        }
    }
}

struct Attach {
    resource: String,
    role: Role,
    collection: bool,
}

#[derive(Default)]
struct Res {
    parent: Option<String>,
    /// Every identifier, inherited ones first: (name, target shape).
    ids: Vec<(String, String)>,
    own_ids: Vec<(String, String)>,
    props: Vec<(String, String)>,
    /// identifier name to the spelling its representations use.
    alias: BTreeMap<String, String>,
    written_create: BTreeSet<String>,
    written_update: BTreeSet<String>,
    site_seen: BTreeSet<String>,
    site_required: BTreeSet<String>,
}

enum MetaKind {
    Marker,
    Enum,
    Value,
}

struct MetaDef {
    kind: MetaKind,
    /// The wire value to the symbol written in a use.
    symbols: BTreeMap<String, String>,
    text: String,
}

/// How members elide into a resource.
#[derive(Clone)]
struct Elide {
    resource: String,
    input: bool,
    /// A collection operation binds the parent's identifiers only.
    collection: bool,
}

struct Cx<'a> {
    m: &'a Smithy,
    ns: String,
    diags: Vec<Diagnostic>,
    meta: BTreeMap<String, MetaDef>,
    resources: BTreeMap<String, Res>,
    attach: BTreeMap<String, Attach>,
    service_public: bool,
    /// Structure id to the resource it is a view of.
    views: BTreeMap<String, String>,
    io_shapes: BTreeSet<String>,
    naming: (String, String),
    imports: BTreeSet<String>,
}

const PRELUDE: &[(&str, &str)] = &[
    ("String", "string"),
    ("Boolean", "boolean"),
    ("Byte", "byte"),
    ("Short", "short"),
    ("Integer", "integer"),
    ("Long", "long"),
    ("Float", "float"),
    ("Double", "double"),
    ("BigInteger", "bigInteger"),
    ("BigDecimal", "bigDecimal"),
    ("Timestamp", "timestamp"),
    ("Blob", "blob"),
    ("Document", "document"),
    ("PrimitiveBoolean", "boolean"),
    ("PrimitiveByte", "byte"),
    ("PrimitiveShort", "short"),
    ("PrimitiveInteger", "integer"),
    ("PrimitiveLong", "long"),
    ("PrimitiveFloat", "float"),
    ("PrimitiveDouble", "double"),
];

/// Traits the converter reads itself, or that have no meaning in `.cru` and
/// are dropped on purpose (see `ignored`).
const HANDLED: &[&str] = &[
    "documentation",
    "required",
    "default",
    "enumValue",
    "length",
    "range",
    "pattern",
    "uniqueItems",
    "mediaType",
    "timestampFormat",
    "http",
    "httpLabel",
    "httpQuery",
    "httpHeader",
    "httpPayload",
    "httpError",
    "error",
    "retryable",
    "readonly",
    "idempotent",
    "idempotencyToken",
    "paginated",
    "sensitive",
    "internal",
    "deprecated",
    "since",
    "unstable",
    "externalDocumentation",
    "title",
    "httpBearerAuth",
    "auth",
    "input",
    "output",
    "examples",
    "trait",
    "unitType",
    "protocolDefinition",
    "notProperty",
    "nestedProperties",
    "resourceIdentifier",
    "suppress",
];

impl<'a> Cx<'a> {
    fn new(m: &'a Smithy) -> Self {
        Self {
            m,
            ns: String::new(),
            diags: Vec::new(),
            meta: BTreeMap::new(),
            resources: BTreeMap::new(),
            attach: BTreeMap::new(),
            service_public: false,
            views: BTreeMap::new(),
            io_shapes: BTreeSet::new(),
            naming: ("{Operation}Input".into(), "{Operation}Output".into()),
            imports: BTreeSet::new(),
        }
    }

    fn diag(&mut self, severity: Severity, shape: &str, message: impl Into<String>) {
        self.diags.push(Diagnostic {
            severity,
            shape: shape.to_owned(),
            message: message.into(),
        });
    }

    fn shape(&self, id: &str) -> Option<&'a Value> {
        self.m.shapes.get(id)
    }

    // ---- driver -----------------------------------------------------------

    fn run(&mut self, options: &Options) -> Result<Converted, String> {
        let services: Vec<&String> = self
            .m
            .shapes
            .iter()
            .filter(|(_, s)| kind(s) == "service")
            .map(|(id, _)| id)
            .collect();
        let service_id = match (&options.service, services.as_slice()) {
            (Some(want), _) => services
                .iter()
                .find(|id| **id == want || short(id) == want)
                .map(|id| (*id).clone())
                .ok_or_else(|| format!("no service {want}"))?,
            (None, [only]) => (*only).clone(),
            (None, []) => return Err("the model defines no service".into()),
            (None, many) => {
                return Err(format!(
                    "the model defines several services; choose one of {}",
                    many.iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        };
        let service = self.shape(&service_id).expect("service").clone();
        self.ns = model::namespace(&service_id).to_owned();
        self.check_service(&service_id, &service);
        self.collect_meta();
        let ops = self.collect_resources(&service_id, &service);
        self.choose_naming(&ops);
        self.find_views(&ops);
        self.find_summary_views();
        self.analyze(&ops);
        let files = self.emit(&service_id, &service, &ops);
        let mut formatted = BTreeMap::new();
        for (path, text) in files {
            formatted.insert(
                path,
                crucible_lang::format(&text, &crucible_lang::FormatOptions::default()),
            );
        }
        let mut diagnostics = std::mem::take(&mut self.diags);
        diagnostics.sort_by(|a, b| (&a.shape, &a.message).cmp(&(&b.shape, &b.message)));
        diagnostics.dedup_by(|a, b| a.shape == b.shape && a.message == b.message);
        Ok(Converted {
            namespace: self.ns.clone(),
            service: short(&service_id).to_owned(),
            files: formatted,
            diagnostics,
        })
    }

    fn check_service(&mut self, id: &str, service: &Value) {
        if let Some(t) = model::traits(service) {
            for key in t.keys() {
                match key.as_str() {
                    "crucible.protocols#json" | "smithy.api#httpBearerAuth" => {}
                    k if k.starts_with("smithy.api#") && HANDLED.contains(&&k[11..]) => {}
                    k if self.meta.contains_key(k) => {}
                    k if k.starts_with("aws.protocols#") || k.contains("Protocol") => {
                        self.diag(
                            Severity::Error,
                            id,
                            format!("protocol {k} is not supported: .cru has `protocol json` only"),
                        );
                    }
                    k => self.diag(
                        dropped_severity(k),
                        id,
                        format!("service trait {k} has no .cru equivalent and was dropped"),
                    ),
                }
            }
        }
        if service
            .get("rename")
            .is_some_and(|r| r.as_object().is_some_and(|o| !o.is_empty()))
        {
            self.diag(
                Severity::Error,
                id,
                "service `rename` is not supported (decision 9): rename the shapes in the model",
            );
        }
        if let Some(auth) = get_trait(service, "smithy.api#auth") {
            if auth.as_array().is_some_and(|a| a.is_empty()) {
                self.service_public = true;
            } else {
                self.diag(
                    Severity::Error,
                    id,
                    "@auth with schemes is not supported; use `auth bearer`",
                );
            }
        }
        for key in [
            "smithy.api#httpApiKeyAuth",
            "smithy.api#httpBasicAuth",
            "smithy.api#httpDigestAuth",
        ] {
            if has_trait(service, key) {
                self.diag(
                    Severity::Error,
                    id,
                    format!("{key} is not supported: .cru has `auth bearer` only"),
                );
            }
        }
    }

    // ---- custom traits -> metadata ---------------------------------------

    fn collect_meta(&mut self) {
        let defs: Vec<(String, &Value)> = self
            .m
            .shapes
            .iter()
            .filter(|(id, s)| {
                has_trait(s, "smithy.api#trait")
                    && !id.starts_with("smithy.api#")
                    && !id.starts_with("crucible.protocols#")
            })
            .map(|(id, s)| (id.clone(), s))
            .collect();
        for (id, shape) in defs {
            let name = short(&id).to_owned();
            let def = get_trait(shape, "smithy.api#trait")
                .cloned()
                .unwrap_or_default();
            let mut targets: Vec<&str> = Vec::new();
            match def.get("selector").and_then(Value::as_str) {
                Some(selector) => {
                    for part in selector.split(',') {
                        let part = part.trim();
                        let t = match part {
                            "operation" => "operation",
                            "service" => "service",
                            "resource" => "resource",
                            "structure" => "structure",
                            "union" => "union",
                            "member" | "structure > member" | "union > member" => "member",
                            "list" => "list",
                            "map" => "map",
                            "enum" => "enum",
                            "string" | "integer" | "long" | "boolean" | "timestamp" | "blob" => {
                                "scalar"
                            }
                            other => {
                                self.diag(
                                    Severity::Error,
                                    &id,
                                    format!("selector `{other}` has no .cru target kind"),
                                );
                                continue;
                            }
                        };
                        if part == "structure > member" || part == "union > member" {
                            self.diag(
                                Severity::Note,
                                &id,
                                format!("selector `{part}` is declared `on member`: .cru cannot narrow a member target"),
                            );
                        }
                        if !targets.contains(&t) {
                            targets.push(t);
                        }
                    }
                }
                None => {
                    self.diag(
                        Severity::Error,
                        &id,
                        "a trait with no selector applies everywhere; .cru needs target kinds",
                    );
                    continue;
                }
            }
            let (kind_, ty, symbols) = self.meta_type(&id, shape);
            let mut text = String::new();
            if let Some(d) = doc_of(model::traits(shape)) {
                text::doc(&mut text, d);
            }
            text.push_str(&format!("metadata {name} on {}", targets.join(", ")));
            if !ty.is_empty() {
                text.push_str(&format!(": {ty}"));
            }
            if let Some(conflicts) = def.get("conflicts").and_then(Value::as_array) {
                let names: Vec<String> = conflicts
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|c| short(c).to_owned())
                    .collect();
                if !names.is_empty() {
                    text.push_str(&format!(" conflicts {}", names.join(", ")));
                }
            }
            text.push('\n');
            self.meta.insert(
                id,
                MetaDef {
                    kind: kind_,
                    symbols,
                    text,
                },
            );
        }
    }

    fn meta_type(
        &mut self,
        id: &str,
        shape: &Value,
    ) -> (MetaKind, String, BTreeMap<String, String>) {
        let none = BTreeMap::new();
        match kind(shape) {
            "structure" if members(shape).is_empty() => (MetaKind::Marker, String::new(), none),
            "enum" => {
                let mut symbols = BTreeMap::new();
                let mut values = Vec::new();
                for (name, mem) in members(shape) {
                    let value = get_trait(mem, "smithy.api#enumValue")
                        .and_then(Value::as_str)
                        .unwrap_or(name);
                    symbols.insert(value.to_owned(), value.to_owned());
                    values.push(value.to_owned());
                }
                if values.iter().all(|v| is_ident(v)) {
                    (
                        MetaKind::Enum,
                        format!("enum {{ {} }}", values.join(", ")),
                        symbols,
                    )
                } else {
                    self.diag(
                        Severity::Warning,
                        id,
                        "enum values are not identifiers; the metadata is typed `string`",
                    );
                    (MetaKind::Value, "string".into(), none)
                }
            }
            "string" => {
                if let Some(values) = get_trait(shape, "smithy.api#pattern")
                    .and_then(Value::as_str)
                    .and_then(alternation)
                {
                    let symbols = values.iter().map(|v| (v.clone(), v.clone())).collect();
                    return (
                        MetaKind::Enum,
                        format!("enum {{ {} }}", values.join(", ")),
                        symbols,
                    );
                }
                let c = constraint_text(shape);
                (MetaKind::Value, format!("string{c}"), none)
            }
            "boolean" | "integer" | "long" | "short" | "byte" | "float" | "double" => {
                let c = constraint_text(shape);
                (MetaKind::Value, format!("{}{c}", kind(shape)), none)
            }
            other => {
                self.diag(
                    Severity::Error,
                    id,
                    format!(
                        "a custom trait of type {other} with members has no .cru metadata form"
                    ),
                );
                (MetaKind::Marker, String::new(), none)
            }
        }
    }

    /// `@key(value)` annotations for the custom traits in `traits`.
    fn meta_uses(&self, traits: Option<&serde_json::Map<String, Value>>) -> Vec<String> {
        let mut out = Vec::new();
        let Some(traits) = traits else { return out };
        for (key, v) in traits {
            let Some(def) = self.meta.get(key) else {
                continue;
            };
            let name = short(key);
            match def.kind {
                MetaKind::Marker => out.push(format!("@{name}")),
                MetaKind::Enum => {
                    let wire = v.as_str().unwrap_or_default();
                    let symbol = def
                        .symbols
                        .get(wire)
                        .cloned()
                        .unwrap_or_else(|| wire.to_owned());
                    out.push(format!("@{name}({symbol})"));
                }
                MetaKind::Value => {
                    if let Some(t) = text::value(v) {
                        out.push(format!("@{name}({t})"));
                    }
                }
            }
        }
        out
    }

    /// Annotations for documentation notes and custom traits, and a warning
    /// for every trait nobody handles.
    fn annotations(
        &mut self,
        id: &str,
        traits: Option<&serde_json::Map<String, Value>>,
    ) -> Vec<String> {
        let mut out = Vec::new();
        let Some(t) = traits else { return out };
        if let Some(d) = t.get("smithy.api#deprecated") {
            let mut args = Vec::new();
            if let Some(s) = d.get("since").and_then(Value::as_str) {
                args.push(format!("since: {}", quote(s)));
            }
            if let Some(s) = d.get("message").and_then(Value::as_str) {
                args.push(format!("message: {}", quote(s)));
            }
            out.push(if args.is_empty() {
                "@deprecated".to_owned()
            } else {
                format!("@deprecated({})", args.join(", "))
            });
        }
        if let Some(s) = t.get("smithy.api#since").and_then(Value::as_str) {
            out.push(format!("@since({})", quote(s)));
        }
        if t.contains_key("smithy.api#unstable") {
            out.push("@unstable".into());
        }
        if let Some(docs) = t
            .get("smithy.api#externalDocumentation")
            .and_then(Value::as_object)
        {
            let args: Vec<String> = docs
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|u| format!("{} = {}", quote(k), quote(u))))
                .collect();
            out.push(format!("@externalDocs({})", args.join(", ")));
        }
        out.extend(self.meta_uses(Some(t)));
        for key in t.keys() {
            if self.meta.contains_key(key) {
                continue;
            }
            if let Some(name) = key.strip_prefix("smithy.api#") {
                if HANDLED.contains(&name) {
                    continue;
                }
            } else if key.starts_with("crucible.protocols#") {
                continue;
            }
            self.diag(
                dropped_severity(key),
                id,
                format!("trait {key} has no .cru equivalent and was dropped"),
            );
        }
        out
    }

    // ---- resources --------------------------------------------------------

    /// Every operation the service reaches, with its attachment recorded.
    fn collect_resources(&mut self, service_id: &str, service: &Value) -> Vec<String> {
        let mut ops: Vec<String> = refs(service, "operations");
        let roots = refs(service, "resources");
        let mut stack: Vec<(String, Option<String>)> =
            roots.iter().rev().map(|r| (r.clone(), None)).collect();
        while let Some((id, parent)) = stack.pop() {
            let Some(shape) = self.shape(&id) else {
                self.diag(
                    Severity::Error,
                    service_id,
                    format!("resource {id} is not in the model"),
                );
                continue;
            };
            if self.resources.contains_key(&id) {
                continue;
            }
            let mut res = Res {
                parent: parent.clone(),
                ..Res::default()
            };
            let ids: Vec<(String, String)> = shape
                .get("identifiers")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.clone(), target(v).to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            res.ids = ids;
            res.props = shape
                .get("properties")
                .and_then(Value::as_object)
                .map(|o| {
                    o.iter()
                        .map(|(k, v)| (k.clone(), target(v).to_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let slots = [
                ("create", Role::Creates, true),
                ("put", Role::Puts, false),
                ("read", Role::Reads, false),
                ("update", Role::Updates, false),
                ("delete", Role::Deletes, false),
                ("list", Role::Lists, true),
            ];
            for (slot, role, collection) in slots {
                if let Some(op) = model::single_ref(shape, slot) {
                    self.bind_op(&op, &id, role, collection, &mut ops);
                }
            }
            for op in refs(shape, "operations") {
                self.bind_op(&op, &id, Role::On, false, &mut ops);
            }
            for op in refs(shape, "collectionOperations") {
                self.bind_op(&op, &id, Role::On, true, &mut ops);
            }
            self.resources.insert(id.clone(), res);
            for child in refs(shape, "resources").into_iter().rev() {
                stack.push((child, Some(id.clone())));
            }
        }
        self.finish_resources();
        ops.sort();
        ops.dedup();
        ops
    }

    /// Parents and own identifiers. Smithy lists a resource under the service
    /// even when its identifiers extend another resource's (Leyline's `Graph`
    /// is addressed by `organizationId` and `graphId`); `.cru` nests it `in`
    /// the resource whose identifiers it extends, which changes no contract.
    fn finish_resources(&mut self) {
        let names: Vec<String> = self.resources.keys().cloned().collect();
        for name in &names {
            if self.resources[name].parent.is_some() {
                continue;
            }
            let ids = self.resources[name].ids.clone();
            let best = names
                .iter()
                .filter(|other| *other != name)
                .filter(|other| {
                    let o = &self.resources[*other].ids;
                    !o.is_empty() && o.len() < ids.len() && o.iter().all(|i| ids.contains(i))
                })
                .max_by_key(|other| self.resources[*other].ids.len())
                .cloned();
            if let Some(p) = best {
                self.diag(
                    Severity::Note,
                    name,
                    format!(
                        "declared `in {}`: its identifiers extend that resource's",
                        short(&p)
                    ),
                );
                self.resources.get_mut(name).expect("resource").parent = Some(p);
            }
        }
        for name in &names {
            let parent_ids = self.resources[name]
                .parent
                .as_ref()
                .map(|p| self.resources[p].ids.clone())
                .unwrap_or_default();
            let r = self.resources.get_mut(name).expect("resource");
            r.own_ids = r
                .ids
                .iter()
                .filter(|i| !parent_ids.contains(i))
                .cloned()
                .collect();
            // Parent identifiers first, as the language orders them.
            r.ids.sort_by_key(|(n, _)| {
                parent_ids
                    .iter()
                    .position(|(p, _)| p == n)
                    .unwrap_or(usize::MAX)
            });
        }
    }

    fn bind_op(
        &mut self,
        op: &str,
        resource: &str,
        role: Role,
        collection: bool,
        ops: &mut Vec<String>,
    ) {
        if let Some(existing) = self.attach.get(op) {
            self.diag(
                Severity::Error,
                op,
                format!(
                    "bound to both {} and {}: .cru attaches an operation to at most one resource",
                    short(&existing.resource),
                    short(resource)
                ),
            );
            return;
        }
        self.attach.insert(
            op.to_owned(),
            Attach {
                resource: resource.to_owned(),
                role,
                collection,
            },
        );
        ops.push(op.to_owned());
    }

    // ---- input and output naming -----------------------------------------

    fn io_of(&self, op: &str) -> (Option<String>, Option<String>) {
        let shape = self.shape(op).expect("operation");
        let pick = |field: &str| model::single_ref(shape, field).filter(|t| t != "smithy.api#Unit");
        (pick("input"), pick("output"))
    }

    fn choose_naming(&mut self, ops: &[String]) {
        let candidates = [("Input", "Output"), ("Request", "Response")];
        let mut best = (0usize, 0usize);
        for (i, (si, so)) in candidates.iter().enumerate() {
            let score = ops
                .iter()
                .map(|op| {
                    let (inp, out) = self.io_of(op);
                    let name = short(op);
                    usize::from(inp.is_some_and(|t| short(&t) == format!("{name}{si}")))
                        + usize::from(out.is_some_and(|t| short(&t) == format!("{name}{so}")))
                })
                .sum();
            if score > best.0 {
                best = (score, i);
            }
        }
        let (si, so) = candidates[best.1];
        self.naming = (format!("{{Operation}}{si}"), format!("{{Operation}}{so}"));
        for op in ops {
            let (i, o) = self.io_of(op);
            self.io_shapes.extend(i);
            self.io_shapes.extend(o);
        }
    }

    // ---- views and property analysis -------------------------------------

    /// Which structures represent a resource: the targets of
    /// `nestedProperties` members in the outputs of its operations.
    fn find_views(&mut self, ops: &[String]) {
        for op in ops {
            let Some(att) = self.attach.get(op) else {
                continue;
            };
            let resource = att.resource.clone();
            let (_, out) = self.io_of(op);
            let Some(out) = out else { continue };
            let mut stack = vec![out];
            let mut seen = BTreeSet::new();
            while let Some(sid) = stack.pop() {
                if !seen.insert(sid.clone()) {
                    continue;
                }
                let Some(s) = self.shape(&sid) else { continue };
                for (_, mem) in members(s) {
                    if has_trait(mem, "smithy.api#nestedProperties") {
                        let mut t = target(mem).to_owned();
                        // A list or map of views.
                        loop {
                            match self.shape(&t).map(|s| (kind(s), s)) {
                                Some(("list", s)) | Some(("set", s)) => {
                                    t = s
                                        .get("member")
                                        .map(|m| target(m).to_owned())
                                        .unwrap_or_default();
                                }
                                Some(("map", s)) => {
                                    t = s
                                        .get("value")
                                        .map(|m| target(m).to_owned())
                                        .unwrap_or_default();
                                }
                                _ => break,
                            }
                        }
                        if self.shape(&t).is_some_and(|s| kind(s) == "structure") {
                            match self.views.get(&t) {
                                Some(existing) if *existing != resource => {
                                    let msg = format!(
                                        "represents both {} and {}; kept as a view of {}",
                                        short(existing),
                                        short(&resource),
                                        short(existing)
                                    );
                                    self.diag(Severity::Warning, &t, msg);
                                }
                                Some(_) => {}
                                None => {
                                    self.views.insert(t.clone(), resource.clone());
                                    stack.push(t);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Structures that restate a resource's identifier and a property by name
    /// and type are views of it (decision 34h), nested or not.
    fn find_summary_views(&mut self) {
        let candidates: Vec<(String, &Value)> = self
            .m
            .shapes
            .iter()
            .filter(|(id, s)| {
                kind(s) == "structure"
                    && model::namespace(id) == self.ns
                    && !self.io_shapes.contains(*id)
                    && !has_trait(s, "smithy.api#error")
                    && !self.meta.contains_key(*id)
                    && !self.views.contains_key(*id)
            })
            .map(|(id, s)| (id.clone(), s))
            .collect();
        for (id, shape) in candidates {
            let mut best: Option<(usize, String)> = None;
            for (res, r) in &self.resources {
                let mut own = 0;
                let mut props = 0;
                for (name, mem) in members(shape) {
                    if let Some((idn, _)) = self.id_match(res, name, mem, false, false)
                        && r.own_ids.iter().any(|(n, _)| *n == idn)
                    {
                        own += 1;
                    } else if self.prop_match(res, name, mem).is_some() {
                        props += 1;
                    }
                }
                if own >= 1
                    && props >= 1
                    && best.as_ref().is_none_or(|(score, _)| own + props > *score)
                {
                    best = Some((own + props, res.clone()));
                }
            }
            if let Some((_, res)) = best {
                self.views.insert(id, res);
            }
        }
    }

    /// The identifier a member binds, and the name the elision would give it.
    fn id_match(
        &self,
        res: &str,
        name: &str,
        mem: &Value,
        input: bool,
        collection: bool,
    ) -> Option<(String, bool)> {
        let r = self.resources.get(res)?;
        let declared = get_trait(mem, "smithy.api#resourceIdentifier").and_then(Value::as_str);
        let id_name = declared.unwrap_or(name);
        let (idn, idt) = r.ids.iter().find(|(n, _)| n == id_name)?;
        if target(mem) != idt {
            return None;
        }
        let own = r.own_ids.iter().any(|(n, _)| n == idn);
        if collection && own {
            return None;
        }
        let default = if !input && own {
            r.alias.get(idn).cloned().unwrap_or_else(|| idn.clone())
        } else {
            idn.clone()
        };
        Some((idn.clone(), default == name))
    }

    fn prop_match(&self, res: &str, name: &str, mem: &Value) -> Option<String> {
        let r = self.resources.get(res)?;
        // `@notProperty` is Smithy's workaround for its name matching;
        // binding is positive here (decision 33), so it does not stop an
        // elision of a member that is the property in name and type.
        if has_trait(mem, "smithy.api#default") {
            return None;
        }
        for own in [
            "length",
            "range",
            "pattern",
            "uniqueItems",
            "timestampFormat",
            "mediaType",
        ] {
            if has_trait(mem, &format!("smithy.api#{own}")) {
                return None;
            }
        }
        let (n, t) = r.props.iter().find(|(n, _)| n == name)?;
        (target(mem) == t).then(|| n.clone())
    }

    fn analyze(&mut self, ops: &[String]) {
        // Identifier spellings from views and outputs.
        let mut spellings: BTreeMap<(String, String), BTreeMap<String, usize>> = BTreeMap::new();
        let mut sites: Vec<(String, String, bool)> = Vec::new(); // (resource, prop, required)
        let mut writes: Vec<(String, String, bool, bool)> = Vec::new(); // (resource, prop, create, update)
        let view_list: Vec<(String, String)> = self
            .views
            .iter()
            .map(|(s, r)| (s.clone(), r.clone()))
            .collect();
        let mut output_structs: Vec<(String, String)> = view_list.clone();
        for op in ops {
            let Some(att) = self.attach.get(op) else {
                continue;
            };
            let (inp, out) = self.io_of(op);
            if let Some(out) = out {
                output_structs.push((out, att.resource.clone()));
            }
            if let Some(inp) = inp
                && let Some(s) = self.shape(&inp)
            {
                let (create, update) = match (att.role, att.collection) {
                    (Role::Creates, _) => (true, false),
                    (Role::On, true) => (true, false),
                    (Role::Puts, _) => (true, true),
                    (Role::Updates, _) => (false, true),
                    (Role::On, false) => (false, true),
                    _ => (false, false),
                };
                for (name, mem) in members(s) {
                    if has_trait(mem, "smithy.api#httpQuery") {
                        continue;
                    }
                    if let Some(p) = self.prop_match(&att.resource, name, mem) {
                        writes.push((att.resource.clone(), p, create, update));
                    }
                }
            }
        }
        for (sid, res) in &output_structs {
            let Some(s) = self.shape(sid) else { continue };
            for (name, mem) in members(s) {
                if let Some(p) = self.prop_match(res, name, mem) {
                    sites.push((res.clone(), p, has_trait(mem, "smithy.api#required")));
                } else if has_trait(mem, "smithy.api#resourceIdentifier")
                    && let Some(idn) =
                        get_trait(mem, "smithy.api#resourceIdentifier").and_then(Value::as_str)
                {
                    *spellings
                        .entry((res.clone(), idn.to_owned()))
                        .or_default()
                        .entry(name.clone())
                        .or_default() += 1;
                }
            }
        }
        for ((res, idn), counts) in spellings {
            let best = counts
                .iter()
                .max_by_key(|(n, c)| (**c, std::cmp::Reverse((*n).clone())));
            if let Some((name, _)) = best
                && *name != idn
                && let Some(r) = self.resources.get_mut(&res)
                && r.own_ids.iter().any(|(n, _)| *n == idn)
            {
                r.alias.insert(idn, name.clone());
            }
        }
        for (res, p, required) in sites {
            if let Some(r) = self.resources.get_mut(&res) {
                r.site_seen.insert(p.clone());
                if required {
                    r.site_required.insert(p);
                }
            }
        }
        for (res, p, create, update) in writes {
            if let Some(r) = self.resources.get_mut(&res) {
                if create {
                    r.written_create.insert(p.clone());
                }
                if update {
                    r.written_update.insert(p);
                }
            }
        }
    }

    // ---- emission ---------------------------------------------------------

    fn ty(&mut self, owner: &str, target_id: &str) -> String {
        if let Some(name) = target_id.strip_prefix("smithy.api#") {
            if let Some((_, kw)) = PRELUDE.iter().find(|(n, _)| *n == name) {
                return (*kw).to_owned();
            }
            self.diag(
                Severity::Error,
                owner,
                format!("prelude shape {target_id} has no .cru type"),
            );
            return "string".into();
        }
        let ns = model::namespace(target_id);
        if ns != self.ns {
            if ns == "crucible.errors" {
                self.imports.insert(ns.to_owned());
            } else {
                self.diag(
                    Severity::Error,
                    owner,
                    format!("{target_id} is in another namespace; a .cru package has one"),
                );
            }
        }
        short(target_id).to_owned()
    }

    /// `{ length 1..5; pattern ".." }` for the constraint traits in `traits`.
    fn constraints(traits: Option<&serde_json::Map<String, Value>>) -> String {
        let Some(t) = traits else {
            return String::new();
        };
        let mut parts = Vec::new();
        if let Some(v) = t.get("smithy.api#length") {
            parts.push(format!("length {}", range(v)));
        }
        if let Some(v) = t.get("smithy.api#range") {
            parts.push(format!("range {}", range(v)));
        }
        if let Some(p) = t.get("smithy.api#pattern").and_then(Value::as_str) {
            parts.push(format!("pattern {}", quote(p)));
        }
        if t.contains_key("smithy.api#uniqueItems") {
            parts.push("unique".into());
        }
        if let Some(f) = t.get("smithy.api#timestampFormat").and_then(Value::as_str) {
            parts.push(format!("format {f}"));
        }
        if let Some(m) = t.get("smithy.api#mediaType").and_then(Value::as_str) {
            parts.push(format!("mediaType {}", quote(m)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" {{ {} }}", parts.join("; "))
        }
    }

    fn default_literal(&mut self, owner: &str, target_id: &str, default: &Value) -> Option<String> {
        if let Some(s) = self.shape(target_id)
            && kind(s) == "enum"
        {
            let wire = default.as_str()?;
            for (name, mem) in members(s) {
                let value = get_trait(mem, "smithy.api#enumValue")
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                if value == wire {
                    return Some(name.clone());
                }
            }
            self.diag(Severity::Error, owner, "default names no enum value");
            return None;
        }
        text::value(default)
    }

    fn member_default(&self, mem: &Value) -> Option<Value> {
        if let Some(d) = get_trait(mem, "smithy.api#default") {
            return Some(d.clone());
        }
        let t = target(mem);
        if let Some(name) = t.strip_prefix("smithy.api#") {
            return match name {
                "PrimitiveBoolean" => Some(Value::Bool(false)),
                "PrimitiveByte" | "PrimitiveShort" | "PrimitiveInteger" | "PrimitiveLong"
                | "PrimitiveFloat" | "PrimitiveDouble" => Some(Value::from(0)),
                _ => None,
            };
        }
        self.shape(t)
            .and_then(|s| get_trait(s, "smithy.api#default"))
            .cloned()
    }

    /// One member in a structure, input, output, error or union.
    fn member(
        &mut self,
        owner: &str,
        name: &str,
        mem: &Value,
        union: bool,
        elide: Option<&Elide>,
    ) -> String {
        let mut out = String::new();
        let traits = model::traits(mem);
        if let Some(d) = doc_of(traits) {
            text::doc(&mut out, d);
        }
        let required = has_trait(mem, "smithy.api#required");
        // Property elision.
        if let Some(e) = elide {
            if let Some((id, default_name)) =
                self.id_match(&e.resource, name, mem, e.input, e.collection)
            {
                let mut anns = self.annotations(&format!("{owner}${name}"), traits);
                let mods = member_modifiers(mem);
                if has_trait(mem, "smithy.api#httpQuery") {
                    anns.push(query(mem, name));
                }
                if has_trait(mem, "smithy.api#httpHeader") {
                    anns.push(header(mem));
                }
                for a in anns {
                    out.push_str(&a);
                    out.push('\n');
                }
                out.push_str(&mods);
                out.push_str(&format!("${id}"));
                if !default_name {
                    out.push_str(&format!(" as {name}"));
                }
                if !required && !union {
                    out.push('?');
                }
                out.push('\n');
                return out;
            }
            if let Some(prop) = self.prop_match(&e.resource, name, mem) {
                let r = &self.resources[&e.resource];
                let prop_optional = r.site_seen.contains(&prop) && !r.site_required.contains(&prop);
                let mut anns = self.annotations(&format!("{owner}${name}"), traits);
                let mods = member_modifiers(mem);
                if has_trait(mem, "smithy.api#httpQuery") {
                    anns.push(query(mem, name));
                }
                if has_trait(mem, "smithy.api#httpHeader") {
                    anns.push(header(mem));
                }
                if has_trait(mem, "smithy.api#httpPayload") {
                    anns.push("@payload".into());
                }
                if has_trait(mem, "smithy.api#idempotencyToken") {
                    anns.push("@token".into());
                }
                for a in anns {
                    out.push_str(&a);
                    out.push('\n');
                }
                out.push_str(&mods);
                out.push_str(&format!("${prop}"));
                let optional_here = !required;
                if e.input {
                    if optional_here {
                        out.push('?');
                    }
                } else if optional_here && !prop_optional {
                    out.push('?');
                }
                out.push('\n');
                return out;
            }
        }
        let id = format!("{owner}${name}");
        let mut anns = self.annotations(&id, traits);
        if let Some(t) = traits {
            for unsupported in [
                "smithy.api#httpQueryParams",
                "smithy.api#httpPrefixHeaders",
                "smithy.api#httpResponseCode",
                "smithy.api#jsonName",
                "smithy.api#clientOptional",
                "smithy.api#streaming",
            ] {
                if t.contains_key(unsupported) {
                    self.diag(
                        Severity::Error,
                        &id,
                        format!("{unsupported} has no .cru syntax"),
                    );
                }
            }
        }
        if has_trait(mem, "smithy.api#httpQuery") {
            anns.push(query(mem, name));
        }
        if has_trait(mem, "smithy.api#httpHeader") {
            anns.push(header(mem));
        }
        if has_trait(mem, "smithy.api#httpPayload") {
            anns.push("@payload".into());
        }
        if has_trait(mem, "smithy.api#idempotencyToken") {
            anns.push("@token".into());
        }
        for a in anns {
            out.push_str(&a);
            out.push('\n');
        }
        out.push_str(&member_modifiers(mem));
        out.push_str(name);
        let t = target(mem);
        if union && t == "smithy.api#Unit" {
            out.push('\n');
            return out;
        }
        let default = if union {
            None
        } else {
            self.member_default(mem)
        };
        let default = default.filter(|d| !d.is_null());
        if !union && default.is_none() && !required {
            out.push('?');
        }
        out.push_str(": ");
        out.push_str(&self.ty(&id, t));
        out.push_str(&Self::constraints(traits));
        if let Some(d) = default
            && let Some(lit) = self.default_literal(&id, t, &d)
        {
            out.push_str(&format!(" = {lit}"));
        }
        out.push('\n');
        out
    }

    fn members_block(
        &mut self,
        owner: &str,
        shape: &Value,
        union: bool,
        elide: Option<&Elide>,
        skip_tail: &[&str],
    ) -> String {
        let mut out = String::new();
        for (name, mem) in members(shape) {
            if skip_tail.contains(&name.as_str()) {
                continue;
            }
            out.push_str(&self.member(owner, name, mem, union, elide));
        }
        out
    }

    /// Declarations for one non-operation, non-resource shape.
    fn declaration(&mut self, id: &str, shape: &Value) -> Option<String> {
        let k = kind(shape);
        let name = short(id);
        let traits = model::traits(shape);
        let mut out = String::new();
        if let Some(d) = doc_of(traits) {
            text::doc(&mut out, d);
        }
        for a in self.annotations(id, traits) {
            out.push_str(&a);
            out.push('\n');
        }
        let mut modifiers = String::new();
        if has_trait(shape, "smithy.api#sensitive") {
            modifiers.push_str("sensitive ");
        }
        if has_trait(shape, "smithy.api#internal") {
            modifiers.push_str("internal ");
        }
        match k {
            "string" | "boolean" | "byte" | "short" | "integer" | "long" | "float" | "double"
            | "bigInteger" | "bigDecimal" | "timestamp" | "blob" | "document" => {
                if has_trait(shape, "smithy.api#streaming") {
                    self.diag(Severity::Error, id, "@streaming has no .cru syntax");
                }
                out.push_str(&format!(
                    "{modifiers}{k} {name}{}\n",
                    Self::constraints(traits)
                ));
            }
            "enum" | "intEnum" => {
                for unsupported in [
                    "smithy.api#mediaType",
                    "smithy.api#length",
                    "smithy.api#pattern",
                ] {
                    if has_trait(shape, unsupported) {
                        self.diag(
                            Severity::Error,
                            id,
                            format!("{unsupported} on an enum has no .cru syntax"),
                        );
                    }
                }
                out.push_str(&format!(
                    "{modifiers}enum {name}{} {{\n",
                    if k == "intEnum" { ": integer" } else { "" }
                ));
                for (vname, v) in members(shape) {
                    let t = model::traits(v);
                    if let Some(d) = doc_of(t) {
                        text::doc(&mut out, d);
                    }
                    for a in self.annotations(&format!("{id}${vname}"), t) {
                        out.push_str(&a);
                        out.push('\n');
                    }
                    if has_trait(v, "smithy.api#internal") {
                        out.push_str("internal ");
                    }
                    out.push_str(vname);
                    let value = get_trait(v, "smithy.api#enumValue");
                    match value {
                        Some(Value::String(s)) if s == vname => {}
                        Some(v) => {
                            if let Some(l) = text::value(v) {
                                out.push_str(&format!(" = {l}"));
                            }
                        }
                        None if k == "intEnum" => {
                            self.diag(Severity::Error, id, "intEnum value without a number");
                        }
                        None => {}
                    }
                    out.push('\n');
                }
                out.push_str("}\n");
            }
            "list" | "set" => {
                if k == "set" {
                    self.diag(
                        Severity::Error,
                        id,
                        "set is not supported (decision 14); use a list with `unique`",
                    );
                }
                let mem = shape.get("member").cloned().unwrap_or_default();
                self.member_traits_unsupported(id, &mem);
                let t = self.ty(id, target(&mem));
                if has_trait(shape, "smithy.api#sparse") {
                    self.diag(
                        Severity::Error,
                        id,
                        "sparse lists are not supported (decision 14)",
                    );
                }
                out.push_str(&format!(
                    "{modifiers}list {name}: {t}{}\n",
                    Self::constraints(traits)
                ));
            }
            "map" => {
                let key = shape.get("key").cloned().unwrap_or_default();
                let value = shape.get("value").cloned().unwrap_or_default();
                self.member_traits_unsupported(id, &key);
                self.member_traits_unsupported(id, &value);
                let kt = self.ty(id, target(&key));
                let vt = self.ty(id, target(&value));
                if has_trait(shape, "smithy.api#sparse") {
                    self.diag(
                        Severity::Error,
                        id,
                        "sparse maps are not supported (decision 14)",
                    );
                }
                out.push_str(&format!(
                    "{modifiers}map {name}: {kt} -> {vt}{}\n",
                    Self::constraints(traits)
                ));
            }
            "union" => {
                out.push_str(&format!("{modifiers}union {name} {{\n"));
                out.push_str(&self.members_block(id, shape, true, None, &[]));
                out.push_str("}\n");
            }
            "structure" => {
                if has_trait(shape, "smithy.api#error") {
                    out.push_str(&format!("{modifiers}error {name} {{\n"));
                    let fault = get_trait(shape, "smithy.api#error")
                        .and_then(Value::as_str)
                        .unwrap_or("client");
                    let status = get_trait(shape, "smithy.api#httpError")
                        .and_then(Value::as_u64)
                        .unwrap_or(if fault == "server" { 500 } else { 400 });
                    out.push_str(&format!("status {status}\n"));
                    if let Some(r) = get_trait(shape, "smithy.api#retryable") {
                        let throttling = r
                            .get("throttling")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        out.push_str(if throttling {
                            "retryable throttling\n"
                        } else {
                            "retryable\n"
                        });
                    }
                    out.push_str(&self.members_block(id, shape, false, None, &[]));
                    out.push_str("}\n");
                } else {
                    let strict = has_trait(shape, "crucible.protocols#strict");
                    let view = self.views.get(id).cloned();
                    let header = match &view {
                        Some(r) => format!(" for {}", short(r)),
                        None => String::new(),
                    };
                    let elide = view.map(|resource| Elide {
                        resource,
                        input: false,
                        collection: false,
                    });
                    out.push_str(&format!(
                        "{modifiers}{}structure {name}{header} {{\n",
                        if strict { "strict " } else { "" }
                    ));
                    out.push_str(&self.members_block(id, shape, false, elide.as_ref(), &[]));
                    out.push_str("}\n");
                }
            }
            other => {
                self.diag(
                    Severity::Error,
                    id,
                    format!("shape type {other} has no .cru declaration"),
                );
                return None;
            }
        }
        Some(out)
    }

    fn member_traits_unsupported(&mut self, id: &str, mem: &Value) {
        if let Some(t) = model::traits(mem) {
            let keys: Vec<String> = t.keys().cloned().collect();
            for k in keys {
                if k == "smithy.api#documentation" {
                    self.diag(Severity::Warning, id, "documentation on a collection member cannot be written in .cru and was dropped");
                } else {
                    self.diag(
                        Severity::Error,
                        id,
                        format!("{k} on a collection member cannot be written in .cru"),
                    );
                }
            }
        }
    }
}

/// Traits that change nothing a client or server does are warnings when
/// dropped; the rest change the contract, so dropping them is an error.
fn dropped_severity(key: &str) -> Severity {
    match key {
        "smithy.api#references" | "smithy.api#tags" | "smithy.api#suppress" => Severity::Warning,
        _ => Severity::Error,
    }
}

fn member_modifiers(mem: &Value) -> String {
    let mut out = String::new();
    if has_trait(mem, "smithy.api#sensitive") {
        out.push_str("sensitive ");
    }
    if has_trait(mem, "smithy.api#internal") {
        out.push_str("internal ");
    }
    out
}

fn query(mem: &Value, member: &str) -> String {
    let name = get_trait(mem, "smithy.api#httpQuery")
        .and_then(Value::as_str)
        .unwrap_or(member);
    if name == member {
        "@query".into()
    } else {
        format!("@query({})", quote(name))
    }
}

fn header(mem: &Value) -> String {
    let name = get_trait(mem, "smithy.api#httpHeader")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("@header({})", quote(name))
}

fn range(v: &Value) -> String {
    let min = v.get("min").map(|n| n.to_string()).unwrap_or_default();
    let max = v.get("max").map(|n| n.to_string()).unwrap_or_default();
    format!("{min}..{max}")
}

fn constraint_text(shape: &Value) -> String {
    Cx::constraints(model::traits(shape))
}

/// `^(a|b|c)$` as its words, when every one is an identifier.
fn alternation(pattern: &str) -> Option<Vec<String>> {
    let inner = pattern.strip_prefix("^(")?.strip_suffix(")$")?;
    let words: Vec<String> = inner.split('|').map(str::to_owned).collect();
    words.iter().all(|w| is_ident(w)).then_some(words)
}

mod emit;
