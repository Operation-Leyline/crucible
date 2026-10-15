//! Thin Debug implementations; native Debug recursively formats nested models.
use crate::names::{field, literal, variant};
use crucible_ir::ir::{Service, Structure};

pub(crate) fn structure(out: &mut String, service: &Service, name: &str, shape: &Structure) {
    out.push_str(&format!("impl ::std::fmt::Debug for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        let mut debug = f.debug_struct({});\n", literal(name)));
    for member in &shape.members {
        let value = if shape.sensitive()
            || service.sensitive(member)
            || crucible_codegen_common::sensitive_collection(service, &member.target)
        {
            "&\"*** Sensitive Data Redacted ***\"".to_owned()
        } else {
            format!("&self.{}", field(&member.name))
        };
        out.push_str(&format!(
            "        debug.field({}, {value});\n",
            literal(&member.name)
        ));
    }
    out.push_str("        debug.finish()\n    }\n}\n");
}

pub(crate) fn union(
    out: &mut String,
    service: &Service,
    name: &str,
    shape: &Structure,
    unknown: Option<&str>,
) {
    out.push_str(&format!("impl ::std::fmt::Debug for {name} {{\n    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{\n        match self {{\n"));
    for m in &shape.members {
        let sensitive = shape.sensitive()
            || service.sensitive(m)
            || crucible_codegen_common::sensitive_collection(service, &m.target);
        out.push_str(&format!(
            "            Self::{}({}) => f.debug_tuple({}).field({}).finish(),\n",
            variant(&m.name),
            if sensitive { "_" } else { "value" },
            literal(&format!("{name}::{}", variant(&m.name))),
            if sensitive {
                "&\"*** Sensitive Data Redacted ***\""
            } else {
                "value"
            }
        ));
    }
    // Unknown values have no sensitivity contract; do not expose their payload.
    if let Some(unknown) = unknown {
        out.push_str(&format!(
            "            Self::{unknown}(_) => f.write_str({}),\n",
            literal(&format!("{name}::Unknown(*** Sensitive Data Redacted ***)"))
        ));
    }
    out.push_str("        }\n    }\n}\n");
}
