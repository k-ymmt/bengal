/// Compute Levenshtein edit distance between two strings.
///
/// Uses a 2-row DP matrix for O(n*m) time, O(min(n,m)) space.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    // Ensure b is the shorter string to minimise memory.
    if a_bytes.len() < b_bytes.len() {
        return edit_distance(b, a);
    }

    let b_len = b_bytes.len();
    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0; b_len + 1];

    for (i, &a_ch) in a_bytes.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &b_ch) in b_bytes.iter().enumerate() {
            let cost = if a_ch == b_ch { 0 } else { 1 };
            curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b_len]
}

/// Find the best match for `name` among `candidates`.
///
/// Returns `Some(candidate)` if the edit distance between `name` and the best
/// candidate is at most `max(name.len(), candidate.len()) / 3`.
/// Ties go to the first match found.
pub fn find_suggestion<'a>(
    name: &str,
    candidates: impl Iterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;

    for candidate in candidates {
        let dist = edit_distance(name, candidate);
        let max_len = name.len().max(candidate.len());
        let threshold = max_len / 3;
        if dist <= threshold {
            match best {
                Some((_, best_dist)) if dist < best_dist => {
                    best = Some((candidate, dist));
                }
                None => {
                    best = Some((candidate, dist));
                }
                _ => {}
            }
        }
    }

    best.map(|(s, _)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_distance_empty() {
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn test_edit_distance_identical() {
        assert_eq!(edit_distance("abc", "abc"), 0);
    }

    #[test]
    fn test_edit_distance_substitution() {
        assert_eq!(edit_distance("abc", "abd"), 1);
    }

    #[test]
    fn test_edit_distance_insertion() {
        assert_eq!(edit_distance("abc", "abcd"), 1);
    }

    #[test]
    fn test_edit_distance_kitten_sitting() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn test_find_suggestion_close() {
        let candidates = vec!["foo", "bar"];
        assert_eq!(find_suggestion("fo", candidates.into_iter()), Some("foo"));
    }

    #[test]
    fn test_find_suggestion_too_distant() {
        let candidates = vec!["foo", "bar"];
        assert_eq!(find_suggestion("xyz", candidates.into_iter()), None);
    }

    #[test]
    fn test_find_suggestion_exact_match() {
        let candidates = vec!["hello", "world"];
        assert_eq!(
            find_suggestion("hello", candidates.into_iter()),
            Some("hello")
        );
    }

    #[test]
    fn test_find_suggestion_picks_best() {
        let candidates = vec!["foobar", "foobaz", "foobax"];
        // "foobar" has distance 1 from "foobar", "foobaz" has dist 1 too,
        // but first match wins for ties
        assert_eq!(
            find_suggestion("foobar", candidates.into_iter()),
            Some("foobar")
        );
    }

    #[test]
    fn test_find_suggestion_single_char() {
        // For very short strings, threshold = max(1,3)/3 = 1, so dist 1 is OK
        let candidates = vec!["foo", "bar"];
        assert_eq!(find_suggestion("baz", candidates.into_iter()), Some("bar"));
    }
}
