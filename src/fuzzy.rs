/// Lowercase letter without its accent: "thème" and "theme" match each other.
pub fn fold(c: char) -> char {
    let c = c.to_lowercase().next().unwrap_or(c);
    match c {
        'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' | 'í' | 'ì' => 'i',
        'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
        'ù' | 'û' | 'ü' | 'ú' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        'ÿ' => 'y',
        c => c,
    }
}

/// Sublime-like fuzzy score: all pattern chars must appear in order.
/// Consecutive matches and matches at word starts score higher. Case and accents are ignored.
pub fn score(pattern: &str, cand: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }
    let pat: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).map(fold).collect();
    if pat.is_empty() {
        return Some(0);
    }
    let mut pi = 0;
    let mut score = 0i32;
    let mut prev_match = false;
    let mut prev = '/';
    let mut first = None;
    for (i, c) in cand.chars().enumerate() {
        if pi == pat.len() {
            break;
        }
        if fold(c) == pat[pi] {
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
    let name: String = cand.rsplit(['/', '\\']).next().unwrap_or(cand).chars().map(fold).collect();
    let flat: String = pat.iter().collect();
    if name.contains(&flat) {
        score += 60;
    }
    // A whole word of the candidate typed as is ("theme" in "Choisir le thème…") comes first.
    let words: String = cand.chars().map(fold).collect();
    if words.split(|c: char| !c.is_alphanumeric()).any(|w| w == flat) {
        score += 40;
    }
    Some(score - first.unwrap_or(0) as i32 / 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_accents_and_case() {
        assert!(score("theme", "Choisir le thème…").is_some());
        assert!(score("THEME", "Choisir le thème…").is_some());
        assert!(score("thème", "Choisir le theme").is_some());
        assert!(score("prefs", "Préférences").is_some());
        assert!(score("xyz", "Préférences").is_none());
    }

    #[test]
    fn whole_words_and_file_names_first() {
        let a = score("theme", "Choisir le thème…").unwrap();
        let b = score("theme", "Afficher toutes les erreurs du module").unwrap_or(i32::MIN);
        assert!(a > b);
        let f = score("app", "src/app.rs").unwrap();
        let d = score("app", "app/src/main.rs").unwrap();
        assert!(f > d);
    }
}
