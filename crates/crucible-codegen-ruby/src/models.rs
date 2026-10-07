use crate::{
    File, Options,
    names::{field, literal},
};
use crucible_smithy::ir::{Service, Shape};

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
                out.push_str(&format!("    class {name}{parent}\n"));
                for member in &structure.members {
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
                out.push_str("        object\n      end\n    end\n");
            }
            Shape::IntEnum(enumeration) => {
                out.push_str(&format!("    class {name}\n      attr_reader :value\n      def initialize(value)\n        raise ::ArgumentError, 'Expected signed 32-bit integer' unless value.is_a?(::Integer) && (-(2**31)...2**31).cover?(value)\n        @value = value\n        freeze\n      end\n      def self.from_wire(value, context) = new(context.codec.number(value).integer(-(2**31), 2**31 - 1))\n      def to_wire(_context) = @value\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
                for value in &enumeration.values {
                    out.push_str(&format!(
                        "      {} = new({})\n",
                        enum_constant(&value.name),
                        value.value
                    ));
                }
                out.push_str("    end\n");
            }
            Shape::Enum(enumeration) => {
                out.push_str(&format!("    class {name}\n      attr_reader :value\n      def initialize(value)\n        @value = {runtime}::WireCodec.string(value).freeze\n        freeze\n      end\n      def self.from_wire(value, _context) = new(value)\n      def to_wire(_context) = @value\n      def ==(other) = other.is_a?(self.class) && @value == other.value\n      alias eql? ==\n      def hash = @value.hash\n"));
                for value in &enumeration.values {
                    out.push_str(&format!(
                        "      {} = new({})\n",
                        enum_constant(&value.name),
                        literal(&value.value)
                    ));
                }
                out.push_str("    end\n");
            }
            Shape::Union(union) => {
                out.push_str(&format!("    class {name}\n      def self.from_wire(value, context)\n        object = {runtime}::WireCodec.object(value)\n        raise ::ArgumentError, 'Union requires one member' unless object.size == 1\n        key, value = object.first\n        case key\n"));
                for m in &union.members {
                    out.push_str(&format!("        when {} then {}.new(value.nil? ? nil : context.codec.decode(value, context.member(key)))\n",literal(&m.name),variant(&m.name)));
                }
                out.push_str(&format!("        else Unknown.new(key, {runtime}::JsonValue.new(value))\n        end\n      end\n"));
                for m in &union.members {
                    let wire = literal(&m.name);
                    out.push_str(&format!("      class {} < {name}\n        attr_reader :value\n        def initialize(value)\n          @value = value\n          freeze\n        end\n        def to_wire(context)\n          {{{wire} => @value.nil? ? nil : context.codec.encode(@value, context.member({wire}))}}\n        end\n      end\n",variant(&m.name)));
                }
                out.push_str(&format!("      class Unknown < {name}\n        attr_reader :name, :value\n        def initialize(name, value)\n          @name = {runtime}::WireCodec.string(name).freeze\n          raise ::ArgumentError, 'Expected JsonValue' unless value.is_a?({runtime}::JsonValue)\n          @value = value\n          freeze\n        end\n        def to_wire(context)\n          raise ::ArgumentError, 'Unknown variant name is declared' if context.shape.fetch('members').key?(@name)\n          {{@name => @value.value}}\n        end\n      end\n    end\n"));
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
