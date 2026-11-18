//! Spell checking of what is written for people: the Markdown files, the comments of the code,
//! the commit messages. French and English together (a word of either is right), from the word
//! hunspell dictionaries of npm (`dictionary-fr`: Grammalecte's, `dictionary-en`), downloaded
//! once into Florm's data folder, unfolded into all their words and kept sorted in one block of
//! text (searched by halves: little memory). The words added by the user go to a dictionary of their own (`dictionary.txt`, in the
//! shared configuration when there is one).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// A word not found, at (line, column in chars), its length in chars.
#[derive(Clone, Debug, PartialEq)]
pub struct Misspelled {
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub word: String,
}

/// Words sorted by bytes, one per line, and where each one starts: the text of a list as it is
/// on the disk (no allocation per word).
struct List {
    text: String,
    starts: Vec<u32>,
}

impl List {
    fn of_text(text: String) -> List {
        let mut starts = Vec::with_capacity(text.len() / 9);
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            if !line.trim_end_matches('\n').is_empty() {
                starts.push(at as u32);
            }
            at += line.len();
        }
        List { text, starts }
    }

    fn word(&self, k: usize) -> &str {
        let a = self.starts[k] as usize;
        let rest = &self.text[a..];
        &rest[..rest.find('\n').unwrap_or(rest.len())]
    }

    fn has(&self, w: &str) -> bool {
        let (mut lo, mut hi) = (0, self.starts.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.word(mid).cmp(w) {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => return true,
            }
        }
        false
    }
}

/// The lists (shared: adding a word of the user does not copy them) and the words of the user.
pub struct Dict {
    lists: Arc<Vec<List>>,
    mine: HashSet<String>,
}

/// Longer than this, a run of letters is no word: left alone (and no suggestion is looked for).
const LONGEST: usize = 40;

impl Dict {
    #[cfg(test)]
    fn from_words(mut words: Vec<String>, mine: HashSet<String>) -> Dict {
        words.sort_unstable();
        words.dedup();
        Dict { lists: Arc::new(vec![List::of_text(words.join("\n"))]), mine }
    }

    fn has(&self, w: &str) -> bool {
        self.mine.contains(w) || self.lists.iter().any(|l| l.has(w))
    }

    /// The word is right: as written, in lowercase (a word at the start of a sentence), with its
    /// French elisions (`l'`, `qu'`…) or English contraction (`n't`, `'s`…) taken off.
    pub fn knows(&self, w: &str) -> bool {
        if w.chars().count() > LONGEST {
            return true;
        }
        let w: std::borrow::Cow<str> = if w.contains('’') { w.replace('’', "'").into() } else { w.into() };
        if self.has(&w) {
            return true;
        }
        let lower = w.to_lowercase();
        if lower != *w && self.has(&lower) {
            return true;
        }
        // the words with an apostrophe of their own (the lists have none)
        if ["aujourd'hui", "prud'homme", "prud'hommes"].contains(&lower.as_str()) {
            return true;
        }
        const ELISIONS: [&str; 15] = ["l", "d", "j", "m", "n", "s", "t", "c", "qu", "jusqu", "lorsqu", "puisqu", "quoiqu", "quelqu", "presqu"];
        // the elisions taken off one after the other (« qu'l'… » would be odd but stays a loop)
        let mut rest = lower.as_str();
        while let Some((head, tail)) = rest.split_once('\'') {
            if !ELISIONS.contains(&head) {
                break;
            }
            if tail.is_empty() || self.has(tail) {
                return true;
            }
            rest = tail;
        }
        if lower.contains('\'') {
            const CONTRACTIONS: [&str; 7] = ["n't", "'s", "'re", "'ll", "'ve", "'d", "'m"];
            for c in CONTRACTIONS {
                if let Some(base) = lower.strip_suffix(c) {
                    if self.has(base) || base == "can" || base == "won" || base == "ca" {
                        return true;
                    }
                }
            }
            if lower.strip_suffix('\'').is_some_and(|head| self.has(head)) {
                return true;
            }
        }
        // a compound word: each part known (« peut-être » is in the lists, « re-run » is not)
        if lower.contains('-') {
            return lower.split('-').all(|p| p.chars().count() < 3 || self.has(p));
        }
        false
    }

    /// Up to `n` words close to `w` (one change, else two), closest in length first.
    pub fn suggest(&self, w: &str, n: usize) -> Vec<String> {
        let lower = w.to_lowercase();
        if lower.chars().count() > LONGEST {
            return Vec::new();
        }
        let capital = w.chars().next().is_some_and(char::is_uppercase);
        let mut found: Vec<String> = edits(&lower).into_iter().filter(|c| self.has(c)).collect();
        // two changes: the second one only a letter removed or two swapped (all the changes twice
        // over is hundreds of thousands of words to look up: a pause in the editor)
        if found.is_empty() {
            for e in edits(&lower) {
                found.extend(small_edits(&e).into_iter().filter(|c| self.has(c)));
            }
        }
        found.sort();
        found.dedup();
        let len = lower.chars().count() as i64;
        let plain = strip_accents(&lower);
        // the same word but its accents first, then a letter typed twice, then the closest length
        let doubled = |c: &str| {
            let w: Vec<char> = lower.chars().collect();
            (1..w.len()).any(|i| w[i] == w[i - 1] && w[..i].iter().chain(&w[i + 1..]).collect::<String>() == c)
        };
        found.sort_by_key(|c| (strip_accents(c) != plain, !doubled(c), (c.chars().count() as i64 - len).abs(), c.chars().next() != lower.chars().next()));
        found.truncate(n);
        if capital {
            found = found.into_iter().map(|c| capitalize(&c)).collect();
        }
        found
    }
}

fn strip_accents(w: &str) -> String {
    w.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            c => c,
        })
        .collect()
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// The words a letter removed, or two letters swapped, away from `w`.
fn small_edits(w: &str) -> Vec<String> {
    let chars: Vec<char> = w.chars().collect();
    let mut out = Vec::with_capacity(chars.len() * 2);
    for i in 0..chars.len() {
        out.push(chars[..i].iter().chain(&chars[i + 1..]).collect());
        if i + 1 < chars.len() {
            let mut s = chars.clone();
            s.swap(i, i + 1);
            out.push(s.into_iter().collect());
        }
    }
    out
}

/// The words one change away from `w`: a letter removed, swapped with the next, replaced, added.
fn edits(w: &str) -> Vec<String> {
    const LETTERS: &str = "abcdefghijklmnopqrstuvwxyzàâäçéèêëîïôöùûüœæ";
    let chars: Vec<char> = w.chars().collect();
    let mut out = Vec::new();
    for i in 0..=chars.len() {
        if i < chars.len() {
            out.push(chars[..i].iter().chain(&chars[i + 1..]).collect());
            if i + 1 < chars.len() {
                let mut s = chars.clone();
                s.swap(i, i + 1);
                out.push(s.into_iter().collect());
            }
            for l in LETTERS.chars().filter(|l| *l != chars[i]) {
                let mut s = chars.clone();
                s[i] = l;
                out.push(s.into_iter().collect());
            }
        }
        for l in LETTERS.chars() {
            let mut s = chars.clone();
            s.insert(i, l);
            out.push(s.into_iter().collect());
        }
    }
    out
}

// ------------------------------------------------------------------ the words of the lists

/// The hunspell dictionaries of npm (Grammalecte's French, MPL 2.0; SCOWL's English, MIT/BSD).
const LISTS: [(&str, &str); 2] = [("dictionary-fr", "3.0.0"), ("dictionary-en", "4.0.0")];

fn lists_dir() -> PathBuf {
    crate::config::data_dir().join("spell")
}

/// The dictionary of the user (their added words), one word per line.
pub fn my_words_path() -> PathBuf {
    crate::shared::sub("dictionary.txt").unwrap_or_else(|| crate::config::config_dir().join("dictionary.txt"))
}

fn my_words() -> HashSet<String> {
    std::fs::read_to_string(my_words_path()).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Adds `word` to the dictionary of the user (and to the one loaded).
pub fn add_word(word: &str) -> Result<(), String> {
    let path = my_words_path();
    let mut words = my_words();
    if !words.insert(word.to_string()) {
        return Ok(());
    }
    let mut sorted: Vec<&String> = words.iter().collect();
    sorted.sort();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, sorted.iter().map(|w| format!("{w}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    if let Ok(mut s) = state().lock() {
        if let Loaded::Ready(d) = &*s {
            let mut mine = d.mine.clone();
            mine.insert(word.to_string());
            *s = Loaded::Ready(Arc::new(Dict { lists: d.lists.clone(), mine }));
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ hunspell dictionaries unfolded

/// A rule of an affix: what is taken off the word, what is put instead, the affixes that may
/// follow it, and the condition on the word (a pattern of letters and `[…]` / `[^…]` classes).
struct Rule {
    strip: String,
    add: String,
    cont: Vec<String>,
    cond: Vec<(bool, Vec<char>)>,
}

struct Affix {
    prefix: bool,
    cross: bool,
    rules: Vec<Rule>,
}

/// The flags of `s` as the dictionary writes them (`FLAG long`: two characters each, `num`:
/// numbers separated by commas, else one character each).
fn flags(s: &str, mode: &str) -> Vec<String> {
    match mode {
        "long" => s.chars().collect::<Vec<_>>().chunks(2).map(|c| c.iter().collect()).collect(),
        "num" => s.split(',').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect(),
        _ => s.chars().map(String::from).collect(),
    }
}

/// `[^abc]` / `[abc]` / `x` / `.` of a condition, in order.
fn condition(c: &str) -> Vec<(bool, Vec<char>)> {
    if c == "." {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut it = c.chars().peekable();
    while let Some(ch) = it.next() {
        match ch {
            '[' => {
                let neg = it.peek() == Some(&'^');
                if neg {
                    it.next();
                }
                let set: Vec<char> = it.by_ref().take_while(|c| *c != ']').collect();
                out.push((!neg, set));
            }
            '.' => out.push((false, Vec::new())),
            c => out.push((true, vec![c])),
        }
    }
    out
}

fn fits(cond: &[(bool, Vec<char>)], chars: &[char]) -> bool {
    cond.len() <= chars.len() && cond.iter().zip(chars).all(|((yes, set), c)| if *yes { set.contains(c) } else { !set.contains(c) })
}

/// Every word a hunspell dictionary accepts (its roots with their prefixes and suffixes, two
/// suffixes in a row too), but the compounds; the prefixes with an apostrophe (`l'`, `qu'`) are
/// left to `Dict::knows`.
pub fn unfold(aff: &str, dic: &str) -> Vec<String> {
    let mut mode = String::new();
    let (mut needaffix, mut forbidden, mut only_compound) = (String::new(), String::new(), String::new());
    let mut affixes: std::collections::HashMap<String, Affix> = std::collections::HashMap::new();
    for line in aff.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.as_slice() {
            ["FLAG", m, ..] => mode = m.to_string(),
            ["NEEDAFFIX", f, ..] => needaffix = f.to_string(),
            ["FORBIDDENWORD", f, ..] => forbidden = f.to_string(),
            ["ONLYINCOMPOUND", f, ..] => only_compound = f.to_string(),
            [kind @ ("PFX" | "SFX"), flag, cross, n] if n.parse::<usize>().is_ok() && !affixes.contains_key(*flag) => {
                affixes.insert(flag.to_string(), Affix { prefix: *kind == "PFX", cross: *cross == "Y", rules: Vec::new() });
            }
            ["PFX" | "SFX", flag, strip, add, rest @ ..] => {
                let Some(a) = affixes.get_mut(*flag) else { continue };
                let (add, cont) = add.split_once('/').map_or((*add, ""), |(a, c)| (a, c));
                let cond = rest.first().copied().unwrap_or(".");
                let zero = |s: &str| if s == "0" { String::new() } else { s.to_string() };
                a.rules.push(Rule { strip: zero(strip), add: zero(add), cont: flags(cont, &mode), cond: condition(cond) });
            }
            _ => {}
        }
    }
    let apply = |word: &str, a: &Affix, out: &mut Vec<(String, Vec<String>)>| {
        let chars: Vec<char> = word.chars().collect();
        for r in &a.rules {
            if a.prefix {
                if r.add.contains(['\'', '’']) || !word.starts_with(&r.strip) || !fits(&r.cond, &chars) {
                    continue;
                }
                out.push((format!("{}{}", r.add, &word[r.strip.len()..]), r.cont.clone()));
            } else {
                let rev: Vec<char> = chars.iter().rev().copied().collect();
                let cond: Vec<(bool, Vec<char>)> = r.cond.iter().rev().cloned().collect();
                if !word.ends_with(&r.strip) || !fits(&cond, &rev) {
                    continue;
                }
                out.push((format!("{}{}", &word[..word.len() - r.strip.len()], r.add), r.cont.clone()));
            }
        }
    };
    let mut words = Vec::new();
    for line in dic.lines().skip(1) {
        let entry = line.split('\t').next().unwrap_or("").trim();
        // `word/FLAGS` (a `\/` is a slash of the word)
        let (word, fl) = match entry.replace("\\/", "\u{1}").split_once('/') {
            Some((w, f)) => (w.replace('\u{1}', "/"), flags(f.split_whitespace().next().unwrap_or(""), &mode)),
            None => (entry.split_whitespace().next().unwrap_or("").replace("\\/", "/"), Vec::new()),
        };
        if word.is_empty() || fl.iter().any(|f| *f == forbidden || *f == only_compound) {
            continue;
        }
        if !fl.contains(&needaffix) {
            words.push(word.clone());
        }
        // the suffixes, and the suffixes of those suffixes
        let mut suffixed: Vec<(String, Vec<String>, bool)> = Vec::new();
        for f in &fl {
            let Some(a) = affixes.get(f).filter(|a| !a.prefix) else { continue };
            let mut once = Vec::new();
            apply(&word, a, &mut once);
            for (form, cont) in once {
                let needs = cont.contains(&needaffix);
                for c in &cont {
                    if let Some(a2) = affixes.get(c).filter(|a2| !a2.prefix) {
                        let mut twice = Vec::new();
                        apply(&form, a2, &mut twice);
                        words.extend(twice.into_iter().map(|t| t.0));
                    }
                }
                if !needs {
                    words.push(form.clone());
                }
                suffixed.push((form, cont, a.cross));
            }
        }
        // the prefixes, on the word and on its suffixed forms when both cross
        for f in &fl {
            let Some(a) = affixes.get(f).filter(|a| a.prefix) else { continue };
            let mut pre = Vec::new();
            apply(&word, a, &mut pre);
            if a.cross {
                for (form, _, cross) in &suffixed {
                    if *cross {
                        apply(form, a, &mut pre);
                    }
                }
            }
            words.extend(pre.into_iter().map(|p| p.0));
        }
    }
    words
}

/// The lists, downloaded the first time (in the background: it reads and writes files).
pub fn load() -> Result<Dict, String> {
    let dir = lists_dir();
    let mut lists = Vec::new();
    for (package, version) in LISTS {
        let txt = dir.join(format!("{package}-{version}.txt"));
        if !txt.is_file() {
            let tmp = dir.join(package);
            let files = crate::npm::fetch(package, version, &tmp, 120)?;
            let read = |n: &str| std::fs::read_to_string(files.join(n)).map_err(|e| e.to_string());
            let mut words = unfold(&read("index.aff")?, &read("index.dic")?);
            words.sort_unstable();
            words.dedup();
            std::fs::write(&txt, words.join("\n")).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(&tmp);
        }
        // written sorted by bytes, as the search wants
        lists.push(List::of_text(std::fs::read_to_string(&txt).map_err(|e| e.to_string())?));
    }
    Ok(Dict { lists: Arc::new(lists), mine: my_words() })
}

pub enum Loaded {
    No,
    Loading,
    Ready(Arc<Dict>),
    Failed(String),
}

fn state() -> &'static Mutex<Loaded> {
    static S: OnceLock<Mutex<Loaded>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Loaded::No))
}

/// The dictionary when it is loaded; the first call starts loading it on a thread (`repaint`
/// when it is there).
pub fn dict(repaint: impl Fn() + Send + 'static) -> Option<Arc<Dict>> {
    let mut s = state().lock().ok()?;
    match &*s {
        Loaded::Ready(d) => Some(d.clone()),
        Loaded::Loading | Loaded::Failed(_) => None,
        Loaded::No => {
            *s = Loaded::Loading;
            std::thread::spawn(move || {
                let r = load();
                if let Ok(mut s) = state().lock() {
                    *s = match r {
                        Ok(d) => Loaded::Ready(Arc::new(d)),
                        Err(e) => Loaded::Failed(e),
                    };
                }
                repaint();
            });
            None
        }
    }
}

/// A small dictionary in place of the downloaded one, for the tests of the app.
#[cfg(test)]
pub fn use_words(words: &[&str]) {
    if let Ok(mut s) = state().lock() {
        *s = Loaded::Ready(Arc::new(Dict::from_words(words.iter().map(|w| w.to_string()).collect(), my_words())));
    }
}

/// Why the dictionary is not there (a failed download), once.
pub fn failure() -> Option<String> {
    match &*state().lock().ok()? {
        Loaded::Failed(e) => Some(e.clone()),
        _ => None,
    }
}

// ------------------------------------------------------------------ what is checked

/// What of a file is checked: all of it (Markdown, a commit message), or its comments.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Prose,
    Comments { line: &'static str, block: Option<(&'static str, &'static str)>, docstrings: bool },
}

/// What is checked in `path`, None: not checked.
pub fn kind_of(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_str()?;
    if name == "COMMIT_EDITMSG" || name == "MERGE_MSG" || name == "TAG_EDITMSG" {
        return Some(Kind::Prose);
    }
    let ext = path.extension()?.to_str()?.to_lowercase();
    let c = |line, block| Some(Kind::Comments { line, block, docstrings: false });
    match ext.as_str() {
        "md" | "markdown" | "txt" | "rst" => Some(Kind::Prose),
        "py" => Some(Kind::Comments { line: "#", block: None, docstrings: true }),
        "sh" | "bash" | "zsh" | "fish" | "rb" | "yml" | "yaml" | "toml" | "conf" | "cfg" | "ini" => c("#", None),
        "rs" | "js" | "mjs" | "cjs" | "ts" | "jsx" | "tsx" | "go" | "c" | "h" | "cc" | "cpp" | "hpp" | "java" | "kt" | "swift" | "cs" | "scss" | "less" | "dart" | "php" => c("//", Some(("/*", "*/"))),
        "css" => c("\u{0}", Some(("/*", "*/"))),
        "sql" | "lua" | "hs" => c("--", None),
        "xml" | "html" | "htm" | "vue" | "svelte" => c("\u{0}", Some(("<!--", "-->"))),
        _ => None,
    }
}

/// The parts of `text` to check, as (line, column in chars, the text), the rest left out.
fn parts(text: &str, kind: Kind) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    match kind {
        Kind::Prose => {
            let mut fence = false;
            for (i, line) in text.lines().enumerate() {
                let t = line.trim_start();
                if t.starts_with("```") || t.starts_with("~~~") {
                    fence = !fence;
                    continue;
                }
                // code (fenced or indented by 4), and the comments of a commit message
                if fence || line.starts_with("    ") || line.starts_with('\t') || line.starts_with('#') && !line.starts_with("# ") {
                    continue;
                }
                out.push((i, 0, prose_line(line)));
            }
        }
        Kind::Comments { line, block, docstrings } => {
            let mut inside: Option<&str> = None;
            for (i, l) in text.lines().enumerate() {
                let chars: Vec<char> = l.chars().collect();
                // where each char starts: the rest of the line is a slice, not a copy (a copy per
                // char made long lines quadratic)
                let offs: Vec<usize> = l.char_indices().map(|(b, _)| b).collect();
                let mut k = 0;
                while k < chars.len() {
                    let rest: &str = &l[offs[k]..];
                    if let Some(end) = inside {
                        match rest.find(end) {
                            Some(at) => {
                                let n = rest[..at].chars().count();
                                out.push((i, k, rest[..at].to_string()));
                                k += n + end.chars().count();
                                inside = None;
                            }
                            None => {
                                out.push((i, k, rest.to_string()));
                                break;
                            }
                        }
                        continue;
                    }
                    // a string: skipped (a comment marker in it is no comment)
                    let c = chars[k];
                    if docstrings && (rest.starts_with("\"\"\"") || rest.starts_with("'''")) {
                        inside = Some(if rest.starts_with("\"\"\"") { "\"\"\"" } else { "'''" });
                        k += 3;
                        continue;
                    }
                    if c == '"' || c == '\'' || c == '`' {
                        k += 1;
                        while k < chars.len() && chars[k] != c {
                            k += if chars[k] == '\\' { 2 } else { 1 };
                        }
                        k += 1;
                        continue;
                    }
                    if !line.starts_with('\u{0}') && rest.starts_with(line) {
                        let start = k + line.chars().count();
                        out.push((i, start, chars[start..].iter().collect()));
                        break;
                    }
                    if let Some((open, close)) = block.filter(|(o, _)| rest.starts_with(o)) {
                        inside = Some(close);
                        k += open.chars().count();
                        continue;
                    }
                    k += 1;
                }
            }
        }
    }
    out
}

/// A line of Markdown with what is no prose blanked (same length: the columns stay): inline
/// code, links' addresses, URLs, HTML tags.
fn prose_line(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out: Vec<char> = chars.clone();
    let blank = |out: &mut Vec<char>, a: usize, b: usize| out[a..b.min(chars.len())].iter_mut().for_each(|c| *c = ' ');
    let offs: Vec<usize> = line.char_indices().map(|(b, _)| b).collect();
    let mut k = 0;
    while k < chars.len() {
        let rest: &str = &line[offs[k]..];
        let end_of = |stop: &dyn Fn(char) -> bool| chars[k..].iter().position(|c| stop(*c)).map_or(chars.len(), |p| k + p);
        if chars[k] == '`' {
            let end = chars[k + 1..].iter().position(|c| *c == '`').map_or(chars.len(), |p| k + 1 + p + 1);
            blank(&mut out, k, end);
            k = end;
        } else if rest.starts_with("](") {
            let end = end_of(&|c| c == ')');
            blank(&mut out, k, end);
            k = end;
        } else if rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("www.") {
            let end = end_of(&|c| c.is_whitespace());
            blank(&mut out, k, end);
            k = end;
        } else if chars[k] == '<' && chars.get(k + 1).is_some_and(|c| c.is_alphabetic() || *c == '/' || *c == '!') {
            let end = end_of(&|c| c == '>') + 1;
            blank(&mut out, k, end);
            k = end;
        } else {
            k += 1;
        }
    }
    out.into_iter().collect()
}

/// The words of `text` (of the kind `kind`) that `dict` does not know. A word is a run of
/// letters (with `'` or `-` inside); left out: the ones touching a digit, `_`, `.`, `/`, `@` or
/// `\` (names, paths, addresses), with a capital inside (camelCase, ACRONYMS), of less than 3
/// letters, and the ones starting with a capital that the lists do not know (names).
pub fn check(text: &str, kind: Kind, dict: &Dict) -> Vec<Misspelled> {
    let mut out = Vec::new();
    for (line, col0, part) in parts(text, kind) {
        let chars: Vec<char> = part.chars().collect();
        let mut k = 0;
        while k < chars.len() {
            if !chars[k].is_alphabetic() {
                k += 1;
                continue;
            }
            let start = k;
            while k < chars.len() && (chars[k].is_alphabetic() || (matches!(chars[k], '\'' | '’' | '-') && chars.get(k + 1).is_some_and(|c| c.is_alphabetic()))) {
                k += 1;
            }
            let word: String = chars[start..k].iter().collect();
            let touches = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_digit() || matches!(c, '_' | '/' | '\\' | '@' | '#' | '$' | '%' | '=' | '<' | '>' | '{' | '}'));
            let dotted = chars.get(k) == Some(&'.') && chars.get(k + 1).is_some_and(|c| c.is_alphanumeric()) || start > 0 && chars[start - 1] == '.' && start > 1 && chars[start - 2].is_alphanumeric();
            if touches(chars.get(k)) || (start > 0 && touches(chars.get(start - 1))) || dotted {
                continue;
            }
            let n = word.chars().count();
            let inner_capital = word.chars().skip(1).any(char::is_uppercase);
            if n < 3 || inner_capital || dict.knows(&word) || word.chars().next().is_some_and(char::is_uppercase) {
                continue;
            }
            out.push(Misspelled { line, col: col0 + start, len: n, word });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        let words = ["le", "la", "eau", "fichier", "fichiers", "été", "peut-être", "the", "file", "does", "running", "aujourd'hui", "est", "fonction", "function", "this", "returns", "value", "café", "cafés"];
        Dict::from_words(words.iter().map(|s| s.to_string()).collect(), ["florm".to_string()].into())
    }

    #[test]
    fn words_known_and_not() {
        let d = dict();
        for w in ["fichier", "Fichier", "l'eau", "l’eau", "aujourd'hui", "doesn't", "peut-être", "florm", "Florm"] {
            assert!(d.knows(w), "{w}");
        }
        for w in ["fichierr", "fonctoin", "l'eaux"] {
            assert!(!d.knows(w), "{w}");
        }
        assert_eq!(d.suggest("fichierr", 3), ["fichier", "fichiers"]);
        assert_eq!(d.suggest("fonctoin", 3), ["fonction"]);
        assert_eq!(d.suggest("Cafe", 1), ["Café"]);
        // two changes: a letter replaced, then one more removed or swapped
        assert_eq!(d.suggest("fonctoinn", 3), ["fonction"]);
        // a trap of elisions (« l'l'l'…x »): no recursion, no crash
        let trap = "l'".repeat(100_000) + "x";
        let t = std::time::Instant::now();
        let _ = d.knows(&trap);
        let _ = d.knows(&format!("{}fichierr", "l'".repeat(19)));
        assert!(t.elapsed().as_millis() < 50);
        assert!(d.knows("l'l'fichier") && !d.knows("l'l'fichierr"));
        assert!(d.suggest(&"a".repeat(1_000_000), 3).is_empty());
    }

    #[test]
    fn markdown_and_comments() {
        let d = dict();
        let md = "Le fichierr est la.\n\n```\nfonctoin()\n```\nUn `fonctoin` et [le lien](https://exemple.fr/fonctoin) Odoo snake_case camelCase v2alpha fichier.py\n";
        let found = check(md, Kind::Prose, &d);
        let words: Vec<(usize, usize, &str)> = found.iter().map(|m| (m.line, m.col, m.word.as_str())).collect();
        // `Un`, `et`: two letters; Odoo: a name; code, addresses, identifiers left out
        assert_eq!(words, [(0, 3, "fichierr"), (5, 21, "lien")]);
        let rs = "let s = \"fonctoin in a string\"; // this functoin returns the value\n/* the fichierr */ fn f() {}\n";
        let found = check(rs, kind_of(Path::new("a.rs")).unwrap(), &d);
        let words: Vec<(usize, usize, &str)> = found.iter().map(|m| (m.line, m.col, m.word.as_str())).collect();
        assert_eq!(words, [(0, 40, "functoin"), (1, 7, "fichierr")]);
        let py = "x = 1  # le fichierr\ndef f():\n    \"\"\"Returns the valeu.\"\"\"\n    return '#fonctoin'\n";
        let found = check(py, kind_of(Path::new("a.py")).unwrap(), &d);
        let words: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
        assert_eq!(words, ["fichierr", "valeu"]);
    }

    #[test]
    fn a_hunspell_dictionary_unfolded() {
        let aff = "SET UTF-8\nFLAG long\nNEEDAFFIX ()\nPFX L' Y 1\nPFX L' 0 l' .\nSFX S. Y 2\nSFX S. 0 s [^sxz]\nSFX S. 0 0 [sxz]\nSFX a0 Y 3\nSFX a0 er er/()\nSFX a0 er é/S. er\nSFX a0 er ons er\n";
        let dic = "3\nfichier/S.L'\ndéboguer/a0\nnez/S.\n";
        let mut w = unfold(aff, dic);
        w.sort();
        // déboguer once (the form of its rule needs another affix), débogué + s, déboguons
        assert_eq!(w, ["déboguer", "déboguons", "débogué", "débogués", "fichier", "fichiers", "nez", "nez"]);
    }

    /// The real lists, downloaded: `cargo test spell_with_the_real_lists -- --ignored`.
    #[test]
    #[ignore]
    fn spell_with_the_real_lists() {
        let d = load().unwrap();
        for w in ["développeur", "mangeâmes", "conteneur", "l'équipe", "qu'il", "aujourd'hui", "doesn't", "debugger", "running", "color", "téléchargé", "identifiants", "débogueur"] {
            assert!(d.knows(w), "{w}");
        }
        assert!(!d.knows("dévelopeur"));
        assert!(d.suggest("dévelopeur", 3).contains(&"développeur".to_string()), "{:?}", d.suggest("dévelopeur", 3));
        let t = std::time::Instant::now();
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/guide.md")).unwrap();
        let found = check(&text, Kind::Prose, &d);
        eprintln!("{} fautes dans le guide, {:?}: {:?}", found.len(), t.elapsed(), found.iter().take(40).map(|m| &m.word).collect::<Vec<_>>());
    }
}
