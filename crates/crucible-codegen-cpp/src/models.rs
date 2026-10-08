use crate::{
    File, Options,
    names::{field, literal},
};
use crucible_smithy::{
    ShapeId,
    ir::{Service, Shape, SimpleKind},
};
use std::collections::{BTreeMap, BTreeSet};

pub fn model(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
    )
}
pub fn variant(name: &str, member: &str) -> String {
    let mut chars = member.chars();
    format!(
        "{name}{}{}Variant",
        chars.next().expect("name").to_ascii_uppercase(),
        chars.as_str()
    )
}
fn targets(
    service: &Service,
    id: &ShapeId,
    result: &mut BTreeSet<ShapeId>,
    seen: &mut BTreeSet<ShapeId>,
) {
    if !seen.insert(id.clone()) {
        return;
    }
    match service.shape(id).expect("shape") {
        Shape::List(list) => targets(service, &list.member.target, result, seen),
        Shape::Map(map) => {
            targets(service, &map.key.target, result, seen);
            targets(service, &map.value.target, result, seen);
        }
        shape if model(shape) => {
            result.insert(id.clone());
        }
        _ => {}
    }
}
fn reaches(
    graph: &BTreeMap<ShapeId, BTreeSet<ShapeId>>,
    start: &ShapeId,
    target: &ShapeId,
    seen: &mut BTreeSet<ShapeId>,
) -> bool {
    start == target
        || seen.insert(start.clone())
            && graph
                .get(start)
                .is_some_and(|edges| edges.iter().any(|next| reaches(graph, next, target, seen)))
}
pub struct Layout<'a> {
    service: &'a Service,
    options: &'a Options,
    graph: BTreeMap<ShapeId, BTreeSet<ShapeId>>,
}
impl<'a> Layout<'a> {
    pub fn new(service: &'a Service, options: &'a Options) -> Self {
        let mut graph = BTreeMap::new();
        for (id, shape) in &service.shapes {
            if !model(shape) {
                continue;
            }
            let mut edges = BTreeSet::new();
            if let Shape::Structure(structure) | Shape::Union(structure) = shape {
                for member in &structure.members {
                    targets(service, &member.target, &mut edges, &mut BTreeSet::new());
                }
            }
            graph.insert(id.clone(), edges);
        }
        Self {
            service,
            options,
            graph,
        }
    }
    fn boxed(&self, owner: &ShapeId, target: &ShapeId) -> bool {
        reaches(&self.graph, target, owner, &mut BTreeSet::new())
    }
    fn ordered(&self) -> Vec<ShapeId> {
        fn visit(
            layout: &Layout<'_>,
            id: &ShapeId,
            seen: &mut BTreeSet<ShapeId>,
            result: &mut Vec<ShapeId>,
        ) {
            if !seen.insert(id.clone()) {
                return;
            }
            for target in &layout.graph[id] {
                if !layout.boxed(id, target) {
                    visit(layout, target, seen, result);
                }
            }
            result.push(id.clone());
        }
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for id in self.graph.keys() {
            visit(self, id, &mut seen, &mut result);
        }
        result
    }
    pub fn ty(&self, owner: &ShapeId, target: &ShapeId) -> String {
        let runtime = format!("::{}::runtime", self.options.namespace);
        match self.service.shape(target).expect("shape") {
            Shape::Unit { .. } => format!("{runtime}::Unit"),
            Shape::Simple(simple) => match simple.kind {
                SimpleKind::BigInteger => format!("{runtime}::BigInteger"),
                SimpleKind::BigDecimal => format!("{runtime}::BigDecimal"),
                SimpleKind::String => "std::string".into(),
                SimpleKind::Boolean => "bool".into(),
                SimpleKind::Byte => "std::int8_t".into(),
                SimpleKind::Short => "std::int16_t".into(),
                SimpleKind::Integer => "std::int32_t".into(),
                SimpleKind::Long => "std::int64_t".into(),
                SimpleKind::Float => "float".into(),
                SimpleKind::Double => "double".into(),
                SimpleKind::Blob if simple.streaming => format!("{runtime}::ByteStream"),
                SimpleKind::Blob => format!("{runtime}::Blob"),
                SimpleKind::Timestamp => format!("{runtime}::Timestamp"),
                SimpleKind::Document => format!("{runtime}::Json"),
            },
            Shape::List(list) => format!(
                "std::vector<{}>",
                if list.sparse {
                    format!("std::optional<{}>", self.ty(owner, &list.member.target))
                } else {
                    self.ty(owner, &list.member.target)
                }
            ),
            Shape::Map(map) => format!(
                "std::map<{}, {}>",
                self.ty(owner, &map.key.target),
                if map.sparse {
                    format!("std::optional<{}>", self.ty(owner, &map.value.target))
                } else {
                    self.ty(owner, &map.value.target)
                }
            ),
            _ => {
                let native = format!(
                    "::{}::models::{}",
                    self.options.namespace,
                    self.service.name_of(target)
                );
                if self.boxed(owner, target) {
                    format!("{runtime}::Box<{native}>")
                } else {
                    native
                }
            }
        }
    }
    pub fn emit(&self) -> Vec<File> {
        let exception = crucible_codegen_common::service_exception_name(self.service);
        let ns = &self.options.namespace;
        let runtime = format!("::{ns}::runtime");
        let mut header = format!(
            "#pragma once\n#include \"runtime/codec.hpp\"\n#include <compare>\n#include <exception>\n#include <variant>\n\nnamespace {ns} {{ using {exception} = runtime::{exception}; }}\nnamespace {ns}::models {{\n"
        );
        let mut source = format!(
            "#include \"{}/models.hpp\"\n\nnamespace {ns}::runtime {{\n",
            self.options.package_name
        );
        let mut codecs = format!("\nnamespace {ns}::runtime {{\n");
        for id in self.graph.keys() {
            header.push_str(&format!("struct {};\n", self.service.name_of(id)));
        }
        for id in self.ordered() {
            let name = self.service.name_of(&id);
            let qualified = format!("::{ns}::models::{name}");
            match self.service.shape(&id).expect("shape") {
                Shape::Structure(structure) => {
                    let error = structure.error.is_some();
                    header.push_str(&format!(
                        "\nstruct {name}{} {{\n",
                        if error {
                            format!(" : {runtime}::{exception}")
                        } else {
                            String::new()
                        }
                    ));
                    for member in &structure.members {
                        let native = self.ty(&id, &member.target);
                        let native = if member.optional_on_client() {
                            format!("{runtime}::Field<{native}>")
                        } else {
                            native
                        };
                        header.push_str(&format!("    {native} {};\n", field(&member.name)));
                    }
                    let members: Vec<_> = structure
                        .members
                        .iter()
                        .filter(|m| !m.optional_on_client())
                        .chain(structure.members.iter().filter(|m| m.optional_on_client()))
                        .collect();
                    let mut args: Vec<_> = members
                        .iter()
                        .map(|m| {
                            let native = self.ty(&id, &m.target);
                            if m.optional_on_client() {
                                format!("{runtime}::Field<{native}> {} = {{}}", field(&m.name))
                            } else {
                                format!("{native} {}", field(&m.name))
                            }
                        })
                        .collect();
                    if error {
                        args.push(format!("{runtime}::ErrorMetadata metadata = {{}}"));
                    }
                    let mut initializers = Vec::new();
                    if error {
                        initializers.push(format!("{runtime}::{exception}(std::move(metadata))"));
                    }
                    initializers.extend(
                        structure
                            .members
                            .iter()
                            .map(|m| format!("{}(std::move({}))", field(&m.name), field(&m.name))),
                    );
                    header.push_str(&format!(
                        "    explicit {name}({}){} {{}}\n}};\n",
                        args.join(", "),
                        if initializers.is_empty() {
                            String::new()
                        } else {
                            format!(" : {}", initializers.join(", "))
                        }
                    ));
                    let metadata = if error {
                        ", ErrorMetadata metadata = {}"
                    } else {
                        ""
                    };
                    codecs.push_str(&format!("template<> struct Codec<{qualified}> {{\n    static {qualified} decode(const Json& value, const Context& context{metadata});\n    static Json encode(const {qualified}& value, const Context& context);\n}};\n"));
                    source.push_str(&format!("{qualified} Codec<{qualified}>::decode([[maybe_unused]] const Json& value, [[maybe_unused]] const Context& context{}) {{\n    (void)value.object();\n    return {qualified}{{",if error{", ErrorMetadata metadata"}else{""}));
                    let mut decoded: Vec<_> = members
                        .iter()
                        .map(|m| {
                            format!(
                                "{}<{}>(value, context, {})",
                                if m.optional_on_client() {
                                    "optional"
                                } else {
                                    "read"
                                },
                                self.ty(&id, &m.target),
                                literal(&m.name)
                            )
                        })
                        .collect();
                    if error {
                        decoded.push("std::move(metadata)".into());
                    }
                    source.push_str(&format!("{}}};\n}}\nJson Codec<{qualified}>::encode([[maybe_unused]] const {qualified}& value, [[maybe_unused]] const Context& context) {{\n    Json::Object object;\n",decoded.join(", ")));
                    for m in &structure.members {
                        source.push_str(&format!(
                            "    put(object, context, {}, value.{});\n",
                            literal(&m.name),
                            field(&m.name)
                        ));
                    }
                    source.push_str("    return Json(std::move(object));\n}\n");
                }
                Shape::IntEnum(enumeration) => {
                    header.push_str(&format!("\nstruct {name} {{\n    std::int32_t value;\n    explicit {name}(std::int32_t value = 0) : value(value) {{}}\n    auto operator<=>(const {name}&) const = default;\n"));
                    for item in &enumeration.values {
                        header.push_str(&format!(
                            "    static {name} known_{}() {{ return {name}{{{}}}; }}\n",
                            field(&item.name),
                            item.value
                        ));
                    }
                    header.push_str("};\n");
                    codecs.push_str(&format!("template<> struct Codec<{qualified}> {{\n    static {qualified} decode(const Json& value, const Context& context) {{ return {qualified}{{Codec<std::int32_t>::decode(value, context)}}; }}\n    static Json encode(const {qualified}& value, const Context& context) {{ return Codec<std::int32_t>::encode(value.value, context); }}\n}};\n"));
                }
                Shape::Enum(enumeration) => {
                    header.push_str(&format!("\nstruct {name} {{\n    std::string value;\n    explicit {name}(std::string value = {{}}) : value(std::move(value)) {{}}\n    auto operator<=>(const {name}&) const = default;\n"));
                    for item in &enumeration.values {
                        header.push_str(&format!(
                            "    static {name} known_{}() {{ return {name}{{{}}}; }}\n",
                            field(&item.name),
                            literal(&item.value)
                        ));
                    }
                    header.push_str("};\n");
                    codecs.push_str(&format!("template<> struct Codec<{qualified}> {{\n    static {qualified} decode(const Json& value, const Context& context) {{ return {qualified}{{Codec<std::string>::decode(value, context)}}; }}\n    static Json encode(const {qualified}& value, const Context& context) {{ return Codec<std::string>::encode(value.value, context); }}\n}};\n"));
                }
                Shape::Union(union) => {
                    let mut variants = Vec::new();
                    for m in &union.members {
                        let variant = variant(name, &m.name);
                        header.push_str(&format!(
                            "struct {variant} {{ std::optional<{}> value; }};\n",
                            self.ty(&id, &m.target)
                        ));
                        variants.push(variant);
                    }
                    let unknown = format!("{name}Unknown");
                    header.push_str(&format!(
                        "struct {unknown} {{ std::string name; {runtime}::Json value; }};\n"
                    ));
                    variants.push(unknown.clone());
                    header.push_str(&format!(
                        "struct {name} {{ std::variant<{}> value; }};\n",
                        variants.join(", ")
                    ));
                    codecs.push_str(&format!("template<> struct Codec<{qualified}> {{\n    static {qualified} decode(const Json& value, const Context& context);\n    static Json encode(const {qualified}& value, const Context& context);\n}};\n"));
                    source.push_str(&format!("{qualified} Codec<{qualified}>::decode(const Json& value, const Context& context) {{\n    const auto& object = value.object();\n    if (object.size() != 1) throw std::invalid_argument(\"Union requires one member\");\n    const auto& [name, member] = *object.begin();\n"));
                    for m in &union.members {
                        let variant = variant(name, &m.name);
                        let ty = self.ty(&id, &m.target);
                        source.push_str(&format!("    if (name == {}) return {qualified}{{::{ns}::models::{variant}{{member.is_null() ? std::optional<{ty}>{{}} : std::optional<{ty}>{{Codec<{ty}>::decode(member, context.member(name))}}}}}};\n",literal(&m.name)));
                    }
                    source.push_str(&format!("    return {qualified}{{::{ns}::models::{unknown}{{name, member}}}};\n}}\nJson Codec<{qualified}>::encode(const {qualified}& value, const Context& context) {{\n    return std::visit([&](const auto& variant) -> Json {{\n        using T = std::decay_t<decltype(variant)>;\n"));
                    for (index, m) in union.members.iter().enumerate() {
                        let variant = variant(name, &m.name);
                        let ty = self.ty(&id, &m.target);
                        let key = literal(&m.name);
                        source.push_str(&format!("        {}if constexpr (std::is_same_v<T, ::{ns}::models::{variant}>) return Json(Json::Object{{{{{key}, variant.value ? Codec<{ty}>::encode(*variant.value, context.member({key})) : Json()}}}});\n",if index==0{""}else{"else "}));
                    }
                    source.push_str("        else {\n            if (context.shape().at(\"members\").find(variant.name)) throw std::invalid_argument(\"Unknown variant name is declared\");\n            return Json(Json::Object{{variant.name, variant.value}});\n        }\n    }, value.value);\n}\n");
                }
                _ => unreachable!(),
            }
        }
        header.push_str("}\n");
        codecs.push_str("}\n");
        header.push_str(&codecs);
        source.push_str("}\n");
        vec![
            File {
                path: format!("include/{}/models.hpp", self.options.package_name),
                contents: header,
            },
            File {
                path: "src/models.cpp".into(),
                contents: source,
            },
        ]
    }
}
