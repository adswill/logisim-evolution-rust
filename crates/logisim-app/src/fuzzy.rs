//! Small fuzzy matcher for the command palette and the toolbox filter.

/// Score `text` against `query` (case-insensitive subsequence match). `None` = no match.
/// Higher is better: prefix and word-start hits and consecutive runs score more.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let tl: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    if tl.len() != t.len() {
        // exotic case mapping: fall back to a plain substring test
        return text
            .to_lowercase()
            .contains(&q.iter().collect::<String>())
            .then_some(1);
    }
    let mut score = 0;
    let mut qi = 0;
    let mut prev_match: Option<usize> = None;
    for (i, c) in tl.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let word_start = i == 0
                || !t[i - 1].is_alphanumeric()
                || (t[i - 1].is_lowercase() && t[i].is_uppercase());
            score += 10;
            if word_start {
                score += 15;
            }
            if i == 0 {
                score += 10;
            }
            if prev_match == Some(i.wrapping_sub(1)) {
                score += 12; // consecutive
            }
            prev_match = Some(i);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    // shorter texts rank above longer ones for the same match quality
    Some(score * 4 - (t.len() as i32).min(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_and_case() {
        assert!(score("and", "AND Gate").is_some());
        assert!(score("ag", "AND Gate").is_some());
        assert!(score("xyz", "AND Gate").is_none());
        assert!(score("", "anything").is_some());
        assert!(score("A N D", "and").is_some()); // spaces in the query are ignored
    }

    #[test]
    fn better_matches_rank_higher() {
        let and = score("and", "AND Gate").unwrap();
        let nand = score("and", "NAND Gate").unwrap();
        let scattered = score("and", "Pull Resistor Demand").unwrap();
        assert!(and > nand, "prefix beats inner match");
        assert!(nand > scattered || and > scattered);
        assert!(score("tu", "Tunnel").unwrap() > score("tu", "Setup tool").unwrap());
        // shorter wins on equal quality
        assert!(score("ro", "Rotate").unwrap() > score("ro", "Rotate selected parts").unwrap());
    }

    #[test]
    fn word_starts_help() {
        assert!(score("sg", "Show grid").unwrap() > score("sg", "Disguise").unwrap());
    }
}
