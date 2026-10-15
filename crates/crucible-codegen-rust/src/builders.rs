//! Client construction and accessors; field storage remains the wire contract.
use std::collections::{BTreeMap, BTreeSet};

use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape, Structure};

use crate::names::{doc, documented, field, literal};
use crate::types::rust_type;

pub(crate) fn names(service: &Service) -> BTreeMap<ShapeId, String> {
    let mut used: BTreeSet<String> = service
        .shapes
        .keys()
        .map(|id| service.name_of(id).to_owned())
        .collect();
    service
        .shapes
        .iter()
        .filter_map(|(id, shape)| {
            if !matches!(shape, Shape::Structure(_)) {
                return None;
            }
            let base = format!("{}Builder", service.name_of(id));
            let mut name = base.clone();
            let mut suffix = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}{suffix}");
                suffix += 1;
            }
            Some((id.clone(), name))
        })
        .collect()
}

fn method_names(structure: &Structure, reserved: &[&str]) -> BTreeMap<String, String> {
    let mut used: BTreeSet<String> = structure.members.iter().map(|m| field(&m.name)).collect();
    used.extend(reserved.iter().map(|name| (*name).to_owned()));
    structure
        .members
        .iter()
        .map(|member| {
            let native = field(&member.name);
            let mut method = native.clone();
            if reserved.contains(&native.as_str()) {
                let base = format!("member_{}", native.trim_start_matches("r#"));
                method = base.clone();
                let mut suffix = 2;
                while !used.insert(method.clone()) {
                    method = format!("{base}{suffix}");
                    suffix += 1;
                }
            }
            (member.name.clone(), method)
        })
        .collect()
}

pub(crate) fn emit(
    out: &mut String,
    service: &Service,
    id: &ShapeId,
    structure: &Structure,
    boxed: &BTreeSet<ShapeId>,
    prefix: &str,
    builder: &str,
) {
    let name = service.name_of(id);
    let getters = method_names(
        structure,
        &[
            "builder",
            "clone",
            "clone_from",
            "validate",
            "info",
            "to_json",
            "write_json",
            "fmt",
            "source",
            "description",
            "cause",
            "type_id",
        ],
    );
    let setters = method_names(structure, &["build", "default", "clone"]);
    out.push_str(&format!(
        "impl {name} {{\n    /// Starts a builder with all optional members absent.\n    pub fn builder() -> {builder} {{ {builder}::default() }}\n"
    ));
    for member in &structure.members {
        let native = field(&member.name);
        let getter = &getters[&member.name];
        let mut ty = rust_type(service, &member.target, prefix);
        if boxed.contains(&id.with_member(&member.name)) {
            ty = format!("::std::boxed::Box<{ty}>");
        }
        documented(out, "    ", member.docs.as_deref(), &member.notes);
        if member.optional_on_client() {
            out.push_str(&format!("    /// The value when present; inspect the public field to distinguish absent and null.\n    pub fn {getter}(&self) -> ::std::option::Option<&{ty}> {{ self.{native}.value() }}\n"));
        } else {
            out.push_str(&format!(
                "    pub fn {getter}(&self) -> &{ty} {{ &self.{native} }}\n"
            ));
        }
    }
    out.push_str("}\n");
    doc(
        out,
        "",
        Some(&format!(
            "Builds {name}; build checks required members and applies modeled defaults."
        )),
    );
    out.push_str(&format!(
        "#[derive(Clone, Default)]\npub struct {builder} {{\n"
    ));
    for member in &structure.members {
        let mut ty = rust_type(service, &member.target, prefix);
        if boxed.contains(&id.with_member(&member.name)) {
            ty = format!("::std::boxed::Box<{ty}>");
        }
        let wrapper = if member.optional_on_client() {
            "::crucible_server_runtime_rust::__generated::Field"
        } else {
            "::std::option::Option"
        };
        out.push_str(&format!("    {}: {wrapper}<{ty}>,\n", field(&member.name)));
    }
    out.push_str("}\n");
    crate::debug::structure(out, service, builder, structure);
    out.push_str(&format!("impl {builder} {{\n"));
    for member in &structure.members {
        let native = field(&member.name);
        let setter = &setters[&member.name];
        let mut ty = rust_type(service, &member.target, prefix);
        if boxed.contains(&id.with_member(&member.name)) {
            ty = format!("::std::boxed::Box<{ty}>");
        }
        let constructor = if member.optional_on_client() {
            "::crucible_server_runtime_rust::__generated::Field::Value"
        } else {
            "Some"
        };
        documented(
            out,
            "    ",
            member
                .docs
                .as_deref()
                .or(Some("Sets this member to a value.")),
            &member.notes,
        );
        out.push_str(&format!("    pub fn {setter}(mut self, value: impl ::std::convert::Into<{ty}>) -> Self {{ self.{native} = {constructor}(value.into()); self }}\n"));
    }
    let fallible = structure
        .members
        .iter()
        .any(|m| !m.optional_on_client() && m.default.is_none());
    let result = if fallible {
        format!(
            "::std::result::Result<{name}, ::crucible_server_runtime_rust::__generated::BuildError>"
        )
    } else {
        name.to_owned()
    };
    doc(
        out,
        "    ",
        Some(if fallible {
            "Builds the model; fails when a required member without a default is unset."
        } else {
            "Builds the model and applies modeled defaults."
        }),
    );
    out.push_str(&format!(
        "    pub fn build(self) -> {result} {{\n        {}{name} {{\n",
        if fallible { "Ok(" } else { "" }
    ));
    for member in &structure.members {
        let native = field(&member.name);
        let value = if member.optional_on_client() {
            format!("self.{native}")
        } else if member.default.is_some() {
            let function = format!(
                "{}__{}",
                crate::names::function(name),
                crate::names::function(&member.name).trim_start_matches("r#")
            );
            format!("self.{native}.unwrap_or_else(defaults::{function})")
        } else {
            format!(
                "self.{native}.ok_or(::crucible_server_runtime_rust::__generated::build_error({}, {}))?",
                literal(name),
                literal(&member.name)
            )
        };
        out.push_str(&format!("            {native}: {value},\n"));
    }
    out.push_str(&format!(
        "        }}{}\n    }}\n}}\n",
        if fallible { ")" } else { "" }
    ));
}
