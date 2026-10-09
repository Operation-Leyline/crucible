//! Quick fixes for the compiler's diagnostics. Today: an unknown type offers
//! the declared names it most resembles, and declaring it as a structure.

use crate::analysis::{BUILTIN_TYPES, Index};

/// The name in an `unknown type `X`` diagnostic.
pub fn unknown_type(message: &str) -> Option<&str> {
    message
        .strip_prefix("unknown type `")?
        .strip_suffix('`')
        .filter(|name| !name.is_empty())
}

/// Declared or built-in type names close to `name`, closest first.
pub fn similar_types(name: &str, index: &Index) -> Vec<String> {
    let lower = name.to_lowercase();
    let limit = (name.chars().count() / 3).clamp(1, 3);
    let mut scored: Vec<(usize, String)> = index
        .declarations
        .iter()
        .filter(|(_, declaration)| {
            matches!(
                declaration.keyword.as_str(),
                "structure" | "union" | "enum" | "list" | "map"
            ) || BUILTIN_TYPES.contains(&declaration.keyword.as_str())
        })
        .map(|(candidate, _)| candidate.clone())
        .chain(BUILTIN_TYPES.iter().map(|kind| (*kind).to_owned()))
        .filter(|candidate| candidate != name)
        .filter_map(|candidate| {
            let distance = edit_distance(&lower, &candidate.to_lowercase());
            (distance <= limit).then_some((distance, candidate))
        })
        .collect();
    scored.sort();
    scored
        .into_iter()
        .take(3)
        .map(|(_, candidate)| candidate)
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let current = row[j + 1];
            row[j + 1] = (previous + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(current + 1);
            previous = current;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_close_names() {
        let parsed =
            crucible_lang::parse("structure ItemId {}\nstructure Other {}\nerror ItemIdError {}\n");
        let index = Index::new(std::slice::from_ref(&parsed));
        assert_eq!(unknown_type("unknown type `ItemIdd`"), Some("ItemIdd"));
        assert_eq!(unknown_type("something else"), None);
        assert_eq!(similar_types("ItemIdd", &index), ["ItemId"]);
        assert_eq!(similar_types("strng", &index), ["string"]);
        assert!(similar_types("Zzzzzz", &index).is_empty());
    }
}
