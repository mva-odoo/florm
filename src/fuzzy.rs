/// Sublime-like fuzzy score: all pattern chars must appear in order.
/// Consecutive matches and matches at word starts score higher.
pub fn score(pattern: &str, cand: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }
    let pat: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).map(|c| c.to_ascii_lowercase()).collect();
    let mut pi = 0;
    let mut score = 0i32;
    let mut prev_match = false;
    let mut prev = '/';
    let mut first = None;
    for (i, c) in cand.chars().enumerate() {
        if pi == pat.len() {
            break;
        }
        if c.to_ascii_lowercase() == pat[pi] {
            score += 10;
            if prev_match {
                score += 15;
            }
            if matches!(prev, '/' | '\\' | '_' | '-' | '.' | ' ' | ':') || (prev.is_lowercase() && c.is_uppercase()) {
                score += 20;
            }
            first.get_or_insert(i);
            prev_match = true;
            pi += 1;
        } else {
            prev_match = false;
            score -= 1;
        }
        prev = c;
    }
    if pi < pat.len() {
        return None;
    }
    // Prefer hits in the file name over hits in directories.
    let name = cand.rsplit(['/', '\\']).next().unwrap_or(cand).to_lowercase();
    let flat: String = pat.iter().collect();
    if name.contains(&flat) {
        score += 60;
    }
    Some(score - first.unwrap_or(0) as i32 / 4)
}




