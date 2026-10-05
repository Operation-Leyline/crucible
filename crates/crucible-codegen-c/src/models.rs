use crate::{File, Options, names};
use crucible_smithy::{
    ShapeId,
    ir::{Member, Service, Shape, SimpleKind, TimestampFormat},
};

pub fn complex(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Structure(_) | Shape::Union(_) | Shape::List(_) | Shape::Map(_)
    )
}
pub fn native_name(service: &Service, id: &ShapeId, prefix: &str) -> String {
    format!("{prefix}_models_{}", names::field(service.name_of(id)))
}
pub fn native_type(service: &Service, id: &ShapeId, prefix: &str) -> String {
    match service.shape(id).expect("shape") {
        Shape::Unit => format!("{prefix}_unit"),
        Shape::Simple(simple) => match simple.kind {
            SimpleKind::String => format!("{prefix}_string"),
            SimpleKind::Boolean => "bool".into(),
            SimpleKind::Byte => "int8_t".into(),
            SimpleKind::Short => "int16_t".into(),
            SimpleKind::Integer => "int32_t".into(),
            SimpleKind::Long => "int64_t".into(),
            SimpleKind::Float => "float".into(),
            SimpleKind::Double => "double".into(),
            SimpleKind::Blob => format!("{prefix}_blob"),
            SimpleKind::Timestamp => format!("{prefix}_timestamp"),
            SimpleKind::Document => format!("const {prefix}_json *"),
        },
        _ => native_name(service, id, prefix),
    }
}
pub fn slot_type(service: &Service, id: &ShapeId, prefix: &str) -> String {
    let ty = native_type(service, id, prefix);
    if complex(service.shape(id).expect("shape")) {
        format!("const {ty} *")
    } else {
        ty
    }
}
pub fn descriptor(service: &Service, id: &ShapeId, prefix: &str) -> String {
    format!(
        "{prefix}_shape_{}",
        service
            .shapes
            .keys()
            .position(|candidate| candidate == id)
            .expect("shape")
    )
}
fn format_name(format: TimestampFormat) -> &'static str {
    match format {
        TimestampFormat::DateTime => "date_time",
        TimestampFormat::EpochSeconds => "epoch_seconds",
        TimestampFormat::HttpDate => "http_date",
    }
}
fn member(
    service: &Service,
    owner: &str,
    value: &Member,
    prefix: &str,
    state: &str,
    offset: &str,
) -> String {
    let fallback = value
        .default
        .as_ref()
        .map(|v| names::view(&serde_json::to_string(v).expect("default")))
        .unwrap_or_else(|| "{NULL, 0}".into());
    format!(
        "    {{{}, &{}, {}, {}, {}, {prefix}_{}, {fallback}}},\n",
        names::view(&value.name),
        descriptor(service, &value.target, prefix),
        if state.is_empty() {
            "0".into()
        } else {
            format!("offsetof({owner}, {state})")
        },
        if offset.is_empty() {
            "0".into()
        } else {
            format!("offsetof({owner}, {offset})")
        },
        value.optional_on_client(),
        format_name(service.timestamp_format(value))
    )
}
fn kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Unit => "unit",
        Shape::Structure(_) => "structure",
        Shape::Union(_) => "union",
        Shape::Enum(_) => "enum",
        Shape::List(_) => "list",
        Shape::Map(_) => "map",
        Shape::Simple(simple) => match simple.kind {
            SimpleKind::String => "string",
            SimpleKind::Boolean => "boolean",
            SimpleKind::Byte => "byte",
            SimpleKind::Short => "short",
            SimpleKind::Integer => "integer",
            SimpleKind::Long => "long",
            SimpleKind::Float => "float",
            SimpleKind::Double => "double",
            SimpleKind::Blob => "blob",
            SimpleKind::Timestamp => "timestamp",
            SimpleKind::Document => "document",
        },
    }
}
pub fn emit(service: &Service, options: &Options) -> Vec<File> {
    let p = &options.prefix;
    let guard = format!("{}_MODELS_H", p.to_ascii_uppercase());
    let mut header = format!(
        "#ifndef {guard}\n#define {guard}\n#include \"runtime/schema.h\"\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n"
    );
    let mut source = format!(
        "#include \"{}/models.h\"\n#include <stddef.h>\n\n",
        options.package_name
    );
    for (id, shape) in &service.shapes {
        let native = native_name(service, id, p);
        if complex(shape) {
            header.push_str(&format!("typedef struct {native} {native};\n"));
        }
        if matches!(shape, Shape::Enum(_)) {
            header.push_str(&format!("typedef {p}_string {native};\n"));
        }
        header.push_str(&format!(
            "extern const {p}_shape {};\n",
            descriptor(service, id, p)
        ));
    }
    for (id, shape) in &service.shapes {
        let native = native_name(service, id, p);
        match shape {
            Shape::Structure(structure) => {
                header.push_str(&format!("\nstruct {native} {{\n"));
                if structure.members.is_empty() {
                    header.push_str("    unsigned char unused;\n");
                }
                for m in &structure.members {
                    header.push_str(&format!(
                        "    struct {{ {p}_presence state; {} value; }} {};\n",
                        slot_type(service, &m.target, p),
                        names::field(&m.name)
                    ));
                }
                header.push_str("};\n");
            }
            Shape::List(list) => {
                header.push_str(&format!(
                    "\nstruct {native} {{ size_t length; {} const *items; }};\n",
                    slot_type(service, &list.member.target, p)
                ));
            }
            Shape::Map(map) => {
                header.push_str(&format!("\ntypedef struct {{ {} key; {} value; }} {native}_entry;\nstruct {native} {{ size_t length; const {native}_entry *items; }};\n", slot_type(service, &map.key.target, p), slot_type(service, &map.value.target, p)));
            }
            Shape::Enum(enumeration) => {
                for value in &enumeration.values {
                    header.push_str(&format!("static inline {native} {native}_known_{}(void) {{ {native} result = {}; return result; }}\n", names::field(&value.name), names::view(&value.value)));
                }
            }
            Shape::Union(union) => {
                header.push_str(&format!("\nenum {native}_tag {{ {native}_unknown = 0"));
                for (at, m) in union.members.iter().enumerate() {
                    header.push_str(&format!(
                        ", {native}_tag_{} = {}",
                        names::field(&m.name),
                        at + 1
                    ));
                }
                header.push_str(&format!(" }};\nstruct {native} {{\n    uint32_t tag;\n    {p}_presence state;\n    union {{\n"));
                if union.members.is_empty() {
                    header.push_str("        unsigned char unused;\n");
                }
                for m in &union.members {
                    header.push_str(&format!(
                        "        {} {};\n",
                        slot_type(service, &m.target, p),
                        names::field(&m.name)
                    ));
                }
                header.push_str(&format!("    }} value;\n    {p}_string unknown_name;\n    const {p}_json *unknown_value;\n}};\n"));
            }
            _ => {}
        }
        if complex(shape) || matches!(shape, Shape::Enum(_)) {
            let desc = descriptor(service, id, p);
            header.push_str(&format!("{p}_status {native}_encode({p}_arena *arena, const {native} *value, const {p}_json **output, {p}_error *error);\n{p}_status {native}_decode({p}_arena *arena, const {p}_json *value, const {native} **output, {p}_error *error);\n"));
            source.push_str(&format!("{p}_status {native}_encode({p}_arena *arena, const {native} *value, const {p}_json **output, {p}_error *error) {{\n    return {p}_model_encode(arena, &{desc}, value, output, error);\n}}\n{p}_status {native}_decode({p}_arena *arena, const {p}_json *value, const {native} **output, {p}_error *error) {{\n    const void *native;\n    {p}_status status;\n    if (!output) return {p}_model_decode(arena, &{desc}, value, NULL, error);\n    status = {p}_model_decode(arena, &{desc}, value, &native, error);\n    if (status == {p}_ok) *output = (const {native} *)native;\n    return status;\n}}\n"));
        }
    }
    for (id, shape) in &service.shapes {
        let native = native_name(service, id, p);
        let desc = descriptor(service, id, p);
        let mut members = String::new();
        let count = match shape {
            Shape::Structure(structure) => {
                for m in &structure.members {
                    let f = names::field(&m.name);
                    members.push_str(&member(
                        service,
                        &native,
                        m,
                        p,
                        &format!("{f}.state"),
                        &format!("{f}.value"),
                    ));
                }
                structure.members.len()
            }
            Shape::Union(union) => {
                for m in &union.members {
                    members.push_str(&member(
                        service,
                        &native,
                        m,
                        p,
                        "state",
                        &format!("value.{}", names::field(&m.name)),
                    ));
                }
                union.members.len()
            }
            Shape::List(list) => {
                members.push_str(&member(service, &native, &list.member, p, "", ""));
                1
            }
            Shape::Map(map) => {
                members.push_str(&member(
                    service,
                    &format!("{native}_entry"),
                    &map.key,
                    p,
                    "",
                    "key",
                ));
                members.push_str(&member(
                    service,
                    &format!("{native}_entry"),
                    &map.value,
                    p,
                    "",
                    "value",
                ));
                2
            }
            _ => 0,
        };
        if count > 0 {
            source.push_str(&format!(
                "static const {p}_member {desc}_members[] = {{\n{members}}};\n"
            ));
        }
        let timestamp_format = match shape {
            Shape::Simple(simple) => simple.timestamp_format.unwrap_or(TimestampFormat::DateTime),
            _ => TimestampFormat::DateTime,
        };
        source.push_str(&format!("const {p}_shape {desc} = {{\n    .id = {}, .kind = {p}_kind_{}, .size = sizeof({}), .format = {p}_{},\n", names::view(&id.to_string()), kind(shape), native_type(service, id, p), format_name(timestamp_format)));
        if count > 0 {
            source.push_str(&format!(
                "    .members = {desc}_members, .member_count = {count},\n"
            ));
        }
        match shape {
            Shape::List(list) => source.push_str(&format!("    .length_offset = offsetof({native}, length), .items_offset = offsetof({native}, items), .item_size = sizeof({}),\n", slot_type(service, &list.member.target, p))),
            Shape::Map(_) => source.push_str(&format!("    .length_offset = offsetof({native}, length), .items_offset = offsetof({native}, items), .item_size = sizeof({native}_entry),\n")),
            Shape::Union(_) => source.push_str(&format!("    .tag_offset = offsetof({native}, tag), .state_offset = offsetof({native}, state), .unknown_name_offset = offsetof({native}, unknown_name), .unknown_value_offset = offsetof({native}, unknown_value),\n")),
            _ => {}
        }
        source.push_str("};\n");
    }
    source.push_str(&format!(
        "const {p}_shape *{p}_shape_lookup({p}_string id) {{\n    (void)id;\n"
    ));
    for id in service.shapes.keys() {
        let desc = descriptor(service, id, p);
        source.push_str(&format!(
            "    if ({p}_string_equal(id, {desc}.id)) return &{desc};\n"
        ));
    }
    source.push_str("    return NULL;\n}\n");
    header.push_str("\n#ifdef __cplusplus\n}\n#endif\n#endif\n");
    vec![
        File {
            path: format!("include/{}/models.h", options.package_name),
            contents: header,
        },
        File {
            path: "src/models.c".into(),
            contents: source,
        },
    ]
}
