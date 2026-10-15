//! API names are flat: every emitter puts the service's shapes in one
//! namespace (one module, one `models.ts`, one OpenAPI components map), so two
//! shapes must not share a name, nor differ only by case, which collides on a
//! case-insensitive file system and in languages that fold identifiers.
use std::collections::BTreeMap;

use crate::{Diagnostics, ShapeId};

/// Checks the names the service's shapes go by once `rename` is applied,
/// reporting each clash against the later shape of the pair.
pub(crate) fn check_unique<'a>(
    names: impl Iterator<Item = (&'a ShapeId, &'a str)>,
    diagnostics: &mut Diagnostics,
) {
    let mut seen: BTreeMap<String, (&ShapeId, &str)> = BTreeMap::new();
    for (id, name) in names {
        let folded = name.to_ascii_lowercase();
        let Some(&(previous, previous_name)) = seen.get(&folded) else {
            seen.insert(folded, (id, name));
            continue;
        };
        let reason = if previous_name == name {
            format!("shape name `{name}` is also used by {previous}")
        } else {
            format!("shape name `{name}` differs from `{previous_name}` of {previous} only by case")
        };
        diagnostics.push(
            id,
            format!("{reason}; rename one of them with `rename` on the service"),
        );
    }
}
