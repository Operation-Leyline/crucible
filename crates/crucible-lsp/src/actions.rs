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
    fn test_unknown_type() {
        // Happy path
        assert_eq!(unknown_type("unknown type `ItemId`"), Some("ItemId"));

        // Missing prefix
        assert_eq!(unknown_type("ItemId`"), None);
        assert_eq!(unknown_type("known type `ItemId`"), None);

        // Missing suffix
        assert_eq!(unknown_type("unknown type `ItemId"), None);

        // Empty type name
        assert_eq!(unknown_type("unknown type ``"), None);

        // Other malformed messages
        assert_eq!(unknown_type("something else"), None);
        assert_eq!(unknown_type(""), None);
    }

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

    #[test]
    fn test_similar_types_edge_cases() {
        let parsed = crucible_lang::parse(
            "structure ExactMatch {}\nstructure SameSize {}\nstructure A {}\nstructure ABCDE {}\n",
        );
        let index = Index::new(std::slice::from_ref(&parsed));

        // Exact match but different case
        assert_eq!(similar_types("exactmatch", &index), ["ExactMatch"]);

        // Empty string
        // Note: distance("", "a") = 1, and limit for "" is 1.
        // So `similar_types("", ...)` will match any type of length 1!
        assert_eq!(similar_types("", &index), ["A"]);

        // Limits. `name.chars().count() / 3`.
        // Length 1..=5: limit 1
        assert_eq!(similar_types("a", &index), ["A"]); // length 1
        assert_eq!(similar_types("b", &index), ["A"]); // length 1, dist 1

        // Let's test boundary cases
        // 5 / 3 = 1 -> Limit 1
        // (note: "ABCDE" == name will be filtered out because candidate == name!)
        assert!(similar_types("ABCDE", &index).is_empty()); // Exact match is filtered out!
        assert_eq!(similar_types("abcde", &index), ["ABCDE"]); // Match different case
        assert_eq!(similar_types("xBCDE", &index), ["ABCDE"]); // Dist 1
        assert!(similar_types("xyCDE", &index).is_empty()); // Dist 2 > Limit 1

        // Length 6: limit 2 (6 / 3 = 2)
        // Let's check "ABCDEF" with distance 2 to "ABCDE" (deletion + substitution or 2 insertions/deletions)
        assert_eq!(similar_types("ABCDEF", &index), ["ABCDE"]); // Dist 1
        assert_eq!(similar_types("xBCDEF", &index), ["ABCDE"]); // Dist 2 (x->A, F deleted)
        assert!(similar_types("xyCDEF", &index).is_empty()); // Dist 3 > Limit 2
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("a", ""), 1);
        assert_eq!(edit_distance("", "a"), 1);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "ab"), 1); // deletion
        assert_eq!(edit_distance("ab", "abc"), 1); // insertion
        assert_eq!(edit_distance("abc", "axc"), 1); // substitution
        assert_eq!(edit_distance("hello", "world"), 4);
        assert_eq!(edit_distance("café", "cafe"), 1); // unicode
    }
}
