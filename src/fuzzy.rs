// ── Fuzzy Matching ───────────────────────────────────────────────────

/// Simple fuzzy string matching with scoring.
/// Returns Some(score) if the query matches the target, None otherwise.
/// Higher scores indicate better matches.
///
/// Scoring for substring matching:
/// - Exact match: 100 points
/// - Case-insensitive substring match: score based on match position and length
pub fn fuzzy_match(query: &str, target: &str) -> Option<usize> {
    let query_lower = query.to_lowercase();
    let target_lower = target.to_lowercase();

    // Empty query matches everything with low score
    if query.is_empty() {
        return Some(0);
    }

    // Exact match
    if query_lower == target_lower {
        return Some(100);
    }

    // Substring match
    if let Some(pos) = target_lower.find(&query_lower) {
        // Score based on position (earlier is better) and length (longer is better)
        let position_score = 50 - (pos * 5); // Earlier matches get higher score
        let length_score = query.len() * 10; // Longer matches get higher score
        let score = position_score.max(0) + length_score;

        return Some(score.min(95));
    }

    None
}

/// Filter a list of items by fuzzy matching against a query.
/// Returns a list of (index, score) tuples sorted by score (descending).
pub fn fuzzy_filter<T: AsRef<str>>(items: &[T], query: &str) -> Vec<(usize, usize)> {
    let mut matches: Vec<(usize, usize)> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        if let Some(score) = fuzzy_match(query, item.as_ref()) {
            matches.push((idx, score));
        }
    }

    // Sort by score descending
    matches.sort_by(|a, b| b.1.cmp(&a.1));

    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert_eq!(fuzzy_match("copychat", "copychat"), Some(100));
    }

    #[test]
    fn test_prefix_match() {
        assert_eq!(fuzzy_match("copy", "copychat"), Some(130));
    }

    #[test]
    fn test_substring_match() {
        // "ain" should match "main" substring
        assert!(fuzzy_match("ain", "main").is_some());
        assert!(fuzzy_match("ain", "test_main.rs").is_some());
    }

    #[test]
    fn test_no_match() {
        assert_eq!(fuzzy_match("xyz", "copychat"), None);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(fuzzy_match("MAIN", "main.rs"), Some(130));
        assert_eq!(fuzzy_match("Main", "main.rs"), Some(130));
    }

    #[test]
    fn test_empty_query() {
        assert_eq!(fuzzy_match("", "copychat"), Some(0));
    }
}
