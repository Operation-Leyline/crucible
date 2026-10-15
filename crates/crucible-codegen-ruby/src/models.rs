use crate::{
    File, Options,
    names::{field, literal},
};
use crucible_codegen_common::docs;
use crucible_ir::{
    ShapeId,
    ir::{DocNotes, Member, Service, Shape, SimpleKind},
};

/// A YARD comment for model text; empty when the model has none.
pub(crate) fn yard(text: Option<&str>, notes: &DocNotes, indent: &str) -> String {
    let mut text = docs::annotated_block(text, notes).unwrap_or_default();
    if let Some(deprecated) = &notes.deprecated {
        text.push_str(&format!(
            "\n\n@deprecated {}",
            docs::escape_block(&deprecated.note())
        ));
    }
    docs::line_comment(indent, "#", text.trim_start())
}

/// The YARD spelling of a member's Ruby type.
fn yard_type(service: &Service, id: &ShapeId) -> String {
    match service.shape(id) {
        Some(Shape::Simple(s)) => match s.kind {
            SimpleKind::Boolean => "Boolean".into(),
            SimpleKind::String => "String".into(),
            SimpleKind::Blob => "String".into(),
            SimpleKind::Byte
            | SimpleKind::Short
            | SimpleKind::Integer
            | SimpleKind::Long
            | SimpleKind::BigInteger => "Integer".into(),
            SimpleKind::Float | SimpleKind::Double => "Float".into(),
            SimpleKind::BigDecimal => "BigDecimal".into(),
            SimpleKind::Timestamp => "Runtime::Timestamp".into(),
            SimpleKind::Document => "Runtime::JsonValue".into(),
        },
        Some(Shape::List(list)) => format!("Array<{}>", yard_type(service, &list.member.target)),
        Some(Shape::Map(map)) => format!(
            "Hash{{String => {}}}",
            yard_type(service, &map.value.target)
        ),
        Some(Shape::Unit { .. }) | None => "Runtime::Unit".into(),
        Some(_) => service.name_of(id).to_string(),
    }
}

/// `inspect` and, outside errors, `to_s`: thin, with formatting and the
/// redaction marker living in `runtime/describe.rb`. Exceptions keep their
/// message as `to_s`.
fn inspect(
    service: &Service,
    name: &str,
    members: &[Member],
    sensitive: bool,
    error: bool,
) -> String {
    let mut entries: Vec<String> = members
        .iter()
        .map(|member| {
            let field = field(&member.name);
            if docs::is_redacted(service, sensitive, member) {
                format!("{field}: Runtime::Describe.redact(@{field})")
            } else if matches!(service.shape(&member.target), Some(Shape::Simple(s)) if s.kind == SimpleKind::Blob) {
                format!("{field}: Runtime::Describe.bytes(@{field})")
            } else {
                format!("{field}: @{field}")
            }
        })
        .collect();
    if error {
        entries.push("metadata: @metadata".into());
    }
    let mut out = format!(
        "      # @return [String] `{name}(member=value, ...)` with sensitive members redacted and unset ones omitted\n      def inspect = Runtime::Describe.format({}, {{{}}})\n",
        literal(name),
        entries.join(", ")
    );
    if !error {
        out.push_str("      alias to_s inspect\n");
    }
    out
}

pub fn variant(name: &str) -> String {
    let mut chars = name.chars();
    format!(
        "{}{}Variant",
        chars.next().expect("name").to_ascii_uppercase(),
        chars.as_str()
    )
}
pub fn enum_constant(name: &str) -> String {
    let value = name.to_ascii_uppercase();
    if !value.starts_with(|c: char| c.is_ascii_uppercase())
        || ["BEGIN", "END"].contains(&value.as_str())
    {
        format!("VALUE_{value}")
    } else {
        value
    }
}
pub fn emit(service: &Service, options: &Options) -> Vec<File> {
    let exception = crucible_codegen_common::service_exception_name(service);
    let module = &options.module_name;
    let runtime = format!("::{module}::Runtime");
    let mut files = Vec::new();
    let mut registry = format!(
        "# frozen_string_literal: true\nmodule {module}\n    module Registry\n      TYPES = {{\n"
    );
    for (id, shape) in &service.shapes {
        let name = service.name_of(id);
        let mut out = format!("# frozen_string_literal: true\nmodule {module}\n");
        match shape {
            Shape::Structure(structure) => {
                let error = structure.error.is_some();
                let parent = if error {
                    format!(" < {runtime}::{exception}")
                } else {
                    String::new()
                };
                out.push_str(&yard(structure.docs.as_deref(), &structure.notes, "    "));
                out.push_str(&format!("    class {name}{parent}\n"));
                for member in &structure.members {
                    let mut text = docs::annotated_block(member.docs.as_deref(), &member.notes)
                        .unwrap_or_default();
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    if let Some(deprecated) = &member.notes.deprecated {
                        text.push_str(&format!(
                            "@deprecated {}\n",
                            docs::escape_block(&deprecated.note())
                        ));
                    }
                    let kind = yard_type(service, &member.target);
                    if member.optional_on_client() {
                        text.push_str(&format!("@return [{kind}, nil] nil when sent as null; {{Runtime::UNSET}} when omitted"));
                    } else {
                        text.push_str(&format!("@return [{kind}]"));
                    }
                    out.push_str(&docs::line_comment("      ", "#", &text));
                    out.push_str(&format!("      attr_reader :{}\n", field(&member.name)));
                }
                let mut args: Vec<_> = structure
                    .members
                    .iter()
                    .map(|m| {
                        format!(
                            "{}:{}",
                            field(&m.name),
                            if m.optional_on_client() {
                                format!(" {runtime}::UNSET")
                            } else {
                                String::new()
                            }
                        )
                    })
                    .collect();
                if error {
                    args.push(format!("metadata: {runtime}::ErrorMetadata.new"));
                }
                out.push_str("      # Optional members default to {Runtime::UNSET}, which leaves them out of the request.\n");
                out.push_str(&format!("      def initialize({})\n", args.join(", ")));
                if error {
                    out.push_str("        super(metadata)\n");
                }
                for member in &structure.members {
                    let field = field(&member.name);
                    out.push_str(&format!("        @{field} = {field}\n"));
                }
                // Exceptions need mutable backtraces when raised.
                if !error {
                    out.push_str("        freeze\n");
                }
                out.push_str("      end\n");
                out.push_str(&format!("      def self.from_wire(value, context{})\n        object = {runtime}::WireCodec.object(value)\n        new(\n",if error {format!(", metadata = {runtime}::ErrorMetadata.new")} else {String::new()}));
                for m in &structure.members {
                    out.push_str(&format!(
                        "          {}: context.read({}, object, optional: {}),\n",
                        field(&m.name),
                        literal(&m.name),
                        m.optional_on_client()
                    ));
                }
                if error {
                    out.push_str("          metadata: metadata,\n");
                }
                out.push_str(
                    "        )\n      end\n      def to_wire(context)\n        object = {}\n",
                );
                for m in &structure.members {
                    out.push_str(&format!(
                        "        context.put(object, {}, @{}, optional: {})\n",
                        literal(&m.name),
                        field(&m.name),
                        m.optional_on_client()
                    ));
                }
                out.push_str("        object\n      end\n");
                out.push_str(&inspect(
                    service,
                    name,
                    &structure.members,
                    structure.sensitive(),
                    error,
                ));
                out.push_str("    end\n");
            }
            Shape::IntEnum(enumeration) => {
                out.push_str(&yard(
                    enumeration.docs.as_deref(),
                    &enumeration.notes,
                    "    ",
                ));
                out.push_str(&format!("    class {name}\n      # @return [Integer] the integer sent on the wire\n      attr_reader :value\n      def initialize(value)\n        raise ::ArgumentError, 'Expected signed 32-bit integer' unless value.is_a?(::Integer) && (-(2**31)...2**31).cover?(value)\n        @value = value\n        freeze\n      end\n      def self.from_wire(value, context) = new(context.codec.number(value).integer(-(2**31), 2**31 - 1))\n      def to_wire(_context) = @value\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
                for value in &enumeration.values {
                    out.push_str(&yard(value.docs.as_deref(), &value.notes, "      "));
                    out.push_str(&format!(
                        "      {} = new({})\n",
                        enum_constant(&value.name),
                        value.value
                    ));
                }
                out.push_str(&format!(
                    "      def inspect = '{name}(' + @value.to_s + ')'\n      alias to_s inspect\n"
                ));
                out.push_str("    end\n");
            }
            Shape::Enum(enumeration) => {
                out.push_str(&yard(
                    enumeration.docs.as_deref(),
                    &enumeration.notes,
                    "    ",
                ));
                out.push_str(&format!("    class {name}\n      # @return [String] the string sent on the wire; a value this client does not list is kept as is\n      attr_reader :value\n      def initialize(value)\n        @value = {runtime}::WireCodec.string(value).freeze\n        freeze\n      end\n      def self.from_wire(value, _context) = new(value)\n      def to_wire(_context) = @value\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
                for value in &enumeration.values {
                    out.push_str(&yard(value.docs.as_deref(), &value.notes, "      "));
                    out.push_str(&format!(
                        "      {} = new({})\n",
                        enum_constant(&value.name),
                        literal(&value.value)
                    ));
                }
                out.push_str(&format!(
                    "      def inspect = '{name}(' + @value + ')'\n      alias to_s inspect\n"
                ));
                out.push_str("    end\n");
            }
            Shape::Union(union) => {
                out.push_str(&yard(union.docs.as_deref(), &union.notes, "    "));
                out.push_str(&format!("    class {name}\n      def self.from_wire(value, context)\n        object = {runtime}::WireCodec.object(value)\n        raise ::ArgumentError, 'Union requires one member' unless object.size == 1\n        key, value = object.first\n        case key\n"));
                for m in &union.members {
                    out.push_str(&format!("        when {} then {}.new(value.nil? ? nil : context.codec.decode(value, context.member(key)))\n",literal(&m.name),variant(&m.name)));
                }
                out.push_str(&format!("        else Unknown.new(key, {runtime}::JsonValue.new(value))\n        end\n      end\n"));
                for m in &union.members {
                    let wire = literal(&m.name);
                    out.push_str(&yard(m.docs.as_deref(), &m.notes, "      "));
                    let shown = if docs::is_redacted(service, union.sensitive(), m) {
                        "Runtime::Describe.redact(@value)"
                    } else {
                        "@value"
                    };
                    out.push_str(&format!("      class {} < {name}\n        # @return [{}, nil] the member's value; nil when the union carries an explicit null\n        attr_reader :value\n        def initialize(value)\n          @value = value\n          freeze\n        end\n        def to_wire(context)\n          {{{wire} => @value.nil? ? nil : context.codec.encode(@value, context.member({wire}))}}\n        end\n        def inspect = Runtime::Describe.format({}, {{{}: {shown}}})\n        alias to_s inspect\n      end\n",variant(&m.name), yard_type(service, &m.target), literal(name), field(&m.name)));
                }
                let type_literal = literal(name);
                out.push_str(&format!("      # A member this client does not know, kept so it can be sent back unchanged.\n      class Unknown < {name}\n        # @return [String] the member name received\n        attr_reader :name\n        # @return [Runtime::JsonValue] the raw value received\n        attr_reader :value\n        # Omits the value: nothing says whether it is sensitive.\n        def inspect = Runtime::Describe.format({type_literal}, {{unknown_variant: @name}})\n        alias to_s inspect\n        def initialize(name, value)\n          @name = {runtime}::WireCodec.string(name).freeze\n          raise ::ArgumentError, 'Expected JsonValue' unless value.is_a?({runtime}::JsonValue)\n          @value = value\n          freeze\n        end\n        def to_wire(context)\n          raise ::ArgumentError, 'Unknown variant name is declared' if context.shape.fetch('members').key?(@name)\n          {{@name => @value.value}}\n        end\n      end\n    end\n"));
            }
            _ => continue,
        }
        out.push_str("end\n");
        registry.push_str(&format!(
            "        {} => {name},\n",
            literal(&id.to_string())
        ));
        files.push(File {
            path: format!("lib/{}/models/{}.rb", options.package_name, field(name)),
            contents: out,
        });
    }
    registry.push_str("      }.freeze\n    end\nend\n");
    files.push(File {
        path: format!("lib/{}/models/registry.rb", options.package_name),
        contents: registry,
    });
    files
}
