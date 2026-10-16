use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

/// The namespace of the built-in types (`crucible.api#String`), which no
/// model file declares, and of the standard vocabulary the `.cru` lowering
/// uses while it works (decision 62c in docs/idl/decisions.md).
pub const PRELUDE: &str = "crucible.api";

/// An absolute shape id: `namespace#Name`, or `namespace#Name$member`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeId {
    namespace: String,
    name: String,
    member: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not an absolute shape id (namespace#Name)")]
pub struct ShapeIdError(String);

impl ShapeId {
    pub fn new(namespace: &str, name: &str) -> Self {
        Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            member: None,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn member(&self) -> Option<&str> {
        self.member.as_deref()
    }

    pub fn with_member(&self, member: &str) -> Self {
        Self {
            member: Some(member.to_owned()),
            ..self.root()
        }
    }

    /// The shape a member id belongs to; a shape's own id is its own root.
    pub fn root(&self) -> Self {
        Self::new(&self.namespace, &self.name)
    }

    /// A built-in in the [`PRELUDE`] namespace: `ShapeId::prelude("String")`
    /// is `crucible.api#String`.
    pub fn prelude(name: &str) -> Self {
        Self::new(PRELUDE, name)
    }

    /// The id as Crucible spells it now: one in the `smithy.api` namespace,
    /// which release snapshots and infrastructure bindings written before
    /// the prelude moved still use, becomes its [`PRELUDE`] id (decision 62c
    /// in docs/idl/decisions.md). Every other id is unchanged.
    pub fn with_current_prelude(self) -> Self {
        if self.namespace == "smithy.api" {
            Self {
                namespace: PRELUDE.to_owned(),
                ..self
            }
        } else {
            self
        }
    }

    /// Whether the id is in the [`PRELUDE`], whose shapes no model file declares.
    pub fn is_prelude(&self) -> bool {
        self.namespace == PRELUDE
    }
}

impl FromStr for ShapeId {
    type Err = ShapeIdError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || ShapeIdError(text.to_owned());
        let (namespace, rest) = text.split_once('#').ok_or_else(invalid)?;
        let (name, member) = match rest.split_once('$') {
            Some((name, member)) => (name, Some(member)),
            None => (rest, None),
        };
        let identifier = |part: &str| {
            part.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        if !namespace.split('.').all(identifier)
            || !identifier(name)
            || member.is_some_and(|m| !identifier(m))
        {
            return Err(invalid());
        }
        Ok(Self {
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            member: member.map(str::to_owned),
        })
    }
}

impl fmt::Display for ShapeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.namespace, self.name)?;
        if let Some(member) = &self.member {
            write!(f, "${member}")?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ShapeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let id: ShapeId = text.parse().map_err(serde::de::Error::custom)?;
        Ok(READ_MAP.with(|map| match &*map.borrow() {
            Some(map) => map(id),
            None => id,
        }))
    }
}

type IdMap = std::rc::Rc<dyn Fn(ShapeId) -> ShapeId>;

thread_local! {
    /// The rewrite [`mapping_ids`] applies to every id deserialized on this
    /// thread while it runs.
    static READ_MAP: std::cell::RefCell<Option<IdMap>> = const { std::cell::RefCell::new(None) };
}

/// Runs `read` with every [`ShapeId`] deserialized on this thread passed
/// through `map`: each place the IR's types store an id (a field, a list
/// item or a map key typed `ShapeId`), and nothing else, since free-form
/// strings, values and documentation are not ids. Release snapshots map
/// their retired prelude ids this way, so a new id field in the IR is
/// mapped by its type alone.
pub(crate) fn mapping_ids<T>(
    map: impl Fn(ShapeId) -> ShapeId + 'static,
    read: impl FnOnce() -> T,
) -> T {
    struct Reset(Option<IdMap>);
    impl Drop for Reset {
        fn drop(&mut self) {
            READ_MAP.with(|map| *map.borrow_mut() = self.0.take());
        }
    }
    let previous = READ_MAP.with(|slot| slot.borrow_mut().replace(std::rc::Rc::new(map)));
    let _reset = Reset(previous);
    read()
}

impl serde::Serialize for ShapeId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_shape_and_member_ids() {
        for text in ["crucible.api#String", "dev.leyline#Graph$name", "a#B"] {
            assert_eq!(text.parse::<ShapeId>().unwrap().to_string(), text);
        }
    }

    #[test]
    fn rejects_relative_and_malformed_ids() {
        for text in ["String", "#String", "a#", "a#B$", "a.#B", "a#1B", "a#B$c$d"] {
            assert!(text.parse::<ShapeId>().is_err(), "{text}");
        }
    }

    #[test]
    fn member_root_is_its_shape() {
        let member: ShapeId = "a#B$c".parse().unwrap();
        assert_eq!(member.root(), ShapeId::new("a", "B"));
        assert_eq!(member.root().with_member("c"), member);
    }
}
