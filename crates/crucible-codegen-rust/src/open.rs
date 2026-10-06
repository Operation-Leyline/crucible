//! Thin declarations for client enums and unions; serde behavior is shared.
use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Enum, Service, Structure};

use crate::names::{doc, literal, variant};
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
        "impl ::crucible::__private::serde::Serialize for {name} {{\n    fn serialize<S: ::crucible::__private::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {{\n        ::crucible::open::serialize_{kind}(self, serializer)\n    }}\n}}\nimpl<'de> ::crucible::__private::serde::Deserialize<'de> for {name} {{\n    fn deserialize<D: ::crucible::__private::serde::Deserializer<'de>>(deserializer: D) -> ::std::result::Result<Self, D::Error> {{\n        ::crucible::open::deserialize_{kind}(deserializer)\n    }}\n}}\n"
    ));
}

pub(crate) fn enumeration(out: &mut String, name: &str, values: &Enum) {
    let unknown = unknown_variant(values.values.iter().map(|value| value.name.as_str()));
    doc(out, "", values.docs.as_deref());
    out.push_str(&format!(
        "#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum {name} {{\n"
    ));
    for value in &values.values {
        doc(out, "    ", value.docs.as_deref());
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
    out.push_str(&format!("            Self::{unknown}(value) => value,\n        }}\n    }}\n}}\nimpl ::crucible::open::Enum for {name} {{\n    fn as_str(&self) -> &str {{ self.as_str() }}\n    fn from_string(value: ::std::string::String) -> Self {{\n        match value.as_str() {{\n"));
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
    doc(out, "", union.docs.as_deref());
    out.push_str(&format!(
        "#[derive(Debug, Clone, PartialEq)]\npub enum {name} {{\n"
    ));
    for member in &union.members {
        doc(out, "    ", member.docs.as_deref());
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
        out.push_str(&format!("    /// An arm introduced by a newer service.\n    {unknown}(::crucible::UnknownUnion),\n"));
    }
    out.push_str(&format!("}}\nimpl ::crucible::open::Union for {name} {{\n{ignore}    fn deserialize_member<'de, D: ::crucible::__private::serde::Deserializer<'de>>(name: ::std::string::String, value: D) -> ::std::result::Result<Self, D::Error> {{\n        match name.as_str() {{\n"));
    for member in &union.members {
        let decode = if let Some(mut codec) = crate::types::codec(service, member) {
            if boxed.contains(&id.with_member(&member.name)) {
                codec = format!("::crucible::codec::Boxed<{codec}>");
            }
            format!("::crucible::codec::Adapter::<{codec}>::deserialize")
        } else {
            "::crucible::__private::serde::Deserialize::deserialize".to_owned()
        };
        out.push_str(&format!(
            "            {} => {decode}(value).map(Self::{}),\n",
            literal(&member.wire_name),
            variant(&member.name)
        ));
    }
    if client {
        out.push_str(&format!("            _ => ::crucible::__private::serde::Deserialize::deserialize(value).map(|value| Self::{unknown}(::crucible::UnknownUnion {{ name, value }})),\n"));
    } else {
        out.push_str("            _ => Err(::crucible::__private::serde::de::Error::custom(\"unknown union member\")),\n");
    }
    out.push_str("        }\n    }\n    fn serialize_member<S: ::crucible::__private::serde::ser::SerializeMap>(&self, map: &mut S) -> ::std::result::Result<(), S::Error> {\n        match self {\n");
    for member in &union.members {
        let value = if let Some(mut codec) = crate::types::codec(service, member) {
            if boxed.contains(&id.with_member(&member.name)) {
                codec = format!("::crucible::codec::Boxed<{codec}>");
            }
            format!("&::crucible::codec::Ref::<_, {codec}>(value, ::std::marker::PhantomData)")
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
        out.push_str(&format!("            Self::{unknown}(value) => ::crucible::open::serialize_unknown(value, &[{known}], map),\n"));
    }
    out.push_str("        }\n    }\n}\n");
    serde_impls(out, name, "union");
}

pub(crate) fn int_enumeration(
    out: &mut String,
    name: &str,
    values: &crucible_smithy::ir::IntEnum,
    client: bool,
) {
    let unknown = unknown_variant(values.values.iter().map(|v| v.name.as_str()));
    doc(out, "", values.docs.as_deref());
    out.push_str(&format!(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum {name} {{\n"
    ));
    for value in &values.values {
        doc(out, "    ", value.docs.as_deref());
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
    out.push_str(&format!("        }}\n    }}\n}}\nimpl ::crucible::open::IntEnum for {name} {{\n    fn as_i32(&self) -> i32 {{ self.as_i32() }}\n    fn from_i32(value: i32) -> Option<Self> {{\n        match value {{\n"));
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
