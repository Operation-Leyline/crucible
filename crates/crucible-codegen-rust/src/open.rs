//! Thin declarations for client enums and unions; serde behavior is shared.
use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Enum, Service, Structure};

use crate::names::{documented, literal, variant};
use crate::types::rust_type;

pub(crate) fn unknown_variant<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let used: BTreeSet<String> = names.map(variant).collect();
    let mut name = "UnknownValue".to_owned();
    let mut suffix = 2;
    while used.contains(&name) {
        name = format!("UnknownValue{suffix}");
        suffix += 1;
    }
    name
}

fn serde_impls(out: &mut String, name: &str, kind: &str) {
    out.push_str(&format!(
        "impl ::crucible_server_runtime_rust::__generated::serde::Serialize for {name} {{\n    fn serialize<S: ::crucible_server_runtime_rust::__generated::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {{\n        ::crucible_server_runtime_rust::__generated::open::serialize_{kind}(self, serializer)\n    }}\n}}\nimpl<'de> ::crucible_server_runtime_rust::__generated::serde::Deserialize<'de> for {name} {{\n    fn deserialize<D: ::crucible_server_runtime_rust::__generated::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {{\n        ::crucible_server_runtime_rust::__generated::open::deserialize_{kind}(deserializer)\n    }}\n}}\n"
    ));
}

pub(crate) fn enumeration(out: &mut String, name: &str, values: &Enum, debug: bool) {
    let unknown = unknown_variant(values.values.iter().map(|value| value.name.as_str()));
    documented(out, "", values.docs.as_deref(), &values.notes);
    let debug = if debug { "Debug, " } else { "" };
    out.push_str(&format!(
        "#[derive({debug}Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\n{}pub enum {name} {{\n",
        crate::NON_EXHAUSTIVE
    ));
    for value in &values.values {
        documented(out, "    ", value.docs.as_deref(), &value.notes);
        out.push_str(&format!("    {},\n", variant(&value.name)));
    }
    out.push_str(&format!("    /// A value introduced by a newer service.\n    {unknown}(::std::string::String),\n}}\nimpl {name} {{\n    pub fn as_str(&self) -> &str {{\n        match self {{\n"));
    for value in &values.values {
        out.push_str(&format!(
            "            Self::{} => {},\n",
            variant(&value.name),
            literal(&value.value)
        ));
    }
    out.push_str(&format!("            Self::{unknown}(value) => value,\n        }}\n    }}\n}}\nimpl ::crucible_server_runtime_rust::__generated::open::Enum for {name} {{\n    fn as_str(&self) -> &str {{ self.as_str() }}\n    fn from_string(value: ::std::string::String) -> Self {{\n        match value.as_str() {{\n"));
    for value in &values.values {
        out.push_str(&format!(
            "            {} => Self::{},\n",
            literal(&value.value),
            variant(&value.name)
        ));
    }
    out.push_str(&format!(
        "            _ => Self::{unknown}(value),\n        }}\n    }}\n}}\n"
    ));
    serde_impls(out, name, "enum");
}

pub(crate) fn union(
    out: &mut String,
    service: &Service,
    id: &ShapeId,
    name: &str,
    union: &Structure,
    boxed: &BTreeSet<ShapeId>,
    client: bool,
) {
    let unknown = unknown_variant(union.members.iter().map(|member| member.name.as_str()));
    documented(out, "", union.docs.as_deref(), &union.notes);
    out.push_str(&format!(
        "{}\n{}pub enum {name} {{\n",
        if client {
            "#[derive(Clone, PartialEq)]"
        } else {
            "#[derive(Debug, Clone, PartialEq)]"
        },
        if client { crate::NON_EXHAUSTIVE } else { "" }
    ));
    for member in &union.members {
        documented(out, "    ", member.docs.as_deref(), &member.notes);
        let mut ty = rust_type(service, &member.target, "");
        // An event stream's modeled errors live with the other errors.
        if matches!(service.shape(&member.target), Some(crucible_smithy::ir::Shape::Structure(s)) if s.error.is_some())
        {
            ty = format!("super::errors::{ty}");
        }
        if boxed.contains(&id.with_member(&member.name)) {
            ty = format!("::std::boxed::Box<{ty}>");
        }
        out.push_str(&format!("    {}({ty}),\n", variant(&member.name)));
    }
    // AWS JSON protocols may add a `__type` key beside the member.
    let ignore = if service.protocol_settings.ignore_unknown_union_type
        && !union
            .members
            .iter()
            .any(|member| member.wire_name == "__type")
    {
        "    const IGNORE_TYPE: bool = true;\n"
    } else {
        ""
    };
    if client {
        out.push_str(&format!("    /// An arm introduced by a newer service.\n    {unknown}(::crucible_server_runtime_rust::__generated::UnknownUnion),\n"));
    }
    out.push_str(&format!("}}\nimpl ::crucible_server_runtime_rust::__generated::open::Union for {name} {{\n{ignore}    fn deserialize_member<'de, D: ::crucible_server_runtime_rust::__generated::serde::Deserializer<'de>>(name: ::std::string::String, value: D) -> ::std::result::Result<Self, D::Error> {{\n        match name.as_str() {{\n"));
    for member in &union.members {
        let decode = if let Some(mut codec) = crate::types::codec(service, member) {
            if boxed.contains(&id.with_member(&member.name)) {
                codec =
                    format!("::crucible_server_runtime_rust::__generated::codec::Boxed<{codec}>");
            }
            format!(
                "::crucible_server_runtime_rust::__generated::codec::Adapter::<{codec}>::deserialize"
            )
        } else {
            "::crucible_server_runtime_rust::__generated::serde::Deserialize::deserialize"
                .to_owned()
        };
        out.push_str(&format!(
            "            {} => {decode}(value).map(Self::{}),\n",
            literal(&member.wire_name),
            variant(&member.name)
        ));
    }
    if client {
        out.push_str(&format!("            _ => ::crucible_server_runtime_rust::__generated::serde::Deserialize::deserialize(value).map(|value| Self::{unknown}(::crucible_server_runtime_rust::__generated::UnknownUnion {{ name, value }})),\n"));
    } else {
        out.push_str("            _ => Err(::crucible_server_runtime_rust::__generated::serde::de::Error::custom(\"unknown union member\")),\n");
    }
    out.push_str("        }\n    }\n    fn serialize_member<S: ::crucible_server_runtime_rust::__generated::serde::ser::SerializeMap>(&self, map: &mut S) -> ::std::result::Result<(), S::Error> {\n        match self {\n");
    for member in &union.members {
        let value = if let Some(mut codec) = crate::types::codec(service, member) {
            if boxed.contains(&id.with_member(&member.name)) {
                codec =
                    format!("::crucible_server_runtime_rust::__generated::codec::Boxed<{codec}>");
            }
            format!(
                "&::crucible_server_runtime_rust::__generated::codec::Ref::<_, {codec}>(value, ::std::marker::PhantomData)"
            )
        } else {
            "value".to_owned()
        };
        out.push_str(&format!(
            "            Self::{}(value) => map.serialize_entry({}, {value}),\n",
            variant(&member.name),
            literal(&member.wire_name)
        ));
    }
    let known = union
        .members
        .iter()
        .map(|member| literal(&member.wire_name))
        .collect::<Vec<_>>()
        .join(", ");
    if client {
        out.push_str(&format!("            Self::{unknown}(value) => ::crucible_server_runtime_rust::__generated::open::serialize_unknown(value, &[{known}], map),\n"));
    }
    out.push_str("        }\n    }\n}\n");
    serde_impls(out, name, "union");
    if client {
        crate::debug::union(out, service, name, union, &unknown);
    }
}

pub(crate) fn int_enumeration(
    out: &mut String,
    name: &str,
    values: &crucible_smithy::ir::IntEnum,
    client: bool,
    debug: bool,
) {
    let unknown = unknown_variant(values.values.iter().map(|v| v.name.as_str()));
    documented(out, "", values.docs.as_deref(), &values.notes);
    let debug = if debug { "Debug, " } else { "" };
    out.push_str(&format!(
        "#[derive({debug}Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n{}pub enum {name} {{\n",
        if client { crate::NON_EXHAUSTIVE } else { "" }
    ));
    for value in &values.values {
        documented(out, "    ", value.docs.as_deref(), &value.notes);
        out.push_str(&format!("    {},\n", variant(&value.name)));
    }
    if client {
        out.push_str(&format!("    {unknown}(i32),\n"));
    }
    out.push_str(&format!(
        "}}\nimpl {name} {{\n    pub fn as_i32(&self) -> i32 {{\n        match self {{\n"
    ));
    for value in &values.values {
        out.push_str(&format!(
            "            Self::{} => {},\n",
            variant(&value.name),
            value.value
        ));
    }
    if client {
        out.push_str(&format!("            Self::{unknown}(value) => *value,\n"));
    }
    out.push_str(&format!("        }}\n    }}\n}}\nimpl ::crucible_server_runtime_rust::__generated::open::IntEnum for {name} {{\n    fn as_i32(&self) -> i32 {{ self.as_i32() }}\n    fn from_i32(value: i32) -> ::std::option::Option<Self> {{\n        match value {{\n"));
    for value in &values.values {
        out.push_str(&format!(
            "            {} => Some(Self::{}),\n",
            value.value,
            variant(&value.name)
        ));
    }
    out.push_str(&format!(
        "            _ => {},\n        }}\n    }}\n}}\n",
        if client {
            format!("Some(Self::{unknown}(value))")
        } else {
            "None".to_owned()
        }
    ));
    serde_impls(out, name, "int_enum");
}
