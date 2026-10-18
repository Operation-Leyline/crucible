//! What a rule can talk about: the kinds it ranges over and, for each kind,
//! the flags and paths its conditions may read (decision 46). One table, so
//! the checker, the evaluator and the diagnostics agree.

/// A word after `every` or `no`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Service,
    Resource,
    Operation,
    Error,
    Structure,
    Union,
    /// A string or integer enum.
    Enum,
    List,
    Map,
    /// Strings, and string enums.
    String,
    Boolean,
    /// Every numeric shape, and integer enums.
    Number,
    Byte,
    Short,
    Integer,
    Long,
    Float,
    Double,
    BigInteger,
    BigDecimal,
    Timestamp,
    Blob,
    Document,
    /// Any named shape.
    Shape,
    /// A member of a structure, union, list or map.
    Member,
    InputMember,
    OutputMember,
}

const KINDS: &[(&str, Kind)] = &[
    ("service", Kind::Service),
    ("resource", Kind::Resource),
    ("operation", Kind::Operation),
    ("error", Kind::Error),
    ("structure", Kind::Structure),
    ("union", Kind::Union),
    ("enum", Kind::Enum),
    ("list", Kind::List),
    ("map", Kind::Map),
    ("string", Kind::String),
    ("boolean", Kind::Boolean),
    ("number", Kind::Number),
    ("byte", Kind::Byte),
    ("short", Kind::Short),
    ("integer", Kind::Integer),
    ("long", Kind::Long),
    ("float", Kind::Float),
    ("double", Kind::Double),
    ("bigInteger", Kind::BigInteger),
    ("bigDecimal", Kind::BigDecimal),
    ("timestamp", Kind::Timestamp),
    ("blob", Kind::Blob),
    ("document", Kind::Document),
    ("shape", Kind::Shape),
    ("member", Kind::Member),
];

impl Kind {
    /// The kind a rule's words name: `operation`, `input member`.
    pub(super) fn from_words(words: &[&str]) -> Option<Kind> {
        match words {
            ["input", "member"] => Some(Kind::InputMember),
            ["output", "member"] => Some(Kind::OutputMember),
            [one] => KINDS
                .iter()
                .find(|(name, _)| name == one)
                .map(|(_, kind)| *kind),
            _ => None,
        }
    }

    pub(super) fn words(self) -> &'static str {
        match self {
            Kind::InputMember => "input member",
            Kind::OutputMember => "output member",
            other => KINDS
                .iter()
                .find(|(_, kind)| *kind == other)
                .map_or("?", |(name, _)| name),
        }
    }

    /// `an operation`, `a member`.
    pub(super) fn indefinite(self) -> String {
        let words = self.words();
        let article = if words.starts_with(['a', 'e', 'i', 'o', 'u']) {
            "an"
        } else {
            "a"
        };
        format!("{article} {words}")
    }

    pub(super) fn all_words() -> String {
        let mut words: Vec<&str> = KINDS.iter().map(|(name, _)| *name).collect();
        words.extend(["input member", "output member"]);
        words.join(", ")
    }

    pub(super) fn is_member(self) -> bool {
        matches!(self, Kind::Member | Kind::InputMember | Kind::OutputMember)
    }

    /// A kind of named shape (not the service, a resource, an operation or a
    /// member).
    pub(super) fn is_shape(self) -> bool {
        !matches!(
            self,
            Kind::Service
                | Kind::Resource
                | Kind::Operation
                | Kind::Member
                | Kind::InputMember
                | Kind::OutputMember
        )
    }

    /// Kinds whose values can carry `length`, `range` or `pattern`.
    fn is_constrained(self) -> bool {
        self.is_member()
            || matches!(
                self,
                Kind::Shape
                    | Kind::String
                    | Kind::Number
                    | Kind::Byte
                    | Kind::Short
                    | Kind::Integer
                    | Kind::Long
                    | Kind::Float
                    | Kind::Double
                    | Kind::BigInteger
                    | Kind::BigDecimal
                    | Kind::List
                    | Kind::Map
                    | Kind::Blob
            )
    }
}

/// What a path yields, which decides the operators that apply to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PathType {
    Str,
    Num,
    /// A metadata key with an enumerated value.
    Enum(Vec<String>),
    /// A metadata key with a boolean value.
    Bool,
    /// An operation's `input` or `output`, which can `contain sensitive`.
    Body,
    /// `type`, which can also be tested with `is`.
    Type,
}

pub(super) const FLAGS: &[&str] = &[
    "documented",
    "internal",
    "sensitive",
    "readonly",
    "idempotent",
    "public",
    "paginated",
    "enum",
    "referenced",
    "optional",
    "required",
    "template",
];

/// Whether the bare flag `name` can be read on `kind`.
pub(super) fn flag_applies(name: &str, kind: Kind) -> bool {
    match name {
        "documented" => true,
        "internal" | "sensitive" => kind != Kind::Resource && kind != Kind::Service,
        "readonly" | "idempotent" | "public" | "paginated" => kind == Kind::Operation,
        "enum" | "referenced" => kind.is_shape(),
        "optional" | "required" | "template" => kind.is_member(),
        _ => false,
    }
}

/// The type of a built-in path (`name`, `http.uri`, `type.length.max`) on
/// `kind`, or why it does not apply.
pub(super) fn path_type(head: &str, rest: &[&str], kind: Kind) -> Result<PathType, String> {
    let full = std::iter::once(head)
        .chain(rest.iter().copied())
        .collect::<Vec<_>>()
        .join(".");
    let wrong_kind = |wanted: &str| format!("`{full}` reads {wanted}, not {}", kind.indefinite());
    match (head, rest) {
        ("name", []) => Ok(PathType::Str),
        ("role", []) if kind == Kind::Operation => Ok(PathType::Str),
        ("role", []) => Err(wrong_kind("an operation's role")),
        ("http", ["method" | "uri"]) if kind == Kind::Operation => Ok(PathType::Str),
        ("http", ["status"]) if kind == Kind::Operation => Ok(PathType::Num),
        ("http", _) if kind == Kind::Operation => {
            Err("`http` has `method`, `uri` and `status`".to_owned())
        }
        ("http", _) => Err(wrong_kind("an operation's HTTP binding")),
        ("input" | "output", []) if kind == Kind::Operation => Ok(PathType::Body),
        ("input" | "output", _) => Err(wrong_kind("an operation's body")),
        ("type", []) if kind.is_member() => Ok(PathType::Type),
        ("type", ["length" | "range", "min" | "max"]) if kind.is_member() => Ok(PathType::Num),
        ("type", _) if kind.is_member() => Err(
            "`type` can be compared, tested with `is`, or read as `type.length.min`, `type.length.max`, `type.range.min` or `type.range.max`".to_owned(),
        ),
        ("type", _) => Err(wrong_kind("a member's target")),
        ("length" | "range", ["min" | "max"]) if kind.is_constrained() => Ok(PathType::Num),
        ("length" | "range", _) if kind.is_constrained() => {
            Err(format!("`{head}` has `min` and `max`"))
        }
        ("length" | "range", _) => Err(wrong_kind(
            "a constraint of a string, number, list, map or blob",
        )),
        ("pattern", []) if kind.is_constrained() => Ok(PathType::Str),
        ("pattern", _) => Err(wrong_kind("a constraint of a string")),
        _ => Err(format!(
            "unknown condition `{full}`; for {} the flags are {}, and the values are {}",
            kind.indefinite(),
            flags_for(kind),
            paths_for(kind)
        )),
    }
}

/// The built-in annotations `has @name` tests: those the IR carries as typed
/// fields. A declared `metadata` key of the same name cannot exist (the
/// compiler refuses it).
pub(super) const BUILT_IN: &[&str] = &["token", "query", "header", "payload", "deprecated"];

/// Whether the built-in annotation `name` can be tested on `kind`.
pub(super) fn built_in_applies(name: &str, kind: Kind) -> bool {
    match name {
        "token" | "query" | "header" | "payload" => kind.is_member(),
        // Resources take no annotations, so none can be deprecated.
        "deprecated" => kind != Kind::Resource,
        _ => false,
    }
}

/// `has length`, `has range` and `has pattern`: whether `kind` can carry it.
pub(super) fn has_applies(name: &str, kind: Kind) -> bool {
    matches!(name, "length" | "range" | "pattern") && kind.is_constrained()
}

/// Whether `has member` (unscoped) can read the members of `kind`: a
/// structure, union or error, or a resource's identifiers and properties.
pub(super) fn has_member_applies(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Structure | Kind::Union | Kind::Error | Kind::Resource
    )
}

pub(super) fn flags_for(kind: Kind) -> String {
    let names: Vec<&str> = FLAGS
        .iter()
        .copied()
        .filter(|flag| flag_applies(flag, kind))
        .collect();
    names.join(", ")
}

fn paths_for(kind: Kind) -> String {
    let mut names = vec!["name"];
    if kind == Kind::Operation {
        names.extend([
            "role",
            "http.method",
            "http.uri",
            "http.status",
            "input",
            "output",
        ]);
    }
    if kind.is_member() {
        names.extend(["type", "type.length.max", "type.range.max"]);
    }
    if kind.is_constrained() && !kind.is_member() {
        names.extend(["length.max", "range.max", "pattern"]);
    }
    names.push("@metadata");
    names.join(", ")
}

/// The words of `type is ...` and what each tests.
pub(super) fn type_is(words: &[&str]) -> Option<TypeIs> {
    match words {
        ["inline", "primitive"] => Some(TypeIs::InlinePrimitive),
        other => Kind::from_words(other)
            .filter(|kind| kind.is_shape())
            .map(TypeIs::Kind),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TypeIs {
    /// The member's target is a built-in type, such as `string`, not a named one.
    InlinePrimitive,
    Kind(Kind),
}
