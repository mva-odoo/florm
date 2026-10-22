//! Find in files: parallel walk (respects .gitignore), regex search per file.

use crate::document::build_regex;
use ignore::overrides::OverrideBuilder;
use ignore::WalkState;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const MAX_HITS: usize = 20_000;
const MAX_HITS_PER_FILE: usize = 1_000;
const MAX_FILE_SIZE: u64 = 8_000_000;

#[derive(Clone, Default)]
pub struct Query {
    pub pattern: String,
    pub case: bool,
    pub regex: bool,
    pub whole: bool,
    /// Comma separated globs, e.g. "*.py, *.xml".
    pub include: String,
    /// Comma separated globs, e.g. "node_modules, *.min.js".
    pub exclude: String,
    /// Replace keeping the case of each match (chat→chien, Chat→Chien, CHAT→CHIEN).
    pub preserve: bool,
}

#[derive(Clone)]
pub struct Hit {
    pub line: usize,
    pub col: usize,
    pub len: usize,
    /// Trimmed line text, with the match at preview[start..end] (bytes).
    pub preview: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone)]
pub struct FileHits {
    pub path: PathBuf,
    pub rel: String,
    pub hits: Vec<Hit>,
    /// Some hits were dismissed: replace only the remaining ones.
    pub partial: bool,
}

#[derive(Default)]
pub struct Outcome {
    pub files: Vec<FileHits>,
    pub scanned: usize,
    pub truncated: bool,
    pub millis: u128,
    pub error: Option<String>,
}

fn globs(s: &str) -> impl Iterator<Item = &str> {
    s.split(',').map(str::trim).filter(|g| !g.is_empty())
}

/// Turns a filter as people write it into gitignore-style globs, relative to `root`:
/// - "syndic/*.py" matches the .py files of any "syndic" folder, at any depth and in its subfolders;
/// - "syndic" matches a file or a whole folder with that name, anywhere;
/// - "./…" and absolute paths (inside `root`) are anchored to the root.
/// Exclusions (`include == false`) only get the "anywhere" part: an excluded folder is skipped whole.
fn normalize(root: &Path, glob: &str, include: bool) -> Result<Vec<String>, String> {
    let (neg, g) = match glob.strip_prefix('!') {
        Some(g) => ("!", g),
        None => ("", glob),
    };
    let mut g = g.replace('\\', "/");
    let mut anchored = false;
    if Path::new(&g).is_absolute() {
        let root_s = root.to_string_lossy().replace('\\', "/");
        match g.strip_prefix(root_s.trim_end_matches('/')) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => g = rest.to_string(),
            _ => return Err(format!("« {glob} » est en dehors du dossier ouvert ({})", root.display())),
        }
        anchored = true;
    } else if let Some(rest) = g.strip_prefix("./") {
        g = rest.to_string();
        anchored = true;
    } else if g.starts_with('/') {
        anchored = true;
    }
    let g = g.trim_matches('/');
    if g.is_empty() || g == "**" {
        return Ok(vec![format!("{neg}**")]);
    }
    let (dir, file) = g.rsplit_once('/').unwrap_or(("", g));
    let is_glob = |t: &str| t.contains(['*', '?', '[', '{']);
    // prefix that places the pattern at the root, or at any depth
    let pre = |p: &str| if anchored { "/" } else if p.starts_with("**") { "" } else { "**/" };
    let out = if !is_glob(file) {
        // a file or a folder name
        let file_pat = if !anchored && dir.is_empty() { g.to_string() } else { format!("{}{g}", pre(g)) };
        if include {
            vec![file_pat, format!("{}{g}/**", pre(g))]
        } else {
            vec![file_pat]
        }
    } else if dir.is_empty() {
        vec![format!("{}{file}", if anchored { "/" } else { "" })]
    } else if include {
        let sub = if dir.ends_with("**") { "" } else { "/**" };
        vec![format!("{}{dir}{sub}/{file}", pre(dir))]
    } else {
        vec![format!("{}{g}", pre(dir))]
    };
    Ok(out.into_iter().map(|p| format!("{neg}{p}")).collect())
}

/// Searches `root`; stops early when `current` no longer equals `gen` (a newer search started).
pub fn run(root: &Path, q: &Query, ex: &crate::walk::Exclusions, gen: u64, current: &Arc<AtomicU64>) -> Outcome {
    let start = Instant::now();
    let fail = |e: String| Outcome { error: Some(e), ..Default::default() };
    let re = match build_regex(&q.pattern, q.case, q.regex, q.whole) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let mut ob = OverrideBuilder::new(root);
    for g in globs(&q.include) {
        let pats = match normalize(root, g, true) {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        for g in pats {
            if let Err(e) = ob.add(&g) {
                return fail(e.to_string());
            }
        }
    }
    for g in globs(&q.exclude) {
        let pats = match normalize(root, g.trim_start_matches('!'), false) {
            Ok(p) => p,
            Err(e) => return fail(e),
        };
        for g in pats {
            if let Err(e) = ob.add(&format!("!{g}")) {
                return fail(e.to_string());
            }
        }
    }
    let overrides = match ob.build() {
        Ok(o) => o,
        Err(e) => return fail(e.to_string()),
    };
    let results = Mutex::new(Vec::new());
    let total = AtomicUsize::new(0);
    let scanned = AtomicUsize::new(0);
    crate::walk::walker(root, ex, Vec::new())
        .overrides(overrides)
        .build_parallel()
        .run(|| {
            let (re, results, total, scanned) = (&re, &results, &total, &scanned);
            Box::new(move |entry| {
                if current.load(Ordering::Relaxed) != gen || total.load(Ordering::Relaxed) >= MAX_HITS {
                    return WalkState::Quit;
                }
                let Ok(e) = entry else { return WalkState::Continue };
                if !e.file_type().is_some_and(|t| t.is_file()) {
                    return WalkState::Continue;
                }
                scanned.fetch_add(1, Ordering::Relaxed);
                if let Some(fh) = search_file(e.path(), root, re) {
                    total.fetch_add(fh.hits.len(), Ordering::Relaxed);
                    results.lock().unwrap().push(fh);
                }
                WalkState::Continue
            })
        });
    let mut files = results.into_inner().unwrap();
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let total = total.into_inner();
    Outcome {
        files,
        scanned: scanned.into_inner(),
        truncated: total >= MAX_HITS,
        millis: start.elapsed().as_millis(),
        error: None,
    }
}

fn search_file(path: &Path, root: &Path, re: &regex::Regex) -> Option<FileHits> {
    if std::fs::metadata(path).ok()?.len() > MAX_FILE_SIZE {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let mut hits = Vec::new();
    let (mut line, mut line_start, mut scanned_to) = (0, 0, 0);
    for m in re.find_iter(text) {
        if m.start() == m.end() {
            continue;
        }
        for (i, b) in text.as_bytes()[scanned_to..m.start()].iter().enumerate() {
            if *b == b'\n' {
                line += 1;
                line_start = scanned_to + i + 1;
            }
        }
        scanned_to = m.start();
        let line_end = text[line_start..].find('\n').map_or(text.len(), |i| line_start + i);
        let (preview, start, end) =
            preview(&text[line_start..line_end], m.start() - line_start, m.end().min(line_end) - line_start);
        hits.push(Hit {
            line,
            col: text[line_start..m.start()].chars().count(),
            len: m.as_str().chars().count(),
            preview,
            start,
            end,
        });
        if hits.len() >= MAX_HITS_PER_FILE {
            break;
        }
    }
    if hits.is_empty() {
        return None;
    }
    let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().into_owned();
    Some(FileHits { path: path.to_path_buf(), rel, hits, partial: false })
}

/// Shortens a line around the match: leading whitespace removed, bounded context.
fn preview(line: &str, s: usize, e: usize) -> (String, usize, usize) {
    let lead = line.len() - line.trim_start().len();
    let mut start = lead.min(s);
    if s - start > 80 {
        start = s - 80;
        while !line.is_char_boundary(start) {
            start += 1;
        }
    }
    let mut end = line.len();
    if end - e > 200 {
        end = e + 200;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
    }
    (line[start..end].to_string(), s - start, e.min(end) - start)
}

/// Applies the replacement to a whole text; returns the new text and the number of replacements,
/// or None when nothing was replaced. With `keep`, only the matches starting at one of these
/// (line, char column) positions are replaced (the hits still listed in the results).
pub fn replace_text(
    re: &regex::Regex,
    text: &str,
    replacement: &str,
    regex_mode: bool,
    preserve: bool,
    keep: Option<&HashSet<(usize, usize)>>,
) -> Option<(String, usize)> {
    let mut out = String::with_capacity(text.len());
    let (mut last, mut count) = (0, 0);
    let (mut line, mut line_start, mut scanned_to) = (0, 0, 0);
    for caps in re.captures_iter(text) {
        let m = caps.get(0).unwrap();
        if let Some(keep) = keep {
            if m.start() == m.end() {
                continue;
            }
            for (i, b) in text.as_bytes()[scanned_to..m.start()].iter().enumerate() {
                if *b == b'\n' {
                    line += 1;
                    line_start = scanned_to + i + 1;
                }
            }
            scanned_to = m.start();
            if !keep.contains(&(line, text[line_start..m.start()].chars().count())) {
                continue;
            }
        }
        out.push_str(&text[last..m.start()]);
        out.push_str(&expand_replacement(&caps, replacement, regex_mode, preserve));
        last = m.end();
        count += 1;
    }
    if count == 0 {
        return None;
    }
    out.push_str(&text[last..]);
    Some((out, count))
}

/// What the matched text becomes, for the preview in the results.
pub fn replacement_preview(re: &regex::Regex, matched: &str, replacement: &str, regex_mode: bool, preserve: bool) -> String {
    match re.captures(matched) {
        Some(caps) => expand_replacement(&caps, replacement, regex_mode, preserve),
        None => unescape(replacement, regex_mode),
    }
}

/// What a match becomes: `replacement`, in regex mode with its groups ($1, $2…) and its escapes
/// (\n a line break, \t a tab, \\ a backslash); the case of the match kept when `preserve`.
/// A line break typed in the field (Maj+Entrée) is kept in both modes.
pub fn expand_replacement(caps: &regex::Captures, replacement: &str, regex_mode: bool, preserve: bool) -> String {
    let matched = caps.get(0).map_or("", |m| m.as_str());
    let mut new = String::new();
    if regex_mode {
        caps.expand(&unescape(replacement, true), &mut new);
    } else {
        new.push_str(replacement);
    }
    if preserve {
        new = crate::document::preserve_case(matched, &new);
    }
    new
}

/// \n, \t and \\ of a regex replacement turned into what they mean (other text left as is).
fn unescape(replacement: &str, regex_mode: bool) -> String {
    if !regex_mode || !replacement.contains('\\') {
        return replacement.to_string();
    }
    let mut out = String::with_capacity(replacement.len());
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_hits_with_filters() {
        let dir = std::env::temp_dir().join(format!("florm_search_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::write(dir.join("a.py"), "x = 1\nfoo = 2\n    foo()\n").unwrap();
        std::fs::write(dir.join("sub/b.xml"), "<foo/>\n").unwrap();
        std::fs::write(dir.join("node_modules/c.py"), "foo\n").unwrap();
        let gen = Arc::new(AtomicU64::new(1));
        let q = Query { pattern: "foo".into(), exclude: "node_modules".into(), ..Default::default() };
        let out = run(&dir, &q, &crate::walk::Exclusions::none(), 1, &gen);
        assert_eq!(out.files.len(), 2, "{:?}", out.files.iter().map(|f| &f.rel).collect::<Vec<_>>());
        assert_eq!(out.files.iter().map(|f| f.hits.len()).sum::<usize>(), 3);
        let a = &out.files[0];
        assert_eq!(a.rel, "a.py");
        assert_eq!((a.hits[1].line, a.hits[1].col), (2, 4));
        assert_eq!(&a.hits[1].preview[a.hits[1].start..a.hits[1].end], "foo");
        let q = Query { pattern: "foo".into(), include: "*.xml".into(), ..Default::default() };
        assert_eq!(run(&dir, &q, &crate::walk::Exclusions::none(), 1, &gen).files.len(), 1);
        let rels = |inc: &str| {
            let q = Query { pattern: "foo".into(), include: inc.into(), ..Default::default() };
            run(&dir, &q, &crate::walk::Exclusions::none(), 1, &gen).files.into_iter().map(|f| f.rel).collect::<Vec<_>>()
        };
        assert_eq!(rels("sub"), ["sub/b.xml"]);
        assert_eq!(rels(&format!("{}/sub/", dir.display())), ["sub/b.xml"]);
        assert_eq!(rels(&format!("{}/*.py", dir.display())), ["a.py"]);
        assert_eq!(rels("./**/*.xml"), ["sub/b.xml"]);
        assert_eq!(rels("./*.py"), ["a.py"]);
        let q = Query { pattern: "foo".into(), include: "/elsewhere/*.py".into(), ..Default::default() };
        assert!(run(&dir, &q, &crate::walk::Exclusions::none(), 1, &gen).error.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaces_only_kept_hits() {
        let re = build_regex("foo", false, false, false).unwrap();
        let text = "foo foo\n  foo\n";
        assert_eq!(replace_text(&re, text, "x", false, false, None), Some(("x x\n  x\n".into(), 3)));
        let keep: HashSet<_> = [(0, 4), (1, 2)].into();
        assert_eq!(replace_text(&re, text, "x", false, false, Some(&keep)), Some(("foo x\n  x\n".into(), 2)));
        let re = build_regex(r"f(o+)", false, true, false).unwrap();
        assert_eq!(replace_text(&re, "foo", "$1-", true, false, None), Some(("oo-".into(), 1)));
        assert_eq!(replacement_preview(&re, "foo", "<$1>", true, false), "<oo>");
        assert_eq!(replacement_preview(&re, "FOO", "bar", false, true), "BAR");
        assert_eq!(replace_text(&re, "bar", "x", true, false, None), None);
        // "\n" written in the text becomes a real line break (regex), a typed one is kept as is
        let nl = build_regex(r"\\n", false, true, false).unwrap();
        assert_eq!(replace_text(&nl, r"a\nb", r"\n", true, false, None), Some(("a\nb".into(), 1)));
        assert_eq!(replace_text(&nl, r"a\nb", r"\t|\\|\q", true, false, None), Some((r"a	|\|\qb".into(), 1)));
        let semi = build_regex(";", false, false, false).unwrap();
        assert_eq!(replace_text(&semi, "a;b", ";\n", false, false, None), Some(("a;\nb".into(), 1)));
        assert_eq!(replace_text(&semi, "a;b", r"\n", false, false, None), Some((r"a\nb".into(), 1)));
    }

    #[test]
    fn filters_match_like_people_expect() {
        let dir = std::env::temp_dir().join(format!("florm_filters_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for f in [
            "addons/syndic/__init__.py",
            "addons/syndic/models/budget.py",
            "addons/syndic/views/budget.xml",
            "addons/syndic/tests/test_budget.py",
            "addons/other/models/budget.py",
            "syndic.py",
        ] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "budget\n").unwrap();
        }
        let gen = Arc::new(AtomicU64::new(1));
        let rels = |inc: &str, exc: &str| {
            let q = Query { pattern: "budget".into(), include: inc.into(), exclude: exc.into(), ..Default::default() };
            let out = run(&dir, &q, &crate::walk::Exclusions::none(), 1, &gen);
            assert!(out.error.is_none(), "{:?}", out.error);
            out.files.into_iter().map(|f| f.rel.replace('\\', "/")).collect::<Vec<_>>()
        };
        let syndic_py = ["addons/syndic/__init__.py", "addons/syndic/models/budget.py", "addons/syndic/tests/test_budget.py"];
        assert_eq!(rels("syndic/*.py", ""), syndic_py);
        assert_eq!(rels("syndic/**/*.py", ""), syndic_py);
        assert_eq!(rels("addons/syndic/*.py", ""), syndic_py);
        assert_eq!(rels("syndic/*.py", "tests"), &syndic_py[..2]);
        assert_eq!(rels("syndic/*.py", "syndic/tests"), &syndic_py[..2]);
        assert_eq!(rels("syndic", "").len(), 4);
        assert_eq!(rels("models", ""), ["addons/other/models/budget.py", "addons/syndic/models/budget.py"]);
        assert_eq!(rels("./syndic/*.py", ""), Vec::<String>::new());
        assert_eq!(rels("./addons/syndic/*.py", ""), syndic_py);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
