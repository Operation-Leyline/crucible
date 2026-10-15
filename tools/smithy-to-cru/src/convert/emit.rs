//! Files: the manifest, the service, resources, operations and shapes.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{Attach, Cx, Elide, Role, Severity};
use crate::model::{self, doc_of, get_trait, has_trait, kind, members, refs, short, target};
use crate::text::{self, is_ident, quote};

fn snake(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_uppercase()
            && i > 0
            && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit())
        {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

impl Cx<'_> {
    pub(super) fn emit(
        &mut self,
        service_id: &str,
        service: &Value,
        ops: &[String],
    ) -> BTreeMap<String, String> {
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        let push = |files: &mut BTreeMap<String, String>, path: &str, text: &str| {
            let entry = files.entry(path.to_owned()).or_default();
            if !entry.is_empty() {
                entry.push('\n');
            }
            entry.push_str(text);
        };

        // Shapes the package declares: everything the operations, errors and
        // resources reach, apart from the operations' own input/output shapes.
        let mut roots: Vec<String> = refs(service, "errors");
        for op in ops {
            let s = self.shape(op).expect("op");
            roots.extend(refs(s, "errors"));
            let (i, o) = self.io_of(op);
            for io in [i, o].into_iter().flatten() {
                if let Some(s) = self.shape(&io) {
                    roots.extend(members(s).into_iter().map(|(_, m)| target(m).to_owned()));
                }
            }
        }
        for r in self.resources.values() {
            roots.extend(r.ids.iter().map(|(_, t)| t.clone()));
            roots.extend(r.props.iter().map(|(_, t)| t.clone()));
        }
        let declared = self.closure(roots);

        // Which file each shape lives in.
        let group_of = |cx: &Self, op: &str| -> String {
            cx.attach
                .get(op)
                .map(|a: &Attach| short(&a.resource).to_owned())
                .unwrap_or_else(|| "service".into())
        };
        let mut io_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut err_reach: BTreeSet<String> = BTreeSet::new();
        for op in ops {
            let g = group_of(self, op);
            let s = self.shape(op).expect("op");
            let mut io_roots = Vec::new();
            let (i, o) = self.io_of(op);
            for io in [i, o].into_iter().flatten() {
                if let Some(s) = self.shape(&io) {
                    io_roots.extend(members(s).into_iter().map(|(_, m)| target(m).to_owned()));
                }
            }
            for t in self.closure(io_roots) {
                io_groups.entry(t).or_default().insert(g.clone());
            }
            err_reach.extend(self.closure(refs(s, "errors")));
        }
        err_reach.extend(self.closure(refs(service, "errors")));
        for (id, r) in self.resources.iter() {
            let g = short(id).to_owned();
            let mut roots: Vec<String> = r.ids.iter().map(|(_, t)| t.clone()).collect();
            roots.extend(r.props.iter().map(|(_, t)| t.clone()));
            for t in self.closure(roots) {
                io_groups.entry(t).or_default().insert(g.clone());
            }
        }
        let mut shape_ids: Vec<&String> = declared.iter().collect();
        shape_ids.sort();
        for id in shape_ids {
            if model::namespace(id) != self.ns {
                continue;
            }
            let Some(shape) = self.shape(id) else {
                continue;
            };
            if matches!(kind(shape), "operation" | "resource" | "service")
                || self.meta.contains_key(id)
            {
                continue;
            }
            let Some(text) = self.declaration(id, shape) else {
                continue;
            };
            let path = match io_groups.get(id) {
                Some(groups) if groups.len() == 1 => {
                    let g = groups.iter().next().expect("one");
                    if g == "service" {
                        "shapes/common.cru".to_owned()
                    } else {
                        format!("resources/{}.cru", snake(g))
                    }
                }
                Some(_) => "shapes/common.cru".to_owned(),
                None if err_reach.contains(id) => "shapes/errors.cru".to_owned(),
                None => "shapes/common.cru".to_owned(),
            };
            push(&mut files, &path, &text);
        }

        // Resources, in file order of their group.
        let resource_ids: Vec<String> = self.resources.keys().cloned().collect();
        for id in &resource_ids {
            let text = self.resource_text(id);
            push(
                &mut files,
                &format!("resources/{}.cru", snake(short(id))),
                &text,
            );
        }

        // Operations.
        for op in ops {
            let text = self.operation_text(op);
            let dir = match self.attach.get(op) {
                Some(a) => snake(short(&a.resource)),
                None => "service".to_owned(),
            };
            push(
                &mut files,
                &format!("operations/{dir}/{}.cru", short(op)),
                &text,
            );
        }

        // Metadata declarations.
        let mut meta = String::new();
        for def in self.meta.values() {
            meta.push_str(&def.text);
            meta.push('\n');
        }
        if !meta.is_empty() {
            push(&mut files, "shapes/metadata.cru", &meta);
        }

        let service_text = self.service_text(service_id, service, ops);
        push(
            &mut files,
            &format!("{}.cru", snake(short(service_id))),
            &service_text,
        );

        // Unreached shapes: say so rather than drop them quietly.
        let unreached: Vec<String> = self
            .m
            .shapes
            .iter()
            .filter(|(id, s)| {
                model::namespace(id) == self.ns
                    && !declared.contains(*id)
                    && !matches!(kind(s), "operation" | "resource" | "service" | "apply")
                    && !self.meta.contains_key(*id)
                    && !self.io_shapes.contains(*id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in unreached {
            self.diag(
                Severity::Note,
                &id,
                "not reachable from the service; not converted",
            );
        }

        let mut imports: Vec<String> = self.imports.iter().cloned().collect();
        imports.sort();
        let imports = imports
            .iter()
            .map(|i| quote(i))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest = format!(
            "schema_version = 1\n\n[model]\nnamespace = {}\nlanguage = 1\nsources = [\".\"]\nimports = [{imports}]\n\n[model.naming]\ninput = {}\noutput = {}\n",
            quote(&self.ns),
            quote(&self.naming.0),
            quote(&self.naming.1),
        );
        files.insert("crucible.toml".into(), manifest);
        files
    }

    /// The shapes reachable from `roots` through members, roots included.
    fn closure(&self, roots: Vec<String>) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = roots;
        while let Some(id) = stack.pop() {
            if id.starts_with("smithy.api#") || !seen.insert(id.clone()) {
                continue;
            }
            let Some(s) = self.shape(&id) else { continue };
            match kind(s) {
                "structure" | "union" => {
                    stack.extend(members(s).into_iter().map(|(_, m)| target(m).to_owned()));
                }
                "list" | "set" => stack.extend(s.get("member").map(|m| target(m).to_owned())),
                "map" => {
                    stack.extend(s.get("key").map(|m| target(m).to_owned()));
                    stack.extend(s.get("value").map(|m| target(m).to_owned()));
                }
                _ => {}
            }
        }
        seen
    }

    fn service_text(&mut self, id: &str, service: &Value, ops: &[String]) -> String {
        let traits = model::traits(service);
        let mut out = String::new();
        if let Some(d) = doc_of(traits) {
            text::doc(&mut out, d);
        }
        for a in self.annotations(id, traits) {
            out.push_str(&a);
            out.push('\n');
        }
        out.push_str(&format!("service {} {{\n", short(id)));
        let version = service.get("version").and_then(Value::as_str).unwrap_or("");
        out.push_str(&format!("version {}\n", quote(version)));
        if let Some(t) = get_trait(service, "smithy.api#title").and_then(Value::as_str) {
            out.push_str(&format!("title {}\n", quote(t)));
        }
        out.push_str("protocol json\n");
        if has_trait(service, "smithy.api#httpBearerAuth") {
            out.push_str("auth bearer\n");
        }
        if let Some(p) = get_trait(service, "smithy.api#paginated") {
            out.push_str(&pagination(p));
            out.push('\n');
        }
        let unattached: Vec<&String> = ops
            .iter()
            .filter(|o| !self.attach.contains_key(*o))
            .collect();
        if !unattached.is_empty() {
            out.push_str(&format!(
                "operations [{}]\n",
                unattached
                    .iter()
                    .map(|o| short(o))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let roots: Vec<String> = refs(service, "resources")
            .iter()
            .filter(|r| self.resources.get(*r).is_some_and(|r| r.parent.is_none()))
            .map(|r| short(r).to_owned())
            .collect();
        if !roots.is_empty() {
            out.push_str(&format!("resources [{}]\n", roots.join(", ")));
        }
        let errors = refs(service, "errors");
        if !errors.is_empty() {
            let names: Vec<String> = errors.iter().map(|e| self.ty(id, e)).collect();
            out.push_str(&format!("errors [{}]\n", names.join(", ")));
        }
        out.push_str("}\n");
        out
    }

    fn resource_text(&mut self, id: &str) -> String {
        let shape = self.shape(id).expect("resource");
        let traits = model::traits(shape);
        let mut out = String::new();
        if let Some(d) = doc_of(traits) {
            text::doc(&mut out, d);
        }
        for a in self.annotations(id, traits) {
            out.push_str(&a);
            out.push('\n');
        }
        let (parent, own_ids, props, alias) = {
            let r = &self.resources[id];
            (
                r.parent.clone(),
                r.own_ids.clone(),
                r.props.clone(),
                r.alias.clone(),
            )
        };
        out.push_str(&format!("resource {}", short(id)));
        if let Some(p) = &parent {
            out.push_str(&format!(" in {}", short(p)));
        }
        out.push_str(" {\n");
        for (name, t) in &own_ids {
            let ty = self.ty(id, t);
            out.push_str(&format!("id {name}: {ty}"));
            if let Some(a) = alias.get(name) {
                out.push_str(&format!(" as {a}"));
            }
            out.push('\n');
        }
        for (name, t) in &props {
            let ty = self.ty(id, t);
            let r = &self.resources[id];
            let kw = if r.written_update.contains(name) {
                ""
            } else if r.written_create.contains(name) {
                "immutable "
            } else {
                "readonly "
            };
            let optional = r.site_seen.contains(name) && !r.site_required.contains(name);
            out.push_str(&format!(
                "{kw}property {name}{}: {ty}\n",
                if optional { "?" } else { "" }
            ));
        }
        out.push_str("}\n");
        out
    }

    fn operation_text(&mut self, id: &str) -> String {
        let shape = self.shape(id).expect("op");
        let traits = model::traits(shape);
        let name = short(id);
        let mut out = String::new();
        if let Some(d) = doc_of(traits) {
            text::doc(&mut out, d);
        }
        for a in self.annotations(id, traits) {
            out.push_str(&a);
            out.push('\n');
        }
        let attach = self
            .attach
            .get(id)
            .map(|a| (a.resource.clone(), a.role, a.collection));
        let (input_id, output_id) = self.io_of(id);
        let token = input_id
            .as_ref()
            .and_then(|i| self.shape(i))
            .is_some_and(|s| {
                members(s)
                    .iter()
                    .any(|(_, m)| has_trait(m, "smithy.api#idempotencyToken"))
            });
        let role = attach.as_ref().map(|a| a.1);
        let mut mods = String::new();
        if has_trait(shape, "smithy.api#readonly")
            && !matches!(role, Some(Role::Reads | Role::Lists))
        {
            mods.push_str("readonly ");
        }
        let method = get_trait(shape, "smithy.api#http")
            .and_then(|h| h.get("method"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let implied = matches!(role, Some(Role::Puts | Role::Deletes)) || token;
        let readonly_here = has_trait(shape, "smithy.api#readonly");
        if has_trait(shape, "smithy.api#idempotent") && !implied {
            mods.push_str("idempotent ");
        } else if matches!(method, "PUT" | "DELETE") && !readonly_here && !implied {
            // .cru requires it of PUT and DELETE (decision 26); Smithy does not.
            mods.push_str("idempotent ");
            self.diag(
                Severity::Note,
                id,
                format!("{method} operation declared `idempotent`: .cru requires it, Smithy did not say so"),
            );
        }
        let own_auth = get_trait(shape, "smithy.api#auth");
        match own_auth {
            Some(a) if a.as_array().is_some_and(|a| a.is_empty()) => mods.push_str("public "),
            Some(_) => self.diag(Severity::Error, id, "@auth with schemes is not supported"),
            None if self.service_public => mods.push_str("public "),
            None => {}
        }
        if has_trait(shape, "smithy.api#internal") {
            mods.push_str("internal ");
        }
        out.push_str(&format!("{mods}operation {name}"));
        if let Some((res, role, collection)) = &attach {
            out.push_str(&format!(" {} {}", role.word(), short(res)));
            if *collection && *role == Role::On {
                out.push_str(" collection");
            }
        }
        out.push_str(" {\n");

        match get_trait(shape, "smithy.api#http") {
            Some(h) => {
                let method = h.get("method").and_then(Value::as_str).unwrap_or("GET");
                let uri = h.get("uri").and_then(Value::as_str).unwrap_or("/");
                out.push_str(&format!("http {method} {}", quote(uri)));
                if let Some(code) = h.get("code").and_then(Value::as_u64) {
                    out.push_str(&format!(" -> {code}"));
                }
                out.push('\n');
            }
            None => self.diag(Severity::Error, id, "operation has no @http"),
        }
        if let Some(p) = get_trait(shape, "smithy.api#paginated") {
            out.push_str(&pagination(p));
            out.push('\n');
        }

        let elide = attach.as_ref().map(|(r, _, _)| r.clone());
        for (is_input, io) in [(true, input_id), (false, output_id)] {
            let Some(io) = io else { continue };
            let Some(s) = self.shape(&io) else {
                self.diag(Severity::Error, id, format!("{io} is not in the model"));
                continue;
            };
            let io_traits = model::traits(s);
            if let Some(d) = doc_of(io_traits) {
                text::doc(&mut out, d);
            }
            if let Some(t) = io_traits {
                for k in t.keys() {
                    if let Some(n) = k.strip_prefix("smithy.api#") {
                        if !super::HANDLED.contains(&n) {
                            self.diag(
                                super::dropped_severity(k),
                                &io,
                                format!("trait {k} has no .cru equivalent and was dropped"),
                            );
                        }
                    } else if self.meta.contains_key(k) {
                        self.diag(Severity::Warning, &io, format!("custom trait {k} on an input/output shape cannot be written in .cru and was dropped"));
                    }
                }
            }
            if has_trait(s, "crucible.protocols#strict") {
                out.push_str("strict ");
            }
            out.push_str(if is_input { "input {\n" } else { "output {\n" });
            let collection = attach.as_ref().is_some_and(|(_, role, coll)| {
                matches!(role, Role::Creates | Role::Lists) || (*role == Role::On && *coll)
            });
            let e = elide.as_ref().map(|r| Elide {
                resource: r.clone(),
                input: is_input,
                collection,
            });
            out.push_str(&self.members_block(&io, s, false, e.as_ref(), &[]));
            out.push_str("}\n");
        }

        let errors = refs(shape, "errors");
        if !errors.is_empty() {
            let names: Vec<String> = errors.iter().map(|e| self.ty(id, e)).collect();
            out.push_str(&format!("errors [{}]\n", names.join(", ")));
        }

        if let Some(examples) = get_trait(shape, "smithy.api#examples").and_then(Value::as_array) {
            for ex in examples {
                let title = ex.get("title").and_then(Value::as_str).unwrap_or("");
                if let Some(d) = ex.get("documentation").and_then(Value::as_str) {
                    text::doc(&mut out, d);
                }
                if ex
                    .get("allowConstraintErrors")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    self.diag(
                        Severity::Error,
                        id,
                        format!(
                            "example {title:?} allows constraint errors, which .cru cannot say"
                        ),
                    );
                }
                out.push_str(&format!("example {} {{\n", quote(title)));
                for part in ["input", "output"] {
                    if let Some(v) = ex.get(part) {
                        match text::value(v) {
                            Some(t) => out.push_str(&format!("{part} {t}\n")),
                            None => self.diag(
                                Severity::Error,
                                id,
                                format!("example {title:?} has a null no .cru literal can hold"),
                            ),
                        }
                    }
                }
                if let Some(err) = ex.get("error") {
                    let shape_id = err.get("shapeId").and_then(Value::as_str).unwrap_or("");
                    let content = err
                        .get("content")
                        .and_then(text::value)
                        .unwrap_or_else(|| "{}".into());
                    out.push_str(&format!("error {} {content}\n", short(shape_id)));
                }
                out.push_str("}\n");
            }
        }
        out.push_str("}\n");
        out
    }
}

fn pagination(p: &Value) -> String {
    let mut args = Vec::new();
    for key in ["inputToken", "outputToken", "items", "pageSize"] {
        if let Some(v) = p.get(key).and_then(Value::as_str) {
            let v = if is_ident(v) { v.to_owned() } else { quote(v) };
            args.push(format!("{key}: {v}"));
        }
    }
    format!("paginated({})", args.join(", "))
}
