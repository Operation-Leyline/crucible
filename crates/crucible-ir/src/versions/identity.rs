//! Conservative proof that two operation type closures are identical.

use std::collections::BTreeSet;

use crate::ShapeId;
use crate::ir::{Operation, ResponseMode, Service};

/// The shared input/output/error closure when every lowered record and wire
/// binding is identical. Missing shapes or differences, even documentation,
/// return `None`: callers may conservatively keep a conversion path.
///
/// This compares settled IR facts, including effective member constraints,
/// nullability, defaults and wire formats; it does not re-derive them. A server
/// with additional decoding policy must also compare that policy for each id
/// returned (for example, frozen-release output leniency).
pub fn identical_operation_closure(
    older: &Service,
    old: &Operation,
    current: &Service,
    new: &Operation,
) -> Option<BTreeSet<ShapeId>> {
    if old.response_mode != ResponseMode::Unary
        || new.response_mode != ResponseMode::Unary
        || old.input != new.input
        || old.output != new.output
        || old.method != new.method
        || !same(&old.uri, &new.uri)
        || old.status != new.status
        || old.protocol_headers != new.protocol_headers
        || old.response_protocol_headers != new.response_protocol_headers
        || old.errors != new.errors
        || older.protocol != current.protocol
        || !same(&older.protocol_settings, &current.protocol_settings)
        || !same(&old.request, &new.request)
        || !same(&old.response, &new.response)
    {
        return None;
    }
    let mut stack: Vec<_> = old
        .input
        .iter()
        .chain(&old.output)
        .chain(&old.errors)
        .cloned()
        .collect();
    let mut closure = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !closure.insert(id.clone()) {
            continue;
        }
        let (before, after) = (older.shape(&id)?, current.shape(&id)?);
        if !same(before, after) {
            return None;
        }
        stack.extend(
            before
                .members()
                .into_iter()
                .map(|member| member.target.clone()),
        );
    }
    Some(closure)
}

fn same<T: serde::Serialize>(before: &T, after: &T) -> bool {
    // Serialization includes every IR field, so newly added facts disable the
    // optimization automatically until both closures agree on them.
    matches!(
        (serde_json::to_value(before), serde_json::to_value(after)),
        (Ok(before), Ok(after)) if before == after
    )
}
