//! Text helpers that are safe on any UTF-8 input.
//!
//! Every place that shortens a string for display goes through here, so
//! nothing slices a `str` at a byte offset that might fall inside a
//! multi-byte character.

/// Number of characters (Unicode scalar values) in `s`.
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// The first `max` characters of `s`, with no marker.
pub fn take_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Shorten `s` to at most `max` characters, ending in `...` when it had
/// to cut. Inputs that already fit come back unchanged.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if char_len(s) <= max {
        return s.to_string();
    }
    if max <= 3 {
        return ".".repeat(max);
    }
    let mut out = take_chars(s, max - 3);
    out.push_str("...");
    out
}

/// Whether any of `fields` contains `query`, ignoring case. An empty
/// query matches everything. Used by every list's `/` search.
pub fn matches_query(query: &str, fields: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&q))
}

/// Turn `snake_case`, `camelCase` or `kebab` words into "Title Case".
pub fn normalize_label(value: &str) -> String {
    let mut spaced = String::with_capacity(value.len() + 4);
    let mut prev_lower = false;
    for ch in value.chars() {
        if ch.is_uppercase() && prev_lower {
            spaced.push(' ');
        }
        prev_lower = ch.is_lowercase();
        spaced.push(ch);
    }
    spaced
        .replace('_', " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word: String = first.to_uppercase().collect();
                    word.push_str(&chars.as_str().to_lowercase());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_marks_cut_strings() {
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(char_len(&truncate("hello world", 8)), 8);
    }

    #[test]
    fn truncate_never_splits_a_character() {
        // Each of these is several bytes; byte slicing at 27 would panic
        // on some of them.
        let s = "é".repeat(40);
        let out = truncate(&s, 30);
        assert_eq!(char_len(&out), 30);
        assert!(out.ends_with("..."));

        let emoji = "🔋🔋🔋🔋🔋🔋🔋🔋";
        assert_eq!(truncate(emoji, 5), "🔋🔋...");
        assert_eq!(take_chars(emoji, 2), "🔋🔋");
    }

    #[test]
    fn truncate_tiny_widths() {
        assert_eq!(truncate("hello", 0), "");
        assert_eq!(truncate("hello", 2), "..");
    }

    #[test]
    fn normalize_label_handles_cases() {
        assert_eq!(normalize_label("hvac_action"), "Hvac Action");
        assert_eq!(normalize_label("targetValue"), "Target Value");
        assert_eq!(normalize_label("ÉTAT_ok"), "État Ok");
    }
}
