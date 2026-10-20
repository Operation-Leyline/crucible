//! What the editor shows that the compiler does not compute: the role of every
//! token for highlighting, the declarations a name refers to, and the outline.
//! Keywords are contextual identifiers in this language, so a token's role
//! comes from the syntax tree, not from a word list.

use std::collections::{BTreeMap, HashMap};

use crucible_lang::ast::{
    Annotation, Arg, BindingValue, Body, BodyKind, Clause, ClauseValue, Condition, ConditionKind,
    ConstraintValue, Decl, Dotted, ErrorEntry, ExamplePart, Expr, ExprKind, Ident, Item, Key,
    Member, MemberKind, MetaType, OpEntry, Operand, Path, Ref, Requirement, RequirementKind,
    StepKind, TypeRef, Value,
};
use crucible_lang::{Parsed, TokenKind};

/// Token types, in legend order; [`Role`] indexes into this.
pub const TOKEN_TYPES: &[&str] = &[
    "keyword",
    "type",
    "struct",
    "enum",
    "class",
    "interface",
    "function",
    "property",
    "enumMember",
    "decorator",
    "parameter",
    "string",
    "number",
    "comment",
];

/// Token modifiers, in legend order: bit `1 << index`. After the standard
/// three come the declaration kinds the standard types cannot tell apart: a
/// service and a protocol are both interfaces, a resource and an error both
/// classes. A kind marks the declaration and every reference to it, so an
/// editor can color errors as errors wherever they are named. Clients that
/// do not know a modifier ignore it and still color by type.
pub const TOKEN_MODIFIERS: &[&str] = &[
    "declaration",
    "documentation",
    "defaultLibrary",
    "service",
    "protocol",
    "resource",
    "error",
    "union",
    "rule",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Keyword,
    Type,
    Struct,
    Enum,
    Class,
    Interface,
    Function,
    Property,
    EnumMember,
    Decorator,
    Parameter,
    String,
    Number,
    Comment,
}

pub const DECLARATION: u32 = 1;
pub const DOCUMENTATION: u32 = 2;
pub const DEFAULT_LIBRARY: u32 = 4;
pub const SERVICE: u32 = 8;
pub const PROTOCOL: u32 = 16;
pub const RESOURCE: u32 = 32;
pub const ERROR: u32 = 64;
pub const UNION: u32 = 128;
pub const RULE: u32 = 256;

/// The kind modifier of a declaration, by its keyword; 0 when the token
/// type already says what it is.
pub fn kind_modifier(keyword: &str) -> u32 {
    match keyword {
        "service" => SERVICE,
        "protocol" => PROTOCOL,
        "resource" => RESOURCE,
        "error" => ERROR,
        "union" => UNION,
        "rule" => RULE,
        _ => 0,
    }
}

/// One highlighted token: a byte range, its role and modifier bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub start: u32,
    pub end: u32,
    pub role: Role,
    pub modifiers: u32,
}

/// The built-in scalar types (`docs/idl/reference.md`).
pub const BUILTIN_TYPES: &[&str] = &[
    "string",
    "boolean",
    "byte",
    "short",
    "integer",
    "long",
    "float",
    "double",
    "bigInteger",
    "bigDecimal",
    "timestamp",
    "blob",
    "document",
];

/// A named declaration somewhere in the package or its imports.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// The declaring keyword: `structure`, `operation`, `string`, ...
    pub keyword: String,
    pub role: Role,
    pub file: usize,
    pub name_start: u32,
    pub name_end: u32,
    pub doc: Option<String>,
}

/// Every declaration by name. Names are unique within a compiled package,
/// imports included; the compiler reports a clash.
#[derive(Debug, Clone, Default)]
pub struct Index {
    pub declarations: HashMap<String, Declaration>,
}

impl Index {
    pub fn new(parsed: &[Parsed]) -> Self {
        let mut declarations = HashMap::new();
        for (file, parsed) in parsed.iter().enumerate() {
            for item in &parsed.file.items {
                let Some((keyword, role, name)) = describe(&item.decl) else {
                    continue;
                };
                if name.is_missing() {
                    continue;
                }
                declarations
                    .entry(name.name.clone())
                    .or_insert_with(|| Declaration {
                        keyword,
                        role,
                        file,
                        name_start: name.span.start,
                        name_end: name.span.end,
                        doc: item.doc.as_ref().map(|doc| doc.text.clone()),
                    });
            }
        }
        Self { declarations }
    }

    /// How a reference to `name` is highlighted.
    fn reference(&self, name: &str) -> Option<(Role, u32)> {
        if let Some(declaration) = self.declarations.get(name) {
            return Some((declaration.role, kind_modifier(&declaration.keyword)));
        }
        BUILTIN_TYPES
            .contains(&name)
            .then_some((Role::Type, DEFAULT_LIBRARY))
    }
}

/// The keyword, highlight role and name of a named declaration.
pub fn describe(decl: &Decl) -> Option<(String, Role, &Ident)> {
    Some(match decl {
        Decl::Service(block) => ("service".into(), Role::Interface, &block.name),
        Decl::Protocol(block) => ("protocol".into(), Role::Interface, &block.name),
        Decl::Resource(resource) => ("resource".into(), Role::Class, &resource.name),
        Decl::Operation(operation) => ("operation".into(), Role::Function, &operation.name),
        Decl::Structure(structure) => ("structure".into(), Role::Struct, &structure.name),
        Decl::Union(union) => ("union".into(), Role::Struct, &union.name),
        Decl::Error(error) => ("error".into(), Role::Class, &error.name),
        Decl::Enum(enumeration) => ("enum".into(), Role::Enum, &enumeration.name),
        Decl::Scalar(scalar) => (scalar.kind.name.clone(), Role::Type, &scalar.name),
        Decl::List(list) => ("list".into(), Role::Type, &list.name),
        Decl::Map(map) => ("map".into(), Role::Type, &map.name),
        Decl::Metadata(metadata) => ("metadata".into(), Role::Decorator, &metadata.name),
        Decl::Rule(rule) => ("rule".into(), Role::Function, &rule.name),
        Decl::Template(_) | Decl::Except(_) | Decl::Migration(_) | Decl::Conversion(_) => {
            return None;
        }
    })
}

/// The highlight of every token in one file, in source order.
pub fn highlights(parsed: &Parsed, index: &Index) -> Vec<Highlight> {
    let mut walker = Walker {
        index,
        roles: BTreeMap::new(),
    };
    for item in &parsed.file.items {
        walker.item(item);
    }
    let mut out = Vec::new();
    for token in &parsed.tokens {
        let (start, end) = (token.span.start, token.span.end);
        let (role, modifiers) = match token.kind {
            TokenKind::LineComment => (Role::Comment, 0),
            TokenKind::DocComment => (Role::Comment, DOCUMENTATION),
            TokenKind::String => (Role::String, 0),
            TokenKind::Number => (Role::Number, 0),
            TokenKind::At => (Role::Decorator, 0),
            // A name the tree did not classify is a keyword: the declaration
            // keywords, modifiers, clause and constraint names, roles, HTTP
            // methods and the words of rules and migrations.
            TokenKind::Ident => walker
                .roles
                .get(&start)
                .copied()
                .unwrap_or((Role::Keyword, 0)),
            _ => continue,
        };
        out.push(Highlight {
            start,
            end,
            role,
            modifiers,
        });
    }
    out
}

struct Walker<'a> {
    index: &'a Index,
    roles: BTreeMap<u32, (Role, u32)>,
}

impl Walker<'_> {
    fn set(&mut self, ident: &Ident, role: Role, modifiers: u32) {
        if !ident.is_missing() {
            self.roles.insert(ident.span.start, (role, modifiers));
        }
    }

    fn declare(&mut self, ident: &Ident, role: Role) {
        self.set(ident, role, DECLARATION);
    }

    /// A name that refers to a declaration or a built-in type; anything
    /// else gets `fallback`, if any.
    fn reference(&mut self, ident: &Ident, fallback: Option<Role>) {
        self.reference_kind(ident, fallback, 0);
    }

    /// [`Self::reference`] where the position says what kind of declaration
    /// the name must be: an unresolved name in `errors [..]` is still shown
    /// as an error.
    fn reference_kind(&mut self, ident: &Ident, fallback: Option<Role>, kind: u32) {
        match self.index.reference(&ident.name) {
            Some((role, modifiers)) => self.set(ident, role, modifiers),
            None => {
                if let Some(role) = fallback {
                    self.set(ident, role, kind);
                }
            }
        }
    }

    fn type_ref(&mut self, ty: &TypeRef) {
        self.reference(&ty.name, Some(Role::Type));
        self.constraints(&ty.constraints);
    }

    fn constraints(&mut self, constraints: &[crucible_lang::ast::Constraint]) {
        for constraint in constraints {
            if let ConstraintValue::Value(value) = &constraint.value {
                self.value(value, Role::EnumMember);
            }
        }
    }

    fn annotations(&mut self, annotations: &[Annotation]) {
        for annotation in annotations {
            self.set(&annotation.name, Role::Decorator, 0);
            self.args(&annotation.args, Role::EnumMember);
        }
    }

    /// `ident` is how an identifier value is shown when it names no type: a
    /// member for `paginated(items: graphs)`, a constant for `@mutability(create)`.
    fn args(&mut self, args: &[Arg], ident: Role) {
        for arg in args {
            if let Some(Key::Name(key)) = &arg.key {
                self.set(key, Role::Parameter, 0);
            }
            self.value(&arg.value, ident);
        }
    }

    fn value(&mut self, value: &Value, ident: Role) {
        match value {
            Value::Ident(name) => self.reference(name, Some(ident)),
            Value::List(items, _) => {
                for item in items {
                    self.value(item, ident);
                }
            }
            Value::Object(fields, _) => {
                for field in fields {
                    if let Key::Name(key) = &field.key {
                        self.set(key, Role::Property, 0);
                    }
                    self.value(&field.value, ident);
                }
            }
            Value::Str(_) | Value::Number(_) => {}
        }
    }

    fn clauses(&mut self, clauses: &[Clause]) {
        for clause in clauses {
            match &clause.value {
                ClauseValue::Args(args) => self.args(args, Role::Property),
                ClauseValue::Block(inner) => self.clauses(inner),
                ClauseValue::Value(value) => self.value(value, Role::EnumMember),
            }
        }
    }

    fn members(&mut self, members: &[Member]) {
        for member in members {
            self.member(member);
        }
    }

    fn member(&mut self, member: &Member) {
        self.annotations(&member.annotations);
        match &member.kind {
            MemberKind::Include(name) => self.reference(name, Some(Role::Struct)),
            MemberKind::All(_) => {}
            MemberKind::Elided { name, alias, .. } => {
                self.set(name, Role::Property, 0);
                if let Some(alias) = alias {
                    self.declare(alias, Role::Property);
                }
            }
            MemberKind::Field {
                name, ty, default, ..
            } => {
                self.declare(name, Role::Property);
                if let Some(ty) = ty {
                    self.type_ref(ty);
                }
                if let Some(default) = default {
                    self.value(default, Role::EnumMember);
                }
            }
        }
    }

    fn entries(&mut self, entries: &[OpEntry]) {
        for entry in entries {
            match entry {
                OpEntry::Http(_) => {}
                OpEntry::Call(call) => self.args(&call.args, Role::Property),
                OpEntry::Body(Body { members, .. }) => self.members(members),
                OpEntry::Errors(names, _) => {
                    for name in names {
                        self.reference_kind(name, Some(Role::Class), ERROR);
                    }
                }
                OpEntry::Example(example) => {
                    for part in &example.parts {
                        match part {
                            ExamplePart::Input(value) | ExamplePart::Output(value) => {
                                self.value(value, Role::EnumMember);
                            }
                            ExamplePart::Error(name, value) => {
                                self.reference_kind(name, Some(Role::Class), ERROR);
                                self.value(value, Role::EnumMember);
                            }
                        }
                    }
                }
                OpEntry::Requirement(requirement) => self.requirement(requirement),
            }
        }
    }

    /// `requires` and `filters`: the permission is a resource's entry, the
    /// target a resource, bindings and condition paths input members; the
    /// remaining words (`on`, `when`, `has`, `by handler`) are keywords.
    fn requirement(&mut self, requirement: &Requirement) {
        match &requirement.kind {
            RequirementKind::None(_) | RequirementKind::Pending(_) => {}
            RequirementKind::Filters {
                permission,
                resource,
            } => {
                self.set(permission, Role::EnumMember, 0);
                self.reference_kind(resource, Some(Role::Class), RESOURCE);
            }
            RequirementKind::Requires {
                permission,
                target,
                when,
                ..
            } => {
                self.set(permission, Role::EnumMember, 0);
                if let Some(target) = target {
                    self.reference_kind(&target.resource, Some(Role::Class), RESOURCE);
                    for binding in target.bindings.iter().flatten() {
                        if let Some(name) = &binding.name {
                            self.set(name, Role::Property, 0);
                        }
                        match &binding.value {
                            BindingValue::All(_) => {}
                            BindingValue::Label(label) => self.set(label, Role::Property, 0),
                            BindingValue::Path(path) => {
                                for part in &path.parts {
                                    self.set(part, Role::Property, 0);
                                }
                            }
                        }
                    }
                }
                if let Some(when) = when {
                    self.condition(when);
                }
            }
        }
    }

    fn condition(&mut self, condition: &Condition) {
        match &condition.kind {
            ConditionKind::Has(path) => {
                for part in &path.parts {
                    self.set(part, Role::Property, 0);
                }
            }
            ConditionKind::Equals(path, value) | ConditionKind::In(path, value) => {
                for part in &path.parts {
                    self.set(part, Role::Property, 0);
                }
                self.value(value, Role::EnumMember);
            }
            ConditionKind::And(parts) | ConditionKind::Or(parts) => {
                for part in parts {
                    self.condition(part);
                }
            }
            ConditionKind::Not(inner) => self.condition(inner),
        }
    }

    fn item(&mut self, item: &Item) {
        self.annotations(&item.annotations);
        if let Some((keyword, role, name)) = describe(&item.decl) {
            self.set(name, role, DECLARATION | kind_modifier(&keyword));
        }
        match &item.decl {
            Decl::Service(block) | Decl::Protocol(block) => self.clauses(&block.clauses),
            Decl::Resource(resource) => {
                if let Some(parent) = &resource.parent {
                    self.reference_kind(parent, Some(Role::Class), RESOURCE);
                }
                for entry in &resource.entries {
                    self.annotations(&entry.annotations);
                    self.declare(&entry.name, Role::Property);
                    self.type_ref(&entry.ty);
                    if let Some(alias) = &entry.alias {
                        self.declare(alias, Role::Property);
                    }
                }
                for permission in &resource.permissions {
                    self.annotations(&permission.annotations);
                    self.declare(&permission.name, Role::EnumMember);
                }
            }
            Decl::Operation(operation) => {
                if let Some(attachment) = &operation.attachment {
                    self.reference_kind(&attachment.resource, Some(Role::Class), RESOURCE);
                }
                self.entries(&operation.entries);
            }
            Decl::Template(template) => self.entries(&template.entries),
            Decl::Structure(structure) => {
                for resource in structure.resource.iter().chain(&structure.also) {
                    self.reference_kind(resource, Some(Role::Class), RESOURCE);
                }
                self.members(&structure.members);
            }
            Decl::Union(union) => self.members(&union.members),
            Decl::Error(error) => {
                for entry in &error.entries {
                    if let ErrorEntry::Member(member) = entry {
                        self.member(member);
                    }
                }
            }
            Decl::Enum(enumeration) => {
                if let Some(base) = &enumeration.base {
                    self.reference(base, Some(Role::Type));
                }
                self.enum_values(&enumeration.values);
            }
            Decl::Scalar(scalar) => {
                self.set(&scalar.kind, Role::Type, DEFAULT_LIBRARY);
                self.constraints(&scalar.constraints);
            }
            Decl::List(list) => {
                self.reference(&list.member, Some(Role::Type));
                self.constraints(&list.constraints);
            }
            Decl::Map(map) => {
                self.reference(&map.key, Some(Role::Type));
                self.reference(&map.value, Some(Role::Type));
                self.constraints(&map.constraints);
            }
            Decl::Metadata(metadata) => {
                match &metadata.ty {
                    Some(MetaType::Enum(values)) => self.enum_values(values),
                    Some(MetaType::Type(ty)) => self.type_ref(ty),
                    None => {}
                }
                for conflict in &metadata.conflicts {
                    self.set(conflict, Role::Decorator, 0);
                }
            }
            Decl::Rule(rule) => {
                for clause in &rule.clauses {
                    if let Some(condition) = &clause.condition {
                        self.expr(condition);
                    }
                    for requirement in &clause.requirements {
                        self.expr(requirement);
                    }
                }
            }
            Decl::Except(except) => {
                self.reference_kind(&except.rule, Some(Role::Function), RULE);
                self.reference(&except.target, None);
            }
            Decl::Migration(migration) => {
                for step in &migration.steps {
                    self.step(&step.kind);
                }
            }
            Decl::Conversion(conversion) => {
                self.reference(&conversion.from, Some(Role::Type));
                self.reference(&conversion.to, Some(Role::Type));
            }
        }
    }

    fn enum_values(&mut self, values: &[crucible_lang::ast::EnumValue]) {
        for value in values {
            self.annotations(&value.annotations);
            self.declare(&value.name, Role::EnumMember);
            if let Some(inner) = &value.value {
                self.value(inner, Role::EnumMember);
            }
        }
    }

    fn metadata_ref(&mut self, reference: &Ref) {
        if reference.metadata {
            self.set(&reference.name, Role::Decorator, 0);
        }
    }

    fn path(&mut self, path: &Path) {
        self.metadata_ref(&path.head);
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Or(left, right) | ExprKind::And(left, right) => {
                self.expr(left);
                self.expr(right);
            }
            ExprKind::Not(inner) => self.expr(inner),
            ExprKind::HasOneOf(refs) => {
                for reference in refs {
                    self.metadata_ref(reference);
                }
            }
            ExprKind::Has(reference) => self.metadata_ref(reference),
            ExprKind::HasMember { .. } => {}
            ExprKind::Addresses(name) => {
                self.reference_kind(name, Some(Role::Class), RESOURCE);
            }
            ExprKind::Flag(path) => self.path(path),
            ExprKind::Compare { path, operand, .. } => {
                self.path(path);
                if let Operand::Value(value) = operand {
                    self.value(value, Role::EnumMember);
                }
            }
        }
    }

    fn dotted(&mut self, dotted: &Dotted) {
        if let Some((head, rest)) = dotted.parts.split_first() {
            self.reference(head, None);
            for part in rest {
                if !matches!(part.name.as_str(), "input" | "output") {
                    self.set(part, Role::Property, 0);
                }
            }
        }
    }

    fn step(&mut self, step: &StepKind) {
        match step {
            StepKind::Rename { path, to } => {
                self.dotted(path);
                self.set(to, Role::Property, 0);
            }
            StepKind::Require { path, how } => {
                self.dotted(path);
                if let crucible_lang::ast::RequireHow::Default(value) = how {
                    self.value(value, Role::EnumMember);
                }
            }
            StepKind::Add {
                path,
                name,
                mapping,
                ..
            } => {
                self.dotted(path);
                if let Some(name) = name {
                    self.set(name, Role::Property, 0);
                }
                for (from, to) in mapping {
                    self.set(from, Role::EnumMember, 0);
                    self.set(to, Role::EnumMember, 0);
                }
            }
            StepKind::Move { operation, .. } | StepKind::RemoveOperation(operation) => {
                self.reference(operation, Some(Role::Function));
            }
            StepKind::Reshape {
                path,
                members,
                into,
                ..
            } => {
                self.dotted(path);
                for member in members {
                    self.set(member, Role::Property, 0);
                }
                self.set(into, Role::Property, 0);
            }
            StepKind::Behavior(path) => self.dotted(path),
            StepKind::Sunset(_) => {}
            StepKind::Retype { path, to, .. } => {
                self.dotted(path);
                self.reference(to, Some(Role::Type));
            }
        }
    }
}

/// Every place `name` refers to its declaration in one file, the declaration
/// itself included: `(start, end, is_declaration)`. Words that merely spell
/// the name, a member called the same say, are not references; the name in
/// a `[Name]` or `[Name.member]` doc link is one (decision 80).
pub fn occurrences(
    parsed: &Parsed,
    text: &str,
    index: &Index,
    name: &str,
) -> Vec<(u32, u32, bool)> {
    let mut found: Vec<(u32, u32, bool)> = highlights(parsed, index)
        .into_iter()
        .filter(|highlight| {
            names_declaration(highlight)
                && &text[highlight.start as usize..highlight.end as usize] == name
        })
        .map(|highlight| {
            (
                highlight.start,
                highlight.end,
                highlight.modifiers & DECLARATION != 0,
            )
        })
        .collect();
    if index.declarations.contains_key(name) {
        found.extend(
            doc_links(parsed, text)
                .into_iter()
                .filter(|link| link.name == name)
                .map(|link| (link.start, link.name_end(), false)),
        );
        found.sort_by_key(|(start, _, _)| *start);
    }
    found
}

/// The label of a `[Name]` or `[Name.member]` doc link in a `///` comment,
/// by source offsets: `start..end` covers the whole label, the name first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocLinkSite {
    pub name: String,
    /// The member or enum value after the `.`, with its source range.
    pub member: Option<(String, u32, u32)>,
    pub start: u32,
    pub end: u32,
    /// The whole link: for `[text][Name]` its text too, which navigates
    /// like the label; edits only ever touch the label.
    pub link_start: u32,
    pub link_end: u32,
}

impl DocLinkSite {
    /// Where the name part ends.
    pub fn name_end(&self) -> u32 {
        self.start + self.name.len() as u32
    }
}

/// One `///` line of a doc comment: its Markdown text, where that text
/// starts, and the token's span.
type DocLine<'a> = (&'a str, u32, crucible_lang::Span);

/// Every doc link in a file's doc comments, as the compiler finds them
/// ([`crucible_ir::doc_links::links`]): each run of `///` lines, `//`
/// comments between them skipped, is one Markdown text, as the parser
/// attaches it. Links in code, inline links and links the comment defines
/// are not doc links.
pub fn doc_links(parsed: &Parsed, text: &str) -> Vec<DocLinkSite> {
    fn flush(block: &mut Vec<DocLine<'_>>, out: &mut Vec<DocLinkSite>) {
        let (Some(first), Some(last)) = (block.first(), block.last()) else {
            return;
        };
        let doc = crucible_lang::ast::Doc {
            text: block
                .iter()
                .map(|(line, _, _)| *line)
                .collect::<Vec<_>>()
                .join("\n"),
            span: first.2.to(last.2),
            line_starts: block.iter().map(|(_, start, _)| *start).collect(),
        };
        block.clear();
        for link in crucible_ir::doc_links::links(&doc.text) {
            let span = doc.source_span(link.label_span);
            let (name, member) = crucible_ir::doc_links::split(&link.label);
            let member = member.map(|member| {
                let start = span.start + name.len() as u32 + 1;
                (member.to_owned(), start, span.end)
            });
            let whole = doc.source_span(link.span);
            out.push(DocLinkSite {
                name: name.to_owned(),
                member,
                start: span.start,
                end: span.end,
                link_start: whole.start,
                link_end: whole.end,
            });
        }
    }
    let mut out = Vec::new();
    let mut block: Vec<DocLine<'_>> = Vec::new();
    for token in &parsed.tokens {
        match token.kind {
            TokenKind::DocComment => {
                let whole = token.span.text(text);
                let line = whole.strip_prefix("///").unwrap_or(whole);
                let line = line.strip_prefix(' ').unwrap_or(line);
                let start = token.span.start + (whole.len() - line.len()) as u32;
                block.push((line.trim_end_matches('\r'), start, token.span));
            }
            TokenKind::Whitespace | TokenKind::LineComment => {}
            _ => flush(&mut block, &mut out),
        }
    }
    flush(&mut block, &mut out);
    out
}

/// The doc link whose label covers `offset`; an offset just past it counts,
/// as for [`ident_at`].
pub fn doc_link_at(parsed: &Parsed, text: &str, offset: usize) -> Option<DocLinkSite> {
    let offset = offset as u32;
    doc_links(parsed, text)
        .into_iter()
        .find(|link| link.link_start <= offset && offset <= link.link_end)
}

/// Where the member `member` of the declaration `name` is written, as a
/// doc link resolves it: a structure's, union's or error's member (one an
/// `include` brings, at the included structure), an enum value, or an
/// operation's input member, else its output member. `(file, start, end)`.
pub fn member_location(parsed: &[Parsed], name: &str, member: &str) -> Option<(usize, u32, u32)> {
    member_in(parsed, name, member, 0)
}

fn member_in(
    parsed: &[Parsed],
    name: &str,
    member: &str,
    depth: usize,
) -> Option<(usize, u32, u32)> {
    // An `include` cycle is the compiler's to report.
    if depth > 16 {
        return None;
    }
    let (file, decl) = parsed.iter().enumerate().find_map(|(file, parsed)| {
        parsed.file.items.iter().find_map(|item| {
            describe(&item.decl)
                .is_some_and(|(_, _, ident)| ident.name == name)
                .then_some((file, &item.decl))
        })
    })?;
    let in_members = |members: &mut dyn Iterator<Item = &Member>| -> Option<(usize, u32, u32)> {
        let mut includes = Vec::new();
        for m in members {
            let ident = match &m.kind {
                MemberKind::Field { name, .. } => name,
                MemberKind::Elided { name, alias, .. } => alias.as_ref().unwrap_or(name),
                MemberKind::Include(included) => {
                    includes.push(included.name.as_str());
                    continue;
                }
                MemberKind::All(_) => continue,
            };
            if ident.name == member {
                return Some((file, ident.span.start, ident.span.end));
            }
        }
        includes
            .into_iter()
            .find_map(|included| member_in(parsed, included, member, depth + 1))
    };
    match decl {
        Decl::Structure(s) => in_members(&mut s.members.iter()),
        Decl::Union(u) => in_members(&mut u.members.iter()),
        Decl::Error(e) => in_members(&mut e.entries.iter().filter_map(|entry| match entry {
            ErrorEntry::Member(m) => Some(m),
            _ => None,
        })),
        Decl::Enum(e) => e
            .values
            .iter()
            .find(|v| v.name.name == member)
            .map(|v| (file, v.name.span.start, v.name.span.end)),
        Decl::Operation(op) => [BodyKind::Input, BodyKind::Output]
            .into_iter()
            .filter_map(|kind| {
                op.entries.iter().find_map(|entry| match entry {
                    OpEntry::Body(body) if body.kind == kind => Some(body),
                    _ => None,
                })
            })
            .find_map(|body| in_members(&mut body.members.iter())),
        _ => None,
    }
}

/// Whether a highlighted name is a package declaration or a reference to
/// one, rather than a member, a keyword or a built-in.
fn names_declaration(highlight: &Highlight) -> bool {
    matches!(
        highlight.role,
        Role::Type
            | Role::Struct
            | Role::Enum
            | Role::Class
            | Role::Interface
            | Role::Function
            | Role::Decorator
    ) && highlight.modifiers & DEFAULT_LIBRARY == 0
}

/// How often each name is referred to in one file, declarations not
/// counted: one pass for every declaration's usage count.
pub fn reference_counts(parsed: &Parsed, text: &str, index: &Index) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for highlight in highlights(parsed, index) {
        if names_declaration(&highlight) && highlight.modifiers & DECLARATION == 0 {
            *out.entry(text[highlight.start as usize..highlight.end as usize].to_owned())
                .or_default() += 1;
        }
    }
    out
}

/// The identifier token covering `offset`, if any. An offset just past an
/// identifier counts, so a caret at a name's end still finds it.
pub fn ident_at(parsed: &Parsed, text: &str, offset: usize) -> Option<(u32, u32, String)> {
    let offset = offset as u32;
    parsed
        .tokens
        .iter()
        .filter(|token| token.kind == TokenKind::Ident)
        .find(|token| token.span.start <= offset && offset <= token.span.end)
        .map(|token| {
            (
                token.span.start,
                token.span.end,
                token.span.text(text).to_owned(),
            )
        })
}

/// One entry of the outline, with byte ranges.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub detail: String,
    pub kind: lsp_types::SymbolKind,
    pub start: u32,
    pub end: u32,
    pub name_start: u32,
    pub name_end: u32,
    pub children: Vec<Symbol>,
}

pub fn outline(parsed: &Parsed) -> Vec<Symbol> {
    use lsp_types::SymbolKind as K;
    let mut out = Vec::new();
    for item in &parsed.file.items {
        let Some((keyword, role, name)) = describe(&item.decl) else {
            continue;
        };
        if name.is_missing() {
            continue;
        }
        let kind = match role {
            Role::Interface => K::INTERFACE,
            Role::Class => K::CLASS,
            Role::Function => K::FUNCTION,
            Role::Struct => K::STRUCT,
            Role::Enum => K::ENUM,
            Role::Decorator => K::PROPERTY,
            _ => K::TYPE_PARAMETER,
        };
        let mut children = Vec::new();
        let members = |members: &[Member], into: &mut Vec<Symbol>| {
            for member in members {
                let (name, detail) = match &member.kind {
                    MemberKind::Field { name, ty, .. } => (
                        name,
                        ty.as_ref()
                            .map(|ty| ty.name.name.clone())
                            .unwrap_or_default(),
                    ),
                    MemberKind::Elided { name, alias, .. } => {
                        (alias.as_ref().unwrap_or(name), format!("${}", name.name))
                    }
                    MemberKind::Include(name) => (name, "include".to_owned()),
                    MemberKind::All(_) => continue,
                };
                if name.is_missing() {
                    continue;
                }
                into.push(Symbol {
                    name: name.name.clone(),
                    detail,
                    kind: K::FIELD,
                    start: member.span.start,
                    end: member.span.end,
                    name_start: name.span.start,
                    name_end: name.span.end,
                    children: Vec::new(),
                });
            }
        };
        match &item.decl {
            Decl::Structure(structure) => members(&structure.members, &mut children),
            Decl::Union(union) => members(&union.members, &mut children),
            Decl::Error(error) => {
                let list: Vec<Member> = error
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        ErrorEntry::Member(member) => Some(member.clone()),
                        _ => None,
                    })
                    .collect();
                members(&list, &mut children);
            }
            Decl::Operation(operation) => {
                for entry in &operation.entries {
                    if let OpEntry::Body(body) = entry {
                        let label = match body.kind {
                            crucible_lang::ast::BodyKind::Input => "input",
                            crucible_lang::ast::BodyKind::Output => "output",
                        };
                        let mut fields = Vec::new();
                        members(&body.members, &mut fields);
                        children.push(Symbol {
                            name: label.to_owned(),
                            detail: String::new(),
                            kind: K::OBJECT,
                            start: body.span.start,
                            end: body.span.end,
                            name_start: body.span.start,
                            name_end: body.span.start + label.len() as u32,
                            children: fields,
                        });
                    }
                }
            }
            Decl::Enum(enumeration) => {
                for value in &enumeration.values {
                    if value.name.is_missing() {
                        continue;
                    }
                    children.push(Symbol {
                        name: value.name.name.clone(),
                        detail: String::new(),
                        kind: K::ENUM_MEMBER,
                        start: value.span.start,
                        end: value.span.end,
                        name_start: value.name.span.start,
                        name_end: value.name.span.end,
                        children: Vec::new(),
                    });
                }
            }
            Decl::Resource(resource) => {
                for entry in &resource.entries {
                    children.push(Symbol {
                        name: entry.name.name.clone(),
                        detail: entry.ty.name.name.clone(),
                        kind: K::PROPERTY,
                        start: entry.span.start,
                        end: entry.span.end,
                        name_start: entry.name.span.start,
                        name_end: entry.name.span.end,
                        children: Vec::new(),
                    });
                }
            }
            _ => {}
        }
        out.push(Symbol {
            name: name.name.clone(),
            detail: keyword,
            kind,
            start: item.span.start,
            end: item.span.end,
            name_start: name.span.start,
            name_end: name.span.end,
            children,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
/// A city.
@since("1")
structure City {
    name: CityName = "x"
    kind?: Kind
    size: integer { range 1..10 }
}

string CityName { length 1..10 }

enum Kind { BIG = "big" }

operation GetCity reads City {
    http GET "/cities/{name}" -> 200
    paginated(items: name)
    input { $name }
    errors [Missing]
}
"#;

    fn role_of(highlights: &[Highlight], word: &str, nth: usize) -> (Role, u32) {
        let start = SOURCE
            .match_indices(word)
            .filter(|(index, _)| {
                // Whole words only.
                let before = SOURCE[..*index].chars().last();
                let after = SOURCE[index + word.len()..].chars().next();
                !before.is_some_and(|c| c.is_alphanumeric())
                    && !after.is_some_and(|c| c.is_alphanumeric())
            })
            .nth(nth)
            .unwrap()
            .0 as u32;
        let highlight = highlights
            .iter()
            .find(|highlight| highlight.start == start)
            .unwrap_or_else(|| panic!("no highlight for {word} #{nth}"));
        (highlight.role, highlight.modifiers)
    }

    #[test]
    fn tokens_take_their_role_from_the_tree() {
        let parsed = crucible_lang::parse(SOURCE);
        let index = Index::new(std::slice::from_ref(&parsed));
        let highlights = highlights(&parsed, &index);
        let role = |word, nth| role_of(&highlights, word, nth);
        assert_eq!(role("structure", 0), (Role::Keyword, 0));
        assert_eq!(role("City", 0), (Role::Struct, DECLARATION));
        assert_eq!(role("City", 1), (Role::Struct, 0));
        assert_eq!(role("since", 0), (Role::Decorator, 0));
        assert_eq!(role("name", 0), (Role::Property, DECLARATION));
        assert_eq!(role("CityName", 0), (Role::Type, 0));
        assert_eq!(role("CityName", 1), (Role::Type, DECLARATION));
        assert_eq!(role("Kind", 0), (Role::Enum, 0));
        assert_eq!(role("integer", 0), (Role::Type, DEFAULT_LIBRARY));
        assert_eq!(role("range", 0), (Role::Keyword, 0));
        assert_eq!(role("string", 0), (Role::Type, DEFAULT_LIBRARY));
        assert_eq!(role("BIG", 0), (Role::EnumMember, DECLARATION));
        assert_eq!(role("GetCity", 0), (Role::Function, DECLARATION));
        assert_eq!(role("reads", 0), (Role::Keyword, 0));
        assert_eq!(role("GET", 0), (Role::Keyword, 0));
        assert_eq!(role("items", 0), (Role::Parameter, 0));
        // Unresolved, but `errors [..]` only names errors.
        assert_eq!(role("Missing", 0), (Role::Class, ERROR));
        let doc = highlights.iter().find(|h| h.role == Role::Comment).unwrap();
        assert_eq!(doc.modifiers, DOCUMENTATION);
        assert!(highlights.iter().any(|h| h.role == Role::String));
        assert!(highlights.iter().any(|h| h.role == Role::Number));
    }

    #[test]
    fn every_resource_of_a_view_is_a_reference() {
        let source = "resource Job {\n    id jobId: string\n}\nresource Old {\n    id jobId: string\n}\nstructure Data for Job, Old {\n    $jobId\n}\n";
        let parsed = crucible_lang::parse(source);
        let index = Index::new(std::slice::from_ref(&parsed));
        let highlights = highlights(&parsed, &index);
        let role = |word: &str| {
            let start = source.find(&format!("for Job, {word}")).unwrap() + "for Job, ".len();
            let found = highlights.iter().find(|h| h.start == start as u32).unwrap();
            (found.role, found.modifiers)
        };
        assert_eq!(role("Old"), (Role::Class, RESOURCE));
    }

    #[test]
    fn permissions_and_requirements_are_classified() {
        let source = r#"
resource Inbox {
    id inboxId: string
    internal permission mailSend = "mail:send"
}
operation SendMessage on Inbox {
    requires mailSend on Inbox(inboxId: $inboxId) when has content.template
    filters mailSend on Inbox
}
"#;
        let parsed = crucible_lang::parse(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        let index = Index::new(std::slice::from_ref(&parsed));
        let highlights = highlights(&parsed, &index);
        let at = |word: &str, nth: usize| {
            let start = source
                .match_indices(word)
                .filter(|(index, _)| {
                    let before = source[..*index].chars().last();
                    let after = source[index + word.len()..].chars().next();
                    !before.is_some_and(char::is_alphanumeric)
                        && !after.is_some_and(char::is_alphanumeric)
                })
                .nth(nth)
                .unwrap()
                .0 as u32;
            let highlight = highlights.iter().find(|h| h.start == start).unwrap();
            (highlight.role, highlight.modifiers)
        };
        assert_eq!(at("permission", 0), (Role::Keyword, 0));
        assert_eq!(at("mailSend", 0), (Role::EnumMember, DECLARATION));
        assert_eq!(at("requires", 0), (Role::Keyword, 0));
        assert_eq!(at("mailSend", 1), (Role::EnumMember, 0));
        assert_eq!(at("Inbox", 2), (Role::Class, RESOURCE));
        assert_eq!(at("inboxId", 1), (Role::Property, 0));
        assert_eq!(at("when", 0), (Role::Keyword, 0));
        assert_eq!(at("has", 0), (Role::Keyword, 0));
        assert_eq!(at("content", 0), (Role::Property, 0));
        assert_eq!(at("filters", 0), (Role::Keyword, 0));
        assert_eq!(at("Inbox", 3), (Role::Class, RESOURCE));
    }

    #[test]
    fn declarations_and_references_carry_their_kind() {
        let source = r#"
service Shop {
    operations [GetCity]
    errors [NotFound]
}
resource City { id name: string }
error NotFound { status 404 }
union Shape { a: string }
operation GetCity reads City {
    input { $name }
    output { shape: Shape }
    errors [NotFound]
}
"#;
        let parsed = crucible_lang::parse(source);
        let index = Index::new(std::slice::from_ref(&parsed));
        let highlights = highlights(&parsed, &index);
        let at = |word: &str, nth: usize| {
            let start = source
                .match_indices(word)
                .filter(|(index, _)| {
                    let before = source[..*index].chars().last();
                    let after = source[index + word.len()..].chars().next();
                    !before.is_some_and(char::is_alphanumeric)
                        && !after.is_some_and(char::is_alphanumeric)
                })
                .nth(nth)
                .unwrap()
                .0 as u32;
            let highlight = highlights.iter().find(|h| h.start == start).unwrap();
            (highlight.role, highlight.modifiers)
        };
        assert_eq!(at("Shop", 0), (Role::Interface, DECLARATION | SERVICE));
        assert_eq!(at("City", 0), (Role::Class, DECLARATION | RESOURCE));
        assert_eq!(at("City", 1), (Role::Class, RESOURCE));
        assert_eq!(at("NotFound", 0), (Role::Class, ERROR));
        assert_eq!(at("NotFound", 1), (Role::Class, DECLARATION | ERROR));
        assert_eq!(at("NotFound", 2), (Role::Class, ERROR));
        assert_eq!(at("Shape", 1), (Role::Struct, UNION));
        assert_eq!(at("GetCity", 0), (Role::Function, 0));
        assert_eq!(at("GetCity", 1), (Role::Function, DECLARATION));
        assert_eq!(TOKEN_MODIFIERS.len(), 9);
        assert_eq!(
            1 << TOKEN_MODIFIERS.iter().position(|m| *m == "rule").unwrap(),
            RULE
        );
    }

    #[test]
    fn the_outline_lists_declarations_and_their_members() {
        let parsed = crucible_lang::parse(SOURCE);
        let outline = outline(&parsed);
        let names: Vec<&str> = outline.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["City", "CityName", "Kind", "GetCity"]);
        let city: Vec<&str> = outline[0]
            .children
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(city, ["name", "kind", "size"]);
        assert_eq!(outline[3].children[0].name, "input");
        assert_eq!(outline[3].children[0].children[0].name, "name");
    }

    #[test]
    fn test_kind_modifier() {
        assert_eq!(kind_modifier("service"), SERVICE);
        assert_eq!(kind_modifier("protocol"), PROTOCOL);
        assert_eq!(kind_modifier("resource"), RESOURCE);
        assert_eq!(kind_modifier("error"), ERROR);
        assert_eq!(kind_modifier("union"), UNION);
        assert_eq!(kind_modifier("rule"), RULE);

        // Edge cases and other keywords
        assert_eq!(kind_modifier("structure"), 0);
        assert_eq!(kind_modifier("operation"), 0);
        assert_eq!(kind_modifier(""), 0);
        assert_eq!(kind_modifier("unknown"), 0);
    }
}

#[cfg(test)]
mod doc_link_tests {
    use super::*;

    #[test]
    fn doc_links_are_found_across_lines_and_interleaved_comments() {
        let source = "/// See [Item] and\n// not documentation\n///   [Item.name], `[Item]`.\nstructure Item {\n    include Base\n}\nstructure Base { name: string }\nenum Kind { RED }\n";
        let parsed = crucible_lang::parse(source);
        let links = doc_links(&parsed, source);
        let spans: Vec<&str> = links
            .iter()
            .map(|l| &source[l.start as usize..l.end as usize])
            .collect();
        assert_eq!(spans, ["Item", "Item.name"]);
        let (member, start, end) = links[1].member.clone().unwrap();
        assert_eq!(member, "name");
        assert_eq!(&source[start as usize..end as usize], "name");
        let parsed = [parsed];
        // `name` comes from `include Base`, so it is found there.
        let (file, start, end) = member_location(&parsed, "Item", "name").unwrap();
        assert_eq!(file, 0);
        assert_eq!(start as usize, source.find("name: string").unwrap());
        assert_eq!(end - start, 4);
        let (_, start, _) = member_location(&parsed, "Kind", "RED").unwrap();
        assert_eq!(start as usize, source.find("RED").unwrap());
        assert!(member_location(&parsed, "Item", "missing").is_none());
    }
}
