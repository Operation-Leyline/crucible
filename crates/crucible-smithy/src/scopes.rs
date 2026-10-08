//! `@crucible.auth#scopes` is settled here once: emitters and the server read
//! the lowered lists, never the trait.
use std::collections::BTreeSet;

use serde_json::Value;

use crate::ast::Traits;
use crate::{Diagnostics, ShapeId};

pub(crate) fn trait_id() -> ShapeId {
    ShapeId::new("crucible.auth", "scopes")
}

/// RFC 6749 §3.3: `scope-token = 1*( %x21 / %x23-5B / %x5D-7E )`. Excluding
/// space, `"` and `\` also keeps a scope safe inside a quoted `WWW-Authenticate`
/// parameter without escaping.
fn scope_token(scope: &str) -> bool {
    !scope.is_empty()
        && scope
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x5B).contains(&b) || (0x5D..=0x7E).contains(&b))
}

/// The declared list in model order, or `None` when the shape declares none.
/// Invalid entries are diagnosed against `id` and left out.
pub(crate) fn lower(
    id: &ShapeId,
    traits: &Traits,
    diagnostics: &mut Diagnostics,
) -> Option<Vec<String>> {
    let value = traits.get(&trait_id())?;
    let Some(values) = value.as_array() else {
        diagnostics.push(id, "@crucible.auth#scopes must be a list of scope strings");
        return Some(Vec::new());
    };
    let mut seen = BTreeSet::new();
    let mut scopes = Vec::new();
    for value in values {
        match value {
            Value::String(scope) if scope_token(scope) => {
                if seen.insert(scope.as_str()) {
                    scopes.push(scope.clone());
                } else {
                    diagnostics.push(
                        id,
                        format!("@crucible.auth#scopes lists {scope:?} more than once"),
                    );
                }
            }
            Value::String(scope) => diagnostics.push(
                id,
                format!(
                    "@crucible.auth#scopes entry {scope:?} is not a scope token: it must be nonempty printable ASCII without spaces, '\"' or '\' (RFC 6749 section 3.3)"
                ),
            ),
            _ => diagnostics.push(id, "@crucible.auth#scopes entries must be strings"),
        }
    }
    Some(scopes)
}
