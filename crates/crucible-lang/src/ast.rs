//! The typed syntax tree. It records what was written, with spans; names are
//! not resolved and meaning is not checked here. Missing pieces after an
//! error are empty identifiers, so later phases always see a whole tree.

use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct File {
    pub items: Vec<Item>,
}

/// A top-level declaration with its documentation, annotations and modifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub doc: Option<Doc>,
    pub annotations: Vec<Annotation>,
    pub modifiers: Vec<Ident>,
    pub decl: Decl,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    /// The Markdown text: `///` and one following space removed per line.
    pub text: String,
    pub span: Span,
    /// Where each line of `text` starts in the source, so a position in
    /// the text (a doc link's) maps back to the file.
    pub line_starts: Vec<u32>,
}

impl Doc {
    /// The source span of `range`, a byte range of [`Doc::text`].
    pub fn source_span(&self, range: std::ops::Range<usize>) -> Span {
        let source = |offset: usize| {
            let mut start = 0;
            for (line, text) in self.text.split('\n').enumerate() {
                let end = start + text.len();
                if offset <= end {
                    let line_start = self
                        .line_starts
                        .get(line)
                        .copied()
                        .unwrap_or(self.span.start);
                    return line_start as usize + (offset - start);
                }
                start = end + 1;
            }
            self.span.end as usize
        };
        Span::new(source(range.start), source(range.end))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    /// Stands in for a name the source left out; a diagnostic was reported.
    pub fn missing(span: Span) -> Self {
        Self {
            name: String::new(),
            span,
        }
    }

    pub fn is_missing(&self) -> bool {
        self.name.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrLit {
    /// The unescaped value.
    pub value: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumLit {
    /// The number as written, so no precision is lost before lowering.
    pub text: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub name: Ident,
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub key: Option<Key>,
    pub value: Value,
    /// The key, if any, through the value.
    pub span: Span,
}

/// An argument or object key: `since:` or `"Home" =`.
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Name(Ident),
    Text(StrLit),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(StrLit),
    Number(NumLit),
    Ident(Ident),
    List(Vec<Value>, Span),
    Object(Vec<Field>, Span),
}

impl Value {
    pub fn span(&self) -> Span {
        match self {
            Value::Str(s) => s.span,
            Value::Number(n) => n.span,
            Value::Ident(i) => i.span,
            Value::List(_, span) | Value::Object(_, span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub key: Key,
    pub value: Value,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Service(Block),
    Protocol(Block),
    Resource(Resource),
    Operation(Operation),
    Template(Template),
    Structure(Structure),
    Union(Union),
    Error(ErrorDecl),
    Enum(EnumDecl),
    Scalar(Scalar),
    List(ListDecl),
    Map(MapDecl),
    Metadata(Metadata),
    Rule(Rule),
    Except(Except),
    Migration(Migration),
    Conversion(Conversion),
}

/// A named block of keyword clauses: `service` and `protocol`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub name: Ident,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub doc: Option<Doc>,
    pub name: Ident,
    pub value: ClauseValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClauseValue {
    /// `paginated(inputToken: nextToken)`
    Args(Vec<Arg>),
    /// `errors { header "X-Crucible-Error" }`
    Block(Vec<Clause>),
    /// `version "2026-10-01"`, `auth bearer`, `operations [Ping]`
    Value(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resource {
    pub name: Ident,
    /// `in Organization`
    pub parent: Option<Ident>,
    pub entries: Vec<ResourceEntry>,
    /// `permission mailSend = "mail:send"`, in source order.
    pub permissions: Vec<PermissionEntry>,
}

/// `[internal] permission symbol = "value"` in a resource
/// (docs/permissions.md §4.1).
#[derive(Debug, Clone, PartialEq)]
pub struct PermissionEntry {
    pub doc: Option<Doc>,
    /// `@permissionOnly`, `@notGrantable`, `@aliasOf(p)`, `@deprecated`, `@since`.
    pub annotations: Vec<Annotation>,
    /// `internal`.
    pub modifiers: Vec<Ident>,
    pub name: Ident,
    pub value: StrLit,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceEntryKind {
    Id,
    Property,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResourceEntry {
    pub doc: Option<Doc>,
    pub annotations: Vec<Annotation>,
    /// `readonly`, `immutable`, `writeonly` (decision 34a). Lowering checks
    /// which apply and which combine.
    pub modifiers: Vec<Ident>,
    pub kind: ResourceEntryKind,
    pub name: Ident,
    /// `property lastMessageAt?: DateTime` (decision 34e).
    pub optional: bool,
    pub ty: TypeRef,
    /// `id inboxId: InboxId as id`: the identifier's name in the resource's
    /// own views and output elisions (decision 34f).
    pub alias: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    pub name: Ident,
    pub attachment: Option<Attachment>,
    pub entries: Vec<OpEntry>,
}

/// `reads Graph`, `on Graph collection`.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// Any word: lowering reports one that is not a role.
    pub role: Ident,
    pub resource: Ident,
    pub collection: bool,
    pub span: Span,
}

/// A role template: what every operation of a role carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    pub role: Ident,
    pub entries: Vec<OpEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OpEntry {
    Http(Http),
    /// `paginated(items: graphs)`
    Call(Call),
    Body(Body),
    Errors(Vec<Ident>, Span),
    Example(Example),
    /// `requires ..` or `filters ..` (docs/permissions.md §4.2).
    Requirement(Requirement),
}

/// One authorization clause of an operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Requirement {
    /// Says why, or when a handler requirement applies.
    pub doc: Option<Doc>,
    pub kind: RequirementKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequirementKind {
    /// `requires none`; the span covers `none`.
    None(Span),
    /// `requires pending because "reason"`.
    Pending(StrLit),
    /// `requires p [on Target] [when cond] [by handler]`.
    Requires {
        permission: Ident,
        target: Option<Target>,
        when: Option<Condition>,
        /// `by handler`, its span.
        handler: Option<Span>,
    },
    /// `filters p on Resource`.
    Filters { permission: Ident, resource: Ident },
}

/// `Template(content.template.templateId)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub resource: Ident,
    /// `None` without parentheses: every identifier binds by name.
    pub bindings: Option<Vec<BindingArg>>,
    pub span: Span,
}

/// `[identifier:] (* | $label | member.path)`.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingArg {
    pub name: Option<Ident>,
    pub value: BindingValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BindingValue {
    /// `*`: every instance.
    All(Span),
    /// `$inboxId`: the input member that binds that identifier.
    Label(Ident),
    /// `content.template.templateId`
    Path(Dotted),
}

/// A `when` condition: the boolean subset of the rule grammar.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub kind: ConditionKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConditionKind {
    Has(Dotted),
    Equals(Dotted, Value),
    In(Dotted, Value),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Http {
    pub method: Ident,
    pub uri: StrLit,
    pub status: Option<NumLit>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub name: Ident,
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BodyKind {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Body {
    /// Documents the generated input or output structure.
    pub doc: Option<Doc>,
    /// `strict`
    pub modifiers: Vec<Ident>,
    pub kind: BodyKind,
    pub members: Vec<Member>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    pub doc: Option<Doc>,
    pub title: StrLit,
    pub parts: Vec<ExamplePart>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExamplePart {
    Input(Value),
    Output(Value),
    Error(Ident, Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub doc: Option<Doc>,
    pub annotations: Vec<Annotation>,
    /// `internal`, `sensitive` (decision 19).
    pub modifiers: Vec<Ident>,
    pub kind: MemberKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemberKind {
    /// `include RequestIdMixin`
    Include(Ident),
    /// `$graphId as id?`
    Elided {
        name: Ident,
        alias: Option<Ident>,
        optional: bool,
    },
    /// `$*` in a `for` view: the resource's own identifiers and readable
    /// properties (decision 34d). The span covers `$*`.
    All(Span),
    /// `name?: Type { constraints } = default`. A union arm may have no type.
    Field {
        name: Ident,
        optional: bool,
        ty: Option<TypeRef>,
        default: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeRef {
    pub name: Ident,
    pub constraints: Vec<Constraint>,
    /// The name through the constraint block, if any.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Constraint {
    pub name: Ident,
    pub value: ConstraintValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstraintValue {
    /// `unique`
    Flag,
    /// `length 1..100`, `range ..5`: both ends inclusive.
    Range {
        min: Option<NumLit>,
        max: Option<NumLit>,
    },
    Value(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub name: Ident,
    /// `for Graph`: a view of a resource. With several (`for QueryJob,
    /// GraphHistoryQueryJob`) this is the first, which the view is lowered
    /// against.
    pub resource: Option<Ident>,
    /// The other resources of `for A, B`. Each must declare, identically,
    /// everything the view shows.
    pub also: Vec<Ident>,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Union {
    pub name: Ident,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorDecl {
    pub name: Ident,
    pub entries: Vec<ErrorEntry>,
}

// Members are the usual entry, so boxing them would allocate for nearly
// every entry to save space on the rare `status` and `retryable`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorEntry {
    Status(NumLit),
    Retryable { throttling: bool, span: Span },
    Member(Member),
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: Ident,
    /// `: integer`
    pub base: Option<Ident>,
    pub values: Vec<EnumValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumValue {
    pub doc: Option<Doc>,
    pub annotations: Vec<Annotation>,
    /// `internal` (decision 19).
    pub modifiers: Vec<Ident>,
    pub name: Ident,
    pub value: Option<Value>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    /// `string`, `integer`, `timestamp`, ...
    pub kind: Ident,
    pub name: Ident,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListDecl {
    pub name: Ident,
    pub member: Ident,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapDecl {
    pub name: Ident,
    pub key: Ident,
    pub value: Ident,
    pub constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub name: Ident,
    /// Each target is one or two words: `operation`, `enum value`.
    pub targets: Vec<Vec<Ident>>,
    pub ty: Option<MetaType>,
    pub conflicts: Vec<Ident>,
    /// `retention source`: checked by the compiler and rules, never written
    /// to the IR. `runtime`, the default, reaches generated code.
    pub retention: Option<Ident>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetaType {
    Enum(Vec<EnumValue>),
    Type(TypeRef),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub name: Ident,
    /// `strict`, `exceptable`, `advice`
    pub flags: Vec<Ident>,
    pub clauses: Vec<RuleClause>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantifier {
    Every,
    No,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleClause {
    pub quantifier: Quantifier,
    /// `operation`, `input member`
    pub kind: Vec<Ident>,
    pub condition: Option<Expr>,
    pub requirements: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    /// `has one of @a, @b`
    HasOneOf(Vec<Ref>),
    /// `has length`, `has @operationClass`
    Has(Ref),
    /// `addresses Organization`
    Addresses(Ident),
    /// A bare path is a flag: `readonly`, `paginated`.
    Flag(Path),
    Compare {
        path: Path,
        op: Operator,
        operand: Operand,
    },
}

/// `name` or `@metadataKey`.
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    pub metadata: bool,
    pub name: Ident,
}

/// `type.length.max`, `@operationClass`.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    pub head: Ref,
    pub rest: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    Is,
    StartsWith,
    EndsWith,
    Matches,
    Contains,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Value(Value),
    /// The words after `is`: `is inline primitive`, `is string`.
    Words(Vec<Ident>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Except {
    pub rule: Ident,
    pub target: Ident,
    pub reason: StrLit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Migration {
    pub version: StrLit,
    pub from: StrLit,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub doc: Option<Doc>,
    pub kind: StepKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepKind {
    Rename {
        path: Dotted,
        to: Ident,
    },
    Require {
        path: Dotted,
        how: RequireHow,
    },
    Add {
        path: Dotted,
        name: Option<Ident>,
        downgrade: Ident,
        mapping: Vec<(Ident, Ident)>,
    },
    Move {
        operation: Ident,
        method: Ident,
        uri: StrLit,
    },
    Reshape {
        path: Dotted,
        members: Vec<Ident>,
        into: Ident,
        hook: Hook,
    },
    RemoveOperation(Ident),
    Behavior(Dotted),
    Sunset(StrLit),
    Retype {
        path: Dotted,
        to: Ident,
        hook: Option<Hook>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequireHow {
    Default(Value),
    Hook(Hook),
}

/// `by hook` or `by async hook`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hook {
    pub is_async: bool,
    pub span: Span,
}

/// `CreateGraph.input.homeRegion`
#[derive(Debug, Clone, PartialEq)]
pub struct Dotted {
    pub parts: Vec<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conversion {
    pub from: Ident,
    pub to: Ident,
    pub hook: Hook,
}
