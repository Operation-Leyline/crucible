use crate::{
    File,
    names::{field, literal, ty, variant},
};
use crucible_smithy::ir::{Service, Shape};

pub fn emit(service: &Service, namespace: &str) -> Vec<File> {
    let exception = crucible_codegen_common::service_exception_name(service);
    let mut files = Vec::new();
    let runtime = format!("\\{namespace}\\Runtime");
    let header = format!("<?php\ndeclare(strict_types=1);\nnamespace {namespace};\n\n");
    let mut decode = format!(
        "{header}final class Decoder\n{{\n    public static function model(string $target, mixed $value, {runtime}\\WireContext $context): mixed\n    {{\n        return match ($target) {{\n"
    );
    let mut errors = format!(
        "    public static function error(string $target, mixed $value, {runtime}\\WireContext $context, {runtime}\\ErrorMetadata $metadata): {runtime}\\ServiceError\n    {{\n        return match ($target) {{\n"
    );
    for (id, shape) in &service.shapes {
        let name = service.name_of(id);
        let target = literal(&id.to_string());
        let mut out = header.clone();
        match shape {
            Shape::Structure(structure) => {
                let error = structure.error.is_some();
                let base = if error {
                    format!(" extends \\{namespace}\\{exception}")
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "final class {name}{base} implements {runtime}\\WireModel\n{{\n    /**\n"
                ));
                let members: Vec<_> = structure
                    .members
                    .iter()
                    .filter(|m| !m.optional_on_client())
                    .chain(structure.members.iter().filter(|m| m.optional_on_client()))
                    .collect();
                for member in &members {
                    let value = ty(service, &member.target, namespace, true);
                    let value = if member.optional_on_client() {
                        format!("{value}|null|{runtime}\\Absent")
                    } else {
                        value
                    };
                    out.push_str(&format!("     * @param {value} ${}\n", field(&member.name)));
                }
                out.push_str("     */\n    public function __construct(\n");
                for member in &members {
                    let value = ty(service, &member.target, namespace, false);
                    let value = if member.optional_on_client() {
                        format!("{value}|null|{runtime}\\Absent")
                    } else {
                        value
                    };
                    let default = if member.optional_on_client() {
                        format!(" = {runtime}\\Absent::Value")
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "        public readonly {value} ${}{default},\n",
                        field(&member.name)
                    ));
                }
                if error {
                    out.push_str(&format!("        {runtime}\\ErrorMetadata $metadata = new {runtime}\\ErrorMetadata(),\n"));
                }
                out.push_str(if error {
                    "    ) { parent::__construct($metadata); }\n"
                } else {
                    "    ) {}\n"
                });
                let extra = if error {
                    format!(", {runtime}\\ErrorMetadata $metadata = new {runtime}\\ErrorMetadata()")
                } else {
                    String::new()
                };
                out.push_str(&format!("    public static function fromWire(mixed $value, {runtime}\\WireContext $context{extra}): self\n    {{\n        $object = {runtime}\\WireCodec::object($value);\n        return new self(\n"));
                for member in &structure.members {
                    out.push_str(&format!(
                        "            {}: $context->{}({}, $object),\n",
                        field(&member.name),
                        if member.optional_on_client() {
                            "optional"
                        } else {
                            "read"
                        },
                        literal(&member.name)
                    ));
                }
                if error {
                    out.push_str("            metadata: $metadata,\n");
                }
                out.push_str(&format!("        );\n    }}\n    public function toWire({runtime}\\WireContext $context): {runtime}\\JsonObject\n    {{\n        $object = [];\n"));
                for member in &structure.members {
                    out.push_str(&format!(
                        "        $context->put($object, {}, $this->{}, {});\n",
                        literal(&member.name),
                        field(&member.name),
                        member.optional_on_client()
                    ));
                }
                out.push_str(&format!(
                    "        return new {runtime}\\JsonObject($object);\n    }}\n}}\n"
                ));
                decode.push_str(&format!(
                    "            {target} => {name}::fromWire($value, $context),\n"
                ));
                if error {
                    errors.push_str(&format!(
                        "            {target} => {name}::fromWire($value, $context, $metadata),\n"
                    ));
                }
            }
            Shape::IntEnum(enumeration) => {
                out.push_str(&format!("final readonly class {name} implements {runtime}\\WireModel\n{{\n    public function __construct(public int $value) {{ if ($value < -2147483648 || $value > 2147483647) throw new \\UnexpectedValueException('Integer enum overflow'); }}\n"));
                for value in &enumeration.values {
                    out.push_str(&format!(
                        "    public const {} = {};\n",
                        crate::names::constant(&value.name),
                        value.value
                    ));
                }
                out.push_str(&format!("    public function toWire({runtime}\\WireContext $context): {runtime}\\JsonNumber {{ return new {runtime}\\JsonNumber((string)$this->value); }}\n}}\n"));
                decode.push_str(&format!("            {target} => new {name}($value instanceof {runtime}\\JsonNumber ? $value->integer(-2147483648, 2147483647) : throw new \\UnexpectedValueException('Expected integer enum')),\n"));
            }
            Shape::Enum(enumeration) => {
                out.push_str(&format!("final readonly class {name} implements {runtime}\\WireModel\n{{\n    public function __construct(public string $value) {{}}\n"));
                for value in &enumeration.values {
                    out.push_str(&format!(
                        "    public const {} = {};\n",
                        crate::names::constant(&value.name),
                        literal(&value.value)
                    ));
                }
                out.push_str(&format!("    public function toWire({runtime}\\WireContext $context): string {{ return $this->value; }}\n}}\n"));
                decode.push_str(&format!(
                    "            {target} => new {name}({runtime}\\WireCodec::string($value)),\n"
                ));
            }
            Shape::Union(union) => {
                out.push_str(&format!("abstract class {name} implements {runtime}\\WireModel\n{{\n    public static function fromWire(mixed $value, {runtime}\\WireContext $context): self\n    {{\n        $object = {runtime}\\WireCodec::object($value);\n        if (count($object->members) !== 1) throw new \\UnexpectedValueException('Union requires one member');\n        $key = (string)array_key_first($object->members);\n        $value = $object->get($key);\n        return match ($key) {{\n"));
                for member in &union.members {
                    out.push_str(&format!("            {} => new {name}{}($value === null ? null : $context->codec->decode($value, $context->member($key))),\n", literal(&member.name), variant(&member.name)));
                }
                out.push_str(&format!("            default => new {name}UnknownVariant($key, new {runtime}\\JsonValue($value)),\n        }};\n    }}\n}}\n"));
                for member in &union.members {
                    let variant = format!("{name}{}", variant(&member.name));
                    let wire_name = literal(&member.name);
                    let member_type = ty(service, &member.target, namespace, false);
                    let doc = ty(service, &member.target, namespace, true);
                    files.push(File { path:format!("src/{variant}.php"), contents:format!("{header}final class {variant} extends {name}\n{{\n    /** @param {doc}|null $value */\n    public function __construct(public readonly ?{member_type} $value) {{}}\n    public function toWire({runtime}\\WireContext $context): {runtime}\\JsonObject\n    {{\n        return new {runtime}\\JsonObject([{wire_name} => $this->value === null ? null : $context->codec->encode($this->value, $context->member({wire_name}))]);\n    }}\n}}\n") });
                }
                let unknown = format!("{name}UnknownVariant");
                files.push(File {path:format!("src/{unknown}.php"),contents:format!("{header}final class {unknown} extends {name}\n{{\n    public function __construct(public readonly string $name, public readonly {runtime}\\JsonValue $value) {{}}\n    public function toWire({runtime}\\WireContext $context): {runtime}\\JsonObject\n    {{\n        if ({runtime}\\WireCodec::object($context->shape()->get('members'))->has($this->name)) throw new \\UnexpectedValueException('Unknown variant name is declared');\n        return new {runtime}\\JsonObject([$this->name => $this->value->value()]);\n    }}\n}}\n")});
                decode.push_str(&format!(
                    "            {target} => {name}::fromWire($value, $context),\n"
                ));
            }
            _ => continue,
        }
        files.push(File {
            path: format!("src/{name}.php"),
            contents: out,
        });
    }
    decode.push_str("            default => throw new \\UnexpectedValueException('Unknown model: ' . $target),\n        };\n    }\n");
    errors.push_str(&format!("            default => new {runtime}\\UnknownServiceError($metadata),\n        }};\n    }}\n"));
    files.push(File {
        path: "src/Decoder.php".into(),
        contents: format!("{decode}{errors}}}\n"),
    });
    files
}
