//! The Smithy JSON AST, read as written. One flat struct per shape rather than
//! an enum per type: which fields a type may carry is checked while lowering,
//! where a mistake can be reported against the shape that made it.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::shape_id::ShapeId;

pub(crate) type Traits = BTreeMap<ShapeId, Value>;

#[derive(Debug, Deserialize)]
pub(crate) struct Model {
    pub smithy: String,
    #[serde(default)]
    pub shapes: BTreeMap<ShapeId, Shape>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Blob,
    Boolean,
    String,
    Byte,
    Short,
    Integer,
    IntEnum,
    Long,
    Float,
    Double,
    BigInteger,
    BigDecimal,
    Timestamp,
    Document,
    Enum,
    List,
    Set,
    Map,
    Structure,
    Union,
    Service,
    Operation,
    Resource,
    Apply,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Ref {
    pub target: ShapeId,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Member {
    pub target: ShapeId,
    #[serde(default)]
    pub traits: Traits,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Shape {
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub traits: Traits,
    #[serde(default)]
    pub mixins: Vec<Ref>,
    /// Source order is kept: it is the order emitted fields appear in.
    #[serde(default)]
    pub members: IndexMap<String, Member>,
    pub member: Option<Member>,
    pub key: Option<Member>,
    pub value: Option<Member>,
    pub version: Option<String>,
    #[serde(default)]
    pub operations: Vec<Ref>,
    #[serde(default)]
    pub collection_operations: Vec<Ref>,
    #[serde(default)]
    pub resources: Vec<Ref>,
    #[serde(default)]
    pub errors: Vec<Ref>,
    #[serde(default)]
    pub rename: BTreeMap<ShapeId, String>,
    pub input: Option<Ref>,
    pub output: Option<Ref>,
    pub create: Option<Ref>,
    pub put: Option<Ref>,
    pub read: Option<Ref>,
    pub update: Option<Ref>,
    pub delete: Option<Ref>,
    pub list: Option<Ref>,
}

impl Shape {
    fn bare(kind: Kind, traits: Traits) -> Self {
        Self {
            kind,
            traits,
            mixins: Vec::new(),
            members: IndexMap::new(),
            member: None,
            key: None,
            value: None,
            version: None,
            operations: Vec::new(),
            collection_operations: Vec::new(),
            resources: Vec::new(),
            errors: Vec::new(),
            rename: BTreeMap::new(),
            input: None,
            output: None,
            create: None,
            put: None,
            read: None,
            update: None,
            delete: None,
            list: None,
        }
    }
}

/// The prelude shapes a member may target. A built model leaves them out:
/// every Smithy tool is expected to know them.
pub(crate) fn prelude() -> impl Iterator<Item = (ShapeId, Shape)> {
    let plain = [
        ("Blob", Kind::Blob),
        ("Boolean", Kind::Boolean),
        ("String", Kind::String),
        ("Byte", Kind::Byte),
        ("Short", Kind::Short),
        ("Integer", Kind::Integer),
        ("Long", Kind::Long),
        ("Float", Kind::Float),
        ("Double", Kind::Double),
        ("BigInteger", Kind::BigInteger),
        ("BigDecimal", Kind::BigDecimal),
        ("Timestamp", Kind::Timestamp),
        ("Document", Kind::Document),
    ]
    .map(|(name, kind)| (name, kind, Traits::new()));
    let default = |value: Value| Traits::from([(ShapeId::new("smithy.api", "default"), value)]);
    let primitive = [
        ("PrimitiveBoolean", Kind::Boolean, default(json!(false))),
        ("PrimitiveByte", Kind::Byte, default(json!(0))),
        ("PrimitiveShort", Kind::Short, default(json!(0))),
        ("PrimitiveInteger", Kind::Integer, default(json!(0))),
        ("PrimitiveLong", Kind::Long, default(json!(0))),
        ("PrimitiveFloat", Kind::Float, default(json!(0))),
        ("PrimitiveDouble", Kind::Double, default(json!(0))),
    ];
    let unit = (
        "Unit",
        Kind::Structure,
        Traits::from([(ShapeId::new("smithy.api", "unitType"), json!({}))]),
    );
    plain
        .into_iter()
        .chain(primitive)
        .chain([unit])
        .map(|(name, kind, traits)| (ShapeId::new("smithy.api", name), Shape::bare(kind, traits)))
}
