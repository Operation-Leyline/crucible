//! Typed permissions (docs/permissions.md §6.1): what a resource accepts and
//! what an operation requires. The `.cru` lowering declares them, and
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

    /// The primary `permission` stands for: itself, or the one it aliases.
    pub fn primary<'a>(&'a self, permission: &'a Permission) -> &'a Permission {
        permission
            .alias_of
            .as_deref()
            .and_then(|symbol| self.permission(symbol))
            .unwrap_or(permission)
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
///
/// Serialized by hand (`requirements_serde`): `requires none` with neither
/// documentation nor a filter is still the bare `"None"` it always was, so
/// the snapshot of a model that adds neither is unchanged, and a snapshot
/// frozen before `None` carried fields still reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Requirements {
    /// Nothing: a compile error on an authenticated operation once the
    /// service declares any permission. What an anonymous operation, and every
    /// operation of a service without permissions, lowers to; the client
    /// projection also turns `Pending` into it.
    #[default]
    Undeclared,
    /// `requires none`: authenticated, and no permission is needed.
    None {
        /// The clause's `///`: why no permission is needed, or what the
        /// handler checks per item instead (docs/permissions.md §4.7).
        docs: Option<String>,
        /// Only in a client-audience projection: every clause was a
        /// `filters` on an internal permission, which the projection
        /// removed. Nothing refuses the caller, but what the operation
        /// answers is narrowed, and a reader is told so without the
        /// permission being named. A front end never sets it.
        filtered: bool,
    },
    /// `requires pending because "..."`: the handler still checks.
    Pending {
        reason: String,
        /// The clause's `///`, for the server audience: the client
        /// projection turns `Pending` into `Undeclared`.
        docs: Option<String>,
    },
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
    /// `requires none` with neither documentation nor a filter.
    pub const NONE: Self = Self::None {
        docs: Option::None,
        filtered: false,
    };

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

/// The serde form of [`Requirements`], a release snapshot's: externally
/// tagged, as derived, except that `requires none` with neither field set
/// is written as the bare `"None"`, and both spellings read.
mod requirements_serde {
    use serde::de::value::{MapAccessDeserializer, StrDeserializer};
    use serde::de::{Error, MapAccess, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{Requirement, Requirements};

    fn is_false(value: &bool) -> bool {
        !value
    }

    #[derive(Serialize)]
    #[serde(rename = "Requirements")]
    enum Write<'a> {
        Undeclared,
        None {
            #[serde(skip_serializing_if = "Option::is_none")]
            docs: &'a Option<String>,
            #[serde(skip_serializing_if = "is_false")]
            filtered: bool,
        },
        Pending {
            reason: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            docs: &'a Option<String>,
        },
        List(&'a [Requirement]),
        Internal,
        Partial(&'a [Requirement]),
    }

    #[derive(Deserialize)]
    #[serde(rename = "Requirements")]
    enum Read {
        Undeclared,
        None {
            #[serde(default)]
            docs: Option<String>,
            #[serde(default)]
            filtered: bool,
        },
        Pending {
            reason: String,
            #[serde(default)]
            docs: Option<String>,
        },
        List(Vec<Requirement>),
        Internal,
        Partial(Vec<Requirement>),
    }

    impl Serialize for Requirements {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self {
                Self::None {
                    docs: Option::None,
                    filtered: false,
                } => serializer.serialize_unit_variant("Requirements", 1, "None"),
                Self::None { docs, filtered } => Write::None {
                    docs,
                    filtered: *filtered,
                }
                .serialize(serializer),
                Self::Undeclared => Write::Undeclared.serialize(serializer),
                Self::Pending { reason, docs } => {
                    Write::Pending { reason, docs }.serialize(serializer)
                }
                Self::List(clauses) => Write::List(clauses).serialize(serializer),
                Self::Internal => Write::Internal.serialize(serializer),
                Self::Partial(clauses) => Write::Partial(clauses).serialize(serializer),
            }
        }
    }

    impl<'de> Deserialize<'de> for Requirements {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Either;
            impl<'de> Visitor<'de> for Either {
                type Value = Read;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("an operation's requirements")
                }
                // A unit variant, `"None"` among them: how every snapshot
                // from before `None` carried fields spells it.
                fn visit_str<E: Error>(self, variant: &str) -> Result<Read, E> {
                    match variant {
                        "None" => Ok(Read::None {
                            docs: Option::None,
                            filtered: false,
                        }),
                        _ => Read::deserialize(StrDeserializer::<E>::new(variant)),
                    }
                }
                fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Read, A::Error> {
                    Read::deserialize(MapAccessDeserializer::new(map))
                }
            }
            Ok(match deserializer.deserialize_any(Either)? {
                Read::Undeclared => Self::Undeclared,
                Read::None { docs, filtered } => Self::None { docs, filtered },
                Read::Pending { reason, docs } => Self::Pending { reason, docs },
                Read::List(clauses) => Self::List(clauses),
                Read::Internal => Self::Internal,
                Read::Partial(clauses) => Self::Partial(clauses),
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::{Mode, Requirement, Requirements};
        use crate::ShapeId;

        fn round_trip(requirements: &Requirements) -> String {
            let text = serde_json::to_string(requirements).unwrap();
            let back: Requirements = serde_json::from_str(&text).unwrap();
            assert_eq!(&back, requirements, "{text}");
            text
        }

        #[test]
        fn an_undocumented_none_is_written_as_it_always_was() {
            assert_eq!(round_trip(&Requirements::NONE), r#""None""#);
        }

        #[test]
        fn a_snapshot_from_before_none_carried_fields_reads() {
            let none: Requirements = serde_json::from_str(r#""None""#).unwrap();
            assert_eq!(none, Requirements::NONE);
            let pending: Requirements =
                serde_json::from_str(r#"{"Pending":{"reason":"later"}}"#).unwrap();
            assert_eq!(
                pending,
                Requirements::Pending {
                    reason: "later".into(),
                    docs: Option::None,
                }
            );
        }

        #[test]
        fn every_variant_round_trips() {
            assert_eq!(
                round_trip(&Requirements::None {
                    docs: Some("Each graph is checked.".into()),
                    filtered: false,
                }),
                r#"{"None":{"docs":"Each graph is checked."}}"#
            );
            assert_eq!(
                round_trip(&Requirements::None {
                    docs: Option::None,
                    filtered: true,
                }),
                r#"{"None":{"filtered":true}}"#
            );
            assert_eq!(
                round_trip(&Requirements::Pending {
                    reason: "later".into(),
                    docs: Some("Why.".into()),
                }),
                r#"{"Pending":{"reason":"later","docs":"Why."}}"#
            );
            assert_eq!(
                round_trip(&Requirements::Pending {
                    reason: "later".into(),
                    docs: Option::None,
                }),
                r#"{"Pending":{"reason":"later"}}"#
            );
            assert_eq!(round_trip(&Requirements::Undeclared), r#""Undeclared""#);
            assert_eq!(round_trip(&Requirements::Internal), r#""Internal""#);
            let clause = Requirement {
                mode: Mode::Check,
                resource: ShapeId::new("example", "Inbox"),
                permission: "read".into(),
                bindings: Vec::new(),
                when: Option::None,
                docs: Option::None,
            };
            round_trip(&Requirements::List(vec![clause.clone()]));
            round_trip(&Requirements::Partial(vec![clause]));
        }

        #[test]
        fn an_unknown_variant_is_refused() {
            assert!(serde_json::from_str::<Requirements>(r#""Some""#).is_err());
            assert!(serde_json::from_str::<Requirements>(r#"{"Some":1}"#).is_err());
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

    /// Every member path the condition reads, to rewrite.
    pub fn paths_mut(&mut self) -> Vec<&mut Vec<String>> {
        match self {
            Self::Has(p) | Self::Equals(p, _) | Self::In(p, _) => vec![p],
            Self::And(cs) | Self::Or(cs) => cs.iter_mut().flat_map(Condition::paths_mut).collect(),
            Self::Not(c) => c.paths_mut(),
        }
    }
}
