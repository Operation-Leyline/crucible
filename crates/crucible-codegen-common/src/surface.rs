//! What a generated client shows its callers, named by the generator that
//! emits it: the package to install and import, the client to construct,
//! and per operation the method, paginators, and input and output types.
//!
//! Each client generator builds its [`Surface`] from the same naming
//! functions and collision allocators its emitted code uses, so the two
//! cannot disagree. Documentation sites read it, through the
//! `crucible.sdks/1` contract (docs/docs.md), instead of re-deriving a
//! language's casing, suffixes and type mapping.
//!
//! The structs are non-exhaustive so the contract can gain fields: start
//! from `new` and set the remaining fields.

use std::collections::BTreeMap;

use serde::Serialize;

/// One client's public surface.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Surface {
    /// The target, as `crucible generate` spells it: `rust`, `ts`, ...
    pub language: String,
    pub package: Package,
    pub client: ClientType,
    /// The endpoint the client uses when its configuration names none.
    pub endpoint: Option<String>,
    /// Middleware every client runs by default, ahead of the caller's.
    pub middleware: Vec<Middleware>,
    /// Every operation the client exposes, by absolute operation shape ID.
    pub operations: BTreeMap<String, Operation>,
}

impl Surface {
    pub fn new(language: &str, package: Package, client: ClientType) -> Self {
        Self {
            language: language.to_owned(),
            package,
            client,
            endpoint: None,
            middleware: Vec::new(),
            operations: BTreeMap::new(),
        }
    }
}

/// A default middleware, as `crucible-sdk.toml` names it, and the request
/// headers it adds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Middleware {
    pub name: String,
    pub headers: Vec<String>,
    /// The requests it adds its headers to, completing "sent on ...", where
    /// that is not every request: what the package's README and `AGENTS.md`
    /// and the snippets' header comment say. Not part of `crucible.sdks/1`.
    #[serde(skip)]
    pub sent_on: Option<String>,
}

impl Middleware {
    pub fn new(name: &str, headers: &[&str]) -> Self {
        Self {
            name: name.to_owned(),
            headers: headers.iter().map(|header| (*header).to_owned()).collect(),
            sent_on: None,
        }
    }

    /// `content-sha256`, as the Rust and TypeScript runtimes (the only
    /// clients that offer it) implement it: the body's SHA-256 in
    /// `x-amz-content-sha256` on every POST, PUT and PATCH (the empty
    /// body's digest when there is none) and on any other request with a
    /// body, never on a bodiless GET or DELETE.
    pub fn content_sha256() -> Self {
        Self {
            sent_on: Some(
                "every POST, PUT and PATCH (the empty body's digest when there is none) \
                 and any other request with a body"
                    .to_owned(),
            ),
            ..Self::new("content-sha256", &["x-amz-content-sha256"])
        }
    }
}

/// How a caller gets the package and refers to it in code.
///
/// Every native spelling in a surface (the client's name and construction,
/// input and output types, member types) resolves in a source file that
/// starts with [`Package::imports`]; the package's own declarations carry
/// [`Package::qualifier`] where those imports leave them qualified.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Package {
    /// `cargo`, `npm`, `pypi`, `go`, `maven`, `nuget`, `swiftpm`, `pub`,
    /// `composer`, `rubygems`, `cmake` or `cran`.
    pub ecosystem: String,
    /// The coordinate the ecosystem's tool installs, without a version:
    /// a crate, npm or PyPI name, a Go module path, Maven `group:artifact`.
    pub name: String,
    /// What source code names to use the package: a crate path, an import
    /// specifier, a module, package or namespace, a header.
    pub import: String,
    /// The source lines a snippet starts with, in order: `use`, `import`,
    /// `require`, `#include` or `library` lines that bring the package (and
    /// any platform library its types name) into scope.
    pub imports: Vec<String>,
    /// The prefix the package's own declarations take after [`imports`]:
    /// Go `weather.`, Ruby `Weather::`, empty where the imports bring them
    /// in unqualified. Types in the surface already include it.
    ///
    /// [`imports`]: Package::imports
    pub qualifier: String,
}

impl Package {
    pub fn new(ecosystem: &str, name: impl Into<String>, import: impl Into<String>) -> Self {
        Self {
            ecosystem: ecosystem.to_owned(),
            name: name.into(),
            import: import.into(),
            imports: Vec::new(),
            qualifier: String::new(),
        }
    }
}

/// The client type and how a caller constructs it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ClientType {
    /// The class, struct or type name, qualified as [`Package::qualifier`]
    /// says; C's is its service prefix's client type.
    pub name: String,
    /// Native source that constructs the client: an expression, or with
    /// [`ClientType::statements`] a statement sequence that declares a
    /// variable `client`. Where no endpoint is generated into the client,
    /// it reads one from a variable named `endpoint`.
    pub construct: String,
    /// `construct` is a statement sequence declaring `client`, not an
    /// expression (C, Java).
    pub statements: bool,
    /// Constructing can fail: Go's constructor also returns an `error`,
    /// Swift's throws (`construct` carries its `try`), C's reports one.
    pub fallible: bool,
}

impl ClientType {
    pub fn new(name: impl Into<String>, construct: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            construct: construct.into(),
            statements: false,
            fallible: false,
        }
    }
}

/// One operation as the client exposes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Operation {
    /// The method (or function) a caller invokes.
    pub method: String,
    /// A separate method that also takes per-call options, where the
    /// language has one instead of an optional parameter.
    pub options_method: Option<String>,
    /// A separate asynchronous method, where the language has one beside
    /// the blocking `method` (Java's `xAsync`, C#'s `XAsync`).
    pub async_method: Option<String>,
    /// The methods that page through a `@paginated` operation's results;
    /// `None` for an operation that is not paginated.
    pub paginators: Option<Vec<String>>,
    /// `None` when the operation takes no modeled input.
    pub input: Option<Structure>,
    /// `None` when the operation returns no modeled output.
    pub output: Option<Structure>,
    /// A complete program calling the operation: imports, the client built
    /// with its token, the input with its example's values (or the
    /// generated sample's), the call, reading the output, and handling one
    /// modelled error (docs/docs.md, "Snippets").
    pub snippet: String,
    /// The same program paging through the results, for a paginated
    /// operation; `None` otherwise.
    pub paginated_snippet: Option<String>,
}

impl Operation {
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            ..Self::default()
        }
    }
}

/// An input or output type: its name, how it is built or read, and its
/// members in model order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Structure {
    /// The native type, as a caller names it after [`Package::imports`].
    #[serde(rename = "type")]
    pub type_name: String,
    /// For an input, how a caller constructs it; for an output, how a
    /// caller reads its members. docs/docs.md lists the styles.
    pub style: String,
    /// For a builder input: whether finishing the builder can fail (a
    /// required member without a default), so the call site handles it.
    pub fallible: bool,
    pub members: Vec<Member>,
}

impl Structure {
    pub fn new(type_name: impl Into<String>, style: &str) -> Self {
        Self {
            type_name: type_name.into(),
            style: style.to_owned(),
            ..Self::default()
        }
    }
}

/// A member as the caller sets (input) or reads (output) it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Member {
    /// The model's member name.
    pub member: String,
    /// The native identifier: a property, field, setter, getter or
    /// keyword argument, as the structure's style uses it, escaped as the
    /// generated code writes it (Kotlin's backticks, C#'s `@`, Rust's `r#`).
    pub name: String,
    /// The native type, as a caller writes it after [`Package::imports`]:
    /// what the style's form takes or yields. That is the value's type,
    /// unless the language's field holds a wrapper a bare value does not
    /// convert to (Go's `Optional[T]`).
    #[serde(rename = "type")]
    pub type_name: String,
    /// For an input, whether the caller may leave the member out of the
    /// style's form (a modeled default the server or builder applies, an
    /// idempotency token the client fills, or an optional member); for an
    /// output, whether it may be absent. Each generator states its own
    /// rule, the one its emitted code follows, so languages may differ.
    pub optional: bool,
}

impl Member {
    /// `optional` is the generator's own rule (see [`Member::optional`]),
    /// which no model property alone decides.
    pub fn new(
        member: &crucible_ir::ir::Member,
        name: impl Into<String>,
        type_name: impl Into<String>,
        optional: bool,
    ) -> Self {
        Self {
            member: member.name.clone(),
            name: name.into(),
            type_name: type_name.into(),
            optional,
        }
    }
}

#[doc(hidden)]
/// `path` without Rust- and C++-style qualifiers: `::std::vec::Vec<::a::B>`
/// reads `Vec<B>`, as a caller writes it after importing the package.
pub fn unqualified(path: &str, separator: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut segment = String::new();
    let mut chars = path.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if path[at..].starts_with(separator) {
            segment.clear();
            for _ in 1..separator.chars().count() {
                chars.next();
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' || c == '#' {
            segment.push(c);
        } else {
            out.push_str(&segment);
            segment.clear();
            out.push(c);
        }
    }
    out.push_str(&segment);
    out
}

#[cfg(test)]
mod tests {
    use super::unqualified;

    #[test]
    fn qualifiers_are_dropped() {
        assert_eq!(unqualified("::std::vec::Vec<::crate_a::B>", "::"), "Vec<B>");
        assert_eq!(unqualified("i64", "::"), "i64");
        assert_eq!(
            unqualified("std::map<std::string, ns::Value>", "::"),
            "map<string, Value>"
        );
        assert_eq!(unqualified("globalThis.Date", "."), "Date");
    }
}
