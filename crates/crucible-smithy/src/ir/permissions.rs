//! Typed permissions (docs/permissions.md §6.1): what a resource accepts and
//! what an operation requires. Only the `.cru` front end declares them (a
//! Smithy model that uses a permission trait is refused at lowering), and
//! `crate::validate::permissions` checks them once.
//!
//! The serde names are part of the release snapshot format: every field is
//! `#[serde(default)]`, so a release frozen before permissions existed still
//! reads, and an empty value is not written, so its snapshot is unchanged.
//! `ResourcePermissions::parents` is the exception: `None` (not written)
//! marks a release frozen before parents were recorded, and `Some([])`, a
//! root, is written.

use serde_json::Value;

use crate::doc_notes::DocNotes;
use crate::shape_id::ShapeId;

/// The permissions one resource declares (`Service::permissions`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResourcePermissions {
    /// The authorization name: `inbox`. Stored in every grant and audit
    /// record together with a permission's value.
    #[serde(default)]
    pub name: String,
    /// `@authorizationName` set it, rather than the shape name.
    #[serde(default)]
    pub name_declared: bool,
    /// The resource's identifiers, root to leaf: `["workspaceId", "inboxId"]`.
    #[serde(default)]
    pub identifiers: Vec<String>,
    /// The resources it is nested in, root first, each by authorization name
    /// when it declares permissions (a shape rename keeps it), else by shape
    /// name: `["workspace"]`. With `identifiers`, what a binding's positions
    /// mean: a resource moved to another parent whose identifier has the
    /// same name keeps its identifiers but not its parents. `None` in a
    /// release frozen before parents were recorded; `Some([])` for a root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parents: Option<Vec<String>>,
    /// Source order, aliases included.
    #[serde(default)]
    pub permissions: Vec<Permission>,
}

impl ResourcePermissions {
    /// The permission whose symbol is `symbol`, alias or primary.
    pub fn permission(&self, symbol: &str) -> Option<&Permission> {
        self.permissions.iter().find(|p| p.symbol == symbol)
    }

    /// The permission whose value is `value`, alias or primary. Values are
    /// compared case-sensitively.
    pub fn by_value(&self, value: &str) -> Option<&Permission> {
        self.permissions.iter().find(|p| p.value == value)
    }
}

/// One permission a resource accepts.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Permission {
    /// What the model and generated code call it: `mailSend`.
    #[serde(default)]
    pub symbol: String,
    /// Its identity, stored in grants: `mail:send`.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub tier: Tier,
    /// The primary's symbol, for a deprecated alias of it.
    #[serde(default)]
    pub alias_of: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    /// `@deprecated`, `@since`.
    #[serde(default)]
    pub notes: DocNotes,
}

/// Who sees a permission (decision 9).
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Tier {
    /// Required by operations, offered for grants, in every output.
    #[default]
    Public,
    /// Required by no operation; checked by the service's policy.
    PermissionOnly,
    /// A bundle the service's policy confers whole, never offered for composition.
    NotGrantable,
    /// Server contract only: absent from every client-facing output.
    Internal,
}

/// The hand-written wire enum `@permissionTypes` checks (§4.9).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermissionTypes {
    /// The enum.
    pub shape: ShapeId,
    /// Resources deliberately not grantable through it.
    #[serde(default)]
    pub excluded: Vec<ShapeId>,
}

/// What an operation says about authorization.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Requirements {
    /// Nothing: a compile error on an authenticated operation once the
    /// service declares any permission. What a public operation, and every
    /// operation of a service without permissions, lowers to; the client
    /// projection also turns `Pending` into it.
    #[default]
    Undeclared,
    /// `requires none`: authenticated, and no permission is needed.
    None,
    /// `requires pending because "..."`: the handler still checks.
    Pending { reason: String },
    /// Every clause, in source order, which is the refusal order. Never
    /// empty: the checks refuse an empty list.
    List(Vec<Requirement>),
    /// Only in a client-audience projection (`crate::project`): the
    /// operation has requirements, and every one names an internal
    /// permission, which the projection removed. A front end never produces
    /// it; the server contract keeps the clauses.
    Internal,
    /// Only in a client-audience projection: the clauses this audience
    /// sees, in source order, never empty; at least one other clause named
    /// an internal permission and was removed. A client must not read the
    /// list as complete: holding every permission it names may still be
    /// refused.
    Partial(Vec<Requirement>),
}

impl Requirements {
    pub fn is_undeclared(&self) -> bool {
        matches!(self, Self::Undeclared)
    }

    /// The clauses, empty unless this is a list (complete or partial).
    pub fn clauses(&self) -> &[Requirement] {
        match self {
            Self::List(list) | Self::Partial(list) => list,
            _ => &[],
        }
    }
}

/// One `requires` or `filters` clause.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Requirement {
    #[serde(default)]
    pub mode: Mode,
    pub resource: ShapeId,
    /// The primary's symbol, never an alias.
    #[serde(default)]
    pub permission: String,
    /// One per identifier of `resource`, root to leaf; empty for `Handler`.
    #[serde(default)]
    pub bindings: Vec<Binding>,
    /// `None` for `Handler`.
    #[serde(default)]
    pub when: Option<Condition>,
    #[serde(default)]
    pub docs: Option<String>,
}

/// How a clause is enforced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// Checked before the operation runs.
    #[default]
    Check,
    /// `filters`: a listing answered with what the caller may see.
    Filter,
    /// `by handler`: an obligation the handler discharges.
    Handler,
}

/// Where one identifier's value comes from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Binding {
    /// An input member path; a label is a path of one segment. A path never
    /// crosses a list or map.
    Member(Vec<String>),
    /// `*`: every instance, including ones created later.
    All,
}

/// A `when` condition over the decoded input.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Condition {
    /// The optional member is present, or the union arm selected.
    Has(Vec<String>),
    Equals(Vec<String>, Value),
    In(Vec<String>, Vec<Value>),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Not(Box<Condition>),
}

impl Condition {
    /// The condition as `.cru` spells it.
    pub fn text(&self) -> String {
        fn path(p: &[String]) -> String {
            p.join(".")
        }
        fn group(c: &Condition) -> String {
            match c {
                Condition::And(_) | Condition::Or(_) => format!("({})", c.text()),
                _ => c.text(),
            }
        }
        match self {
            Self::Has(p) => format!("has {}", path(p)),
            Self::Equals(p, v) => format!("{} == {v}", path(p)),
            Self::In(p, vs) => format!(
                "{} in [{}]",
                path(p),
                vs.iter()
                    .map(Value::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::And(cs) => cs.iter().map(group).collect::<Vec<_>>().join(" and "),
            Self::Or(cs) => cs.iter().map(group).collect::<Vec<_>>().join(" or "),
            Self::Not(c) => format!("not {}", group(c)),
        }
    }

    /// Every member path the condition reads.
    pub fn paths(&self) -> Vec<&[String]> {
        match self {
            Self::Has(p) | Self::Equals(p, _) | Self::In(p, _) => vec![p],
            Self::And(cs) | Self::Or(cs) => cs.iter().flat_map(Condition::paths).collect(),
            Self::Not(c) => c.paths(),
        }
    }
}
