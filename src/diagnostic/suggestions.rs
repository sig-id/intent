//! Suggestions for error recovery.
//!
//! This module provides algorithms for suggesting corrections to
//! user mistakes, such as typos in identifiers.

/// Compute the Levenshtein distance between two strings.
///
/// The Levenshtein distance is the minimum number of single-character edits
/// (insertions, deletions, or substitutions) required to change one string
/// into the other.
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();

    // Handle empty string cases
    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    // Use two rows for space optimization
    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row: Vec<usize> = vec![0; b_len + 1];

    for (i, a_char) in a.chars().enumerate() {
        curr_row[0] = i + 1;

        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };

            curr_row[j + 1] = (prev_row[j + 1] + 1) // deletion
                .min(curr_row[j] + 1) // insertion
                .min(prev_row[j] + cost); // substitution
        }

        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[b_len]
}

/// Find the closest matching strings from a list of candidates.
///
/// Returns candidates sorted by distance, limited to those within max_distance.
pub fn find_closest(name: &str, candidates: &[&str], max_distance: usize) -> Vec<String> {
    let mut results: Vec<(usize, &str)> = candidates
        .iter()
        .map(|&candidate| {
            // Try both exact and case-insensitive matching
            let dist = if candidate.eq_ignore_ascii_case(name) {
                0
            } else {
                levenshtein_distance(&name.to_lowercase(), &candidate.to_lowercase())
            };
            (dist, candidate)
        })
        .filter(|(dist, _)| *dist <= max_distance)
        .collect();

    // Sort by distance, then alphabetically
    results.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });

    results.into_iter().map(|(_, s)| s.to_string()).collect()
}

/// Suggest corrections for an unknown identifier.
///
/// Returns the best suggestion if any candidates are close enough.
pub fn suggest_identifier(unknown: &str, known: &[&str]) -> Option<String> {
    if known.is_empty() {
        return None;
    }

    // Calculate max allowed distance based on length
    // Allow more mistakes in longer names
    let max_distance = (unknown.len() / 3).max(1).min(3);

    let closest = find_closest(unknown, known, max_distance);

    closest.into_iter().next()
}

/// Suggest multiple possible corrections for an unknown identifier.
///
/// Returns up to `max_suggestions` candidates that might be what the user meant.
pub fn suggest_multiple(unknown: &str, known: &[&str], max_suggestions: usize) -> Vec<String> {
    if known.is_empty() {
        return Vec::new();
    }

    let max_distance = (unknown.len() / 3).max(1).min(3);

    find_closest(unknown, known, max_distance)
        .into_iter()
        .take(max_suggestions)
        .collect()
}

/// Format a list of suggestions for display in an error message.
pub fn format_suggestions(suggestions: &[String]) -> String {
    match suggestions.len() {
        0 => String::new(),
        1 => format!("Did you mean '{}'?", suggestions[0]),
        2 => format!("Did you mean '{}' or '{}'?", suggestions[0], suggestions[1]),
        _ => {
            let last = suggestions.last().unwrap();
            let rest = &suggestions[..suggestions.len() - 1];
            format!(
                "Did you mean {}, or '{}'?",
                rest.iter()
                    .map(|s| format!("'{}'", s))
                    .collect::<Vec<_>>()
                    .join(", "),
                last
            )
        }
    }
}

/// Find similar keywords for a typo.
pub fn suggest_keyword(unknown: &str) -> Option<String> {
    const KEYWORDS: &[&str] = &[
        "system",
        "component",
        "behavior",
        "state",
        "transition",
        "event",
        "pattern",
        "constraint",
        "invariant",
        "property",
        "initial",
        "terminal",
        "on",
        "from",
        "to",
        "guard",
        "effect",
        "emit",
        "apply",
        "refines",
        "composes",
        "extends",
        "forall",
        "exists",
        "count",
        "always",
        "eventually",
        "next",
        "until",
        "release",
        "weak",
        "strong",
        "fairness",
        "weakly",
        "strongly",
        "self",
        "fork",
        "join",
        "if",
        "then",
        "else",
        "let",
        "in",
        "case",
        "choose",
        "assume",
        "import",
        "from",
        "as",
        "pub",
        "internal",
        "private",
        "nodes",
        "variables",
        "states",
        "transitions",
        "properties",
        "fair",
        "map",
        "strengthens",
        "with",
    ];

    suggest_identifier(unknown, KEYWORDS)
}

/// Find similar type names for a typo.
pub fn suggest_type(unknown: &str) -> Option<String> {
    const TYPES: &[&str] = &[
        "Int",
        "Float",
        "Bool",
        "String",
        "Duration",
        "Entity",
        "EntityList",
        "State",
        "Event",
        "EventList",
        "Component",
        "Behavior",
        "Pattern",
        "List",
        "Map",
        "Set",
        "Optional",
    ];

    suggest_identifier(unknown, TYPES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance_equal() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_levenshtein_distance_empty() {
        assert_eq!(levenshtein_distance("", "hello"), 5);
        assert_eq!(levenshtein_distance("hello", ""), 5);
    }

    #[test]
    fn test_levenshtein_distance_substitution() {
        assert_eq!(levenshtein_distance("hello", "hallo"), 1);
        assert_eq!(levenshtein_distance("hello", "hallo"), 1);
    }

    #[test]
    fn test_levenshtein_distance_insertion() {
        assert_eq!(levenshtein_distance("hello", "helllo"), 1);
    }

    #[test]
    fn test_levenshtein_distance_deletion() {
        assert_eq!(levenshtein_distance("hello", "helo"), 1);
    }

    #[test]
    fn test_find_closest() {
        let candidates = vec!["hello", "hallo", "help", "world"];
        let closest = find_closest("helo", &candidates, 2);
        assert!(closest.contains(&"hello".to_string()));
        assert!(closest.contains(&"helo".to_string()) || closest.contains(&"help".to_string()));
    }

    #[test]
    fn test_suggest_identifier() {
        let known = vec!["component", "behavior", "constraint", "state", "transition"];
        assert_eq!(
            suggest_identifier("componet", &known),
            Some("component".to_string())
        );
        assert_eq!(
            suggest_identifier("behvior", &known),
            Some("behavior".to_string())
        );
    }

    #[test]
    fn test_suggest_identifier_case_insensitive() {
        let known = vec!["Component", "Behavior", "State"];
        assert_eq!(
            suggest_identifier("component", &known),
            Some("Component".to_string())
        );
    }

    #[test]
    fn test_suggest_multiple() {
        // Use words that are closer together for multiple matches
        let known = vec!["component", "componet", "composite", "compute"];
        let suggestions = suggest_multiple("componett", &known, 3);
        assert!(!suggestions.is_empty());
        // "component" and "componet" should both match (distance 1 and 2)
        assert!(
            suggestions.contains(&"componet".to_string())
                || suggestions.contains(&"component".to_string())
        );
    }

    #[test]
    fn test_format_suggestions() {
        assert_eq!(format_suggestions(&[]), "");

        assert_eq!(
            format_suggestions(&["foo".to_string()]),
            "Did you mean 'foo'?"
        );

        assert_eq!(
            format_suggestions(&["foo".to_string(), "bar".to_string()]),
            "Did you mean 'foo' or 'bar'?"
        );

        assert_eq!(
            format_suggestions(&["foo".to_string(), "bar".to_string(), "baz".to_string()]),
            "Did you mean 'foo', 'bar', or 'baz'?"
        );
    }

    #[test]
    fn test_suggest_keyword() {
        assert_eq!(suggest_keyword("componet"), Some("component".to_string()));
        assert_eq!(suggest_keyword("behvior"), Some("behavior".to_string()));
        assert_eq!(suggest_keyword("trasition"), Some("transition".to_string()));
    }

    #[test]
    fn test_suggest_type() {
        assert_eq!(suggest_type("Inte"), Some("Int".to_string()));
        assert_eq!(suggest_type("Boool"), Some("Bool".to_string()));
    }
}
