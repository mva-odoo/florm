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
/// The tightest place where the pattern appears is scored (not the first letters found, which
/// may be scattered over a long path).
pub fn score(pattern: &str, cand: &str) -> Option<i32> {
    if pattern.is_ascii() && cand.is_ascii() {
        // file paths: bytes, without allocating (go to file scores up to a million of them)
        let pat: Vec<u8> = pattern.bytes().filter(|b| !b.is_ascii_whitespace()).map(|b| b.to_ascii_lowercase()).collect();
        let c = cand.as_bytes();
        let name = cand.rfind(['/', '\\']).map_or(0, |i| i + 1);
        return score_seq(&pat, c.len(), name, |i| c[i].to_ascii_lowercase(), |i| {
            let (p, x) = (c[i - 1], c[i]);
            is_sep(p as char) || (p.is_ascii_lowercase() && x.is_ascii_uppercase())
        }, |b| b.is_ascii_alphanumeric());
    }
    let pat: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).map(fold).collect();
    let orig: Vec<char> = cand.chars().collect();
    let low: Vec<char> = orig.iter().map(|c| fold(*c)).collect();
    let name = orig.iter().rposition(|c| matches!(c, '/' | '\\')).map_or(0, |i| i + 1);
    score_seq(&pat, low.len(), name, |i| low[i], |i| {
        let (p, x) = (orig[i - 1], orig[i]);
        is_sep(p) || (p.is_lowercase() && x.is_uppercase())
    }, |c| c.is_alphanumeric())
}

fn is_sep(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ' | ':')
}

/// `at(i)`: folded unit i of the candidate (`n` of them, the file name from `name_start`);
/// `word_start(i)` (i > 0): a word starts there; `alnum`: letters and digits (whole words).
fn score_seq<T: Copy + PartialEq>(
    pat: &[T],
    n: usize,
    name_start: usize,
    at: impl Fn(usize) -> T,
    word_start: impl Fn(usize) -> bool,
    alnum: impl Fn(T) -> bool,
) -> Option<i32> {
    if pat.is_empty() {
        return Some(0);
    }
    // end of the first complete match (none: rejected)…
    let mut pi = 0;
    let mut end = None;
    for i in 0..n {
        if at(i) == pat[pi] {
            pi += 1;
            if pi == pat.len() {
                end = Some(i);
                break;
            }
        }
    }
    let end = end?;
    // …then back to the latest start: the tightest window
    let mut pi = pat.len();
    let mut start = end;
    for i in (0..=end).rev() {
        if at(i) == pat[pi - 1] {
            pi -= 1;
            if pi == 0 {
                start = i;
                break;
            }
        }
    }
    let mut score = 0i32;
    let mut pi = 0;
    let mut prev_match = false;
    for i in start..=end {
        if pi < pat.len() && at(i) == pat[pi] {
            score += 10;
            if prev_match {
                score += 15;
            }
            if i == 0 || word_start(i) {
                score += 20;
            }
            prev_match = true;
            pi += 1;
        } else {
            prev_match = false;
            score -= 1;
        }
    }
    let contains_from = |from: usize| (from..(n + 1).saturating_sub(pat.len())).any(|i| (0..pat.len()).all(|k| at(i + k) == pat[k]));
    // Prefer hits in the file name over hits in directories.
    if contains_from(name_start) {
        score += 60;
    } else if contains_from(0) {
        score += 30;
    }
    // A whole word of the candidate typed as is ("theme" in "Choisir le thème…") comes first.
    let mut w = 0;
    let mut whole = false;
    for i in 0..=n {
        if i == n || !alnum(at(i)) {
            if i - w == pat.len() && (0..pat.len()).all(|k| at(w + k) == pat[k]) {
                whole = true;
                break;
            }
            w = i + 1;
        }
    }
    if whole {
        score += 40;
    }
    // shorter candidates first, all else equal
    Some(score - start as i32 / 4 - n as i32 / 16)
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

    #[test]
    fn tightest_match_wins() {
        // the letters of "syndic" are scattered in the first path, together in the second
        let scattered = score("syndic", "addons/stock/yaml/ndata/import/cron.py").unwrap();
        let tight = score("syndic", "addons/syndic/models/syndic.py").unwrap();
        assert!(tight > scattered);
        assert!(score("syndicmodels", "addons/syndic/models/building.py").is_some());
        assert!(score("syndicx", "addons/syndic/models/building.py").is_none());
    }
}
