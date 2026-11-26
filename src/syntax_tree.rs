//! Syntax trees with tree-sitter: a parse kept up to date at each edit (incremental: a key typed
//! re-parses the little that changed), the colors of the lines shown read from the tree with the
//! grammar's highlight query, and the selection grown node by node. The grammars are compiled
//! into Florm (one per language pack that has one); a pack can turn its grammar off, the regex
//! highlighting (syntect) of the other languages stays.

use eframe::egui::Color32;
use ropey::Rope;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::{OnceLock, RwLock};
use streaming_iterator::StreamingIterator;
use tree_sitter::{InputEdit, Language, Node, Parser, Point, Query, QueryCursor, Tree};

/// A grammar: its name (the one of the language pack's switch), its language, its highlight query.
pub struct Grammar {
    pub name: &'static str,
    pub language: Language,
    /// Its highlight query, compiled the first time a file of this language is colored (all of
    /// them at once took 100 ms on the first file opened), with the theme scope of each of its
    /// captures (same order). None: the grammar refused the query (its files keep the regexes).
    compiled: OnceLock<Option<(Query, Vec<Option<&'static str>>)>>,
    src: String,
    /// The extensions of its files (without the dot).
    pub ext: &'static [&'static str],
    /// The syntaxes (as Florm names them) of its files without a known extension (a #! line).
    pub syntaxes: &'static [&'static str],
}

/// The grammars compiled into Florm: (name, extensions, syntaxes, language, highlight query —
/// several joined for a language built on another: TypeScript on JavaScript, C++ on C).
fn table() -> Vec<(&'static str, &'static [&'static str], &'static [&'static str], Language, String)> {
    let js = tree_sitter_javascript::HIGHLIGHT_QUERY;
    let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
    let ts = tree_sitter_typescript::HIGHLIGHTS_QUERY;
    let c = tree_sitter_c::HIGHLIGHT_QUERY;
    vec![
        ("Python", &["py", "pyi", "pyw"], &["Python"], tree_sitter_python::LANGUAGE.into(), tree_sitter_python::HIGHLIGHTS_QUERY.to_string()),
        ("JavaScript", &["js", "mjs", "cjs", "jsx"], &["JavaScript"], tree_sitter_javascript::LANGUAGE.into(), format!("{js}\n{jsx}")),
        ("TypeScript", &["ts", "mts", "cts"], &[], tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), format!("{js}\n{ts}")),
        ("TSX", &["tsx"], &[], tree_sitter_typescript::LANGUAGE_TSX.into(), format!("{js}\n{jsx}\n{ts}")),
        ("Rust", &["rs"], &["Rust"], tree_sitter_rust::LANGUAGE.into(), tree_sitter_rust::HIGHLIGHTS_QUERY.to_string()),
        ("Go", &["go"], &["Go"], tree_sitter_go::LANGUAGE.into(), tree_sitter_go::HIGHLIGHTS_QUERY.to_string()),
        ("C", &["c", "h"], &["C"], tree_sitter_c::LANGUAGE.into(), c.to_string()),
        ("C++", &["cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "ipp"], &["C++"], tree_sitter_cpp::LANGUAGE.into(), format!("{c}\n{}", tree_sitter_cpp::HIGHLIGHT_QUERY)),
        ("JSON", &["json", "jsonc", "json5", "geojson", "webmanifest"], &["JSON"], tree_sitter_json::LANGUAGE.into(), tree_sitter_json::HIGHLIGHTS_QUERY.to_string()),
        ("Shell", &["sh", "bash", "zsh", "ksh"], &["Bourne Again Shell (bash)"], tree_sitter_bash::LANGUAGE.into(), tree_sitter_bash::HIGHLIGHT_QUERY.to_string()),
        ("CSS", &["css"], &["CSS"], tree_sitter_css::LANGUAGE.into(), tree_sitter_css::HIGHLIGHTS_QUERY.to_string()),
        ("HTML", &["html", "htm", "xhtml"], &["HTML"], tree_sitter_html::LANGUAGE.into(), tree_sitter_html::HIGHLIGHTS_QUERY.to_string()),
        ("YAML", &["yml", "yaml"], &["YAML"], tree_sitter_yaml::LANGUAGE.into(), tree_sitter_yaml::HIGHLIGHTS_QUERY.to_string()),
        ("XML", &["xml", "csproj", "xaml", "rng", "kml", "plist", "xsl", "xslt", "svg", "xsd", "wsdl"], &["XML"], tree_sitter_xml::LANGUAGE_XML.into(), tree_sitter_xml::XML_HIGHLIGHT_QUERY.to_string()),
        ("TOML", &["toml"], &[], tree_sitter_toml_ng::LANGUAGE.into(), tree_sitter_toml_ng::HIGHLIGHTS_QUERY.to_string()),
    ]
}

fn grammars() -> &'static Vec<Grammar> {
    static G: OnceLock<Vec<Grammar>> = OnceLock::new();
    G.get_or_init(|| {
        table()
            .into_iter()
            .map(|(name, ext, syntaxes, language, src)| Grammar { name, language, compiled: OnceLock::new(), src, ext, syntaxes })
            .collect()
    })
}

impl Grammar {
    fn compiled(&self) -> Option<&(Query, Vec<Option<&'static str>>)> {
        self.compiled
            .get_or_init(|| {
                // a query the grammar refuses: that language keeps the regexes (a test checks none does)
                let query = Query::new(&self.language, &self.src).ok()?;
                let scopes = query.capture_names().iter().map(|n| scope_of(n)).collect();
                Some((query, scopes))
            })
            .as_ref()
    }

    /// The theme scopes its query uses (their colors are read once per theme).
    pub fn scopes(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self.compiled().map(|(_, s)| s.iter().flatten().copied().collect()).unwrap_or_default();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// The highlight queries compiled ahead, on a thread (at startup): the first file of a language
/// is colored without waiting for its query.
pub fn warm_up() {
    std::thread::spawn(|| {
        for g in grammars() {
            let _ = g.compiled();
        }
    });
}

/// The names of the grammars compiled into Florm.
#[cfg(test)]
pub fn names() -> Vec<&'static str> {
    grammars().iter().filter(|g| g.compiled().is_some()).map(|g| g.name).collect()
}

/// The theme scope (TextMate) of a capture of a highlight query: the themes of Florm color the
/// tree-sitter captures as they color the regex scopes. A capture not known is looked up without
/// its last part (`function.method.call` → `function.method` → `function`). None: the text color.
pub fn scope_of(capture: &str) -> Option<&'static str> {
    let mut c = capture;
    loop {
        let s = match c {
            "keyword" | "keyword.control" | "conditional" | "repeat" | "include" | "exception" | "import" | "charset" | "media" | "keyframes" | "supports" => Some("keyword.control"),
            "keyword.function" | "keyword.storage" | "storage" => Some("storage.type"),
            "operator" | "keyword.operator" => Some("keyword.operator"),
            "function" | "function.method" | "method" | "function.special" => Some("entity.name.function"),
            "function.builtin" => Some("support.function"),
            "function.macro" => Some("entity.name.function.macro"),
            "constructor" => Some("entity.name.class"),
            "type" | "type.builtin" | "namespace" | "module" => Some("support.type"),
            "string" | "string.special" | "markup.raw" => Some("string"),
            "string.special.key" => Some("support.type.property-name"),
            "string.special.symbol" => Some("constant.other.symbol"),
            "escape" | "string.escape" => Some("constant.character.escape"),
            "comment" | "comment.documentation" => Some("comment"),
            "number" | "float" => Some("constant.numeric"),
            "constant" => Some("constant.other"),
            "constant.builtin" | "boolean" => Some("constant.language"),
            "property" | "field" => Some("variable.other.member"),
            "variable.builtin" => Some("variable.language"),
            "variable.parameter" | "parameter" => Some("variable.parameter"),
            "punctuation.special" | "embedded" => Some("punctuation.section.embedded"),
            "attribute" => Some("entity.other.attribute-name"),
            "tag" => Some("entity.name.tag"),
            "tag.error" | "error" => Some("invalid"),
            "label" => Some("entity.name.label"),
            "markup.heading" => Some("markup.heading"),
            "markup.link" => Some("markup.underline.link"),
            // the plain text color: variables, brackets, separators
            "variable" | "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "delimiter" | "markup" => return None,
            _ => None,
        };
        if s.is_some() {
            return s;
        }
        c = c.rsplit_once('.')?.0;
    }
}

/// Every scope the grammars use (their colors are read once per theme).
#[cfg(test)]
pub fn all_scopes() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = grammars().iter().flat_map(|g| g.scopes()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The grammars turned off (by their language pack), set by the app.
fn off() -> &'static RwLock<HashSet<String>> {
    static OFF: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    OFF.get_or_init(|| RwLock::new(HashSet::new()))
}

pub fn set_off(syntaxes: &[String]) {
    if let Ok(mut o) = off().write() {
        *o = syntaxes.iter().cloned().collect();
    }
}

/// The grammar named `name` (compiled in and not turned off).
pub fn grammar_for(name: &str) -> Option<&'static Grammar> {
    if off().read().is_ok_and(|o| o.contains(name)) {
        return None;
    }
    grammars().iter().find(|g| g.name == name).filter(|g| g.compiled().is_some())
}

/// The grammar of a file: by its extension, else by its syntax (a file without extension whose
/// #! line says Python…). None: no grammar, or turned off.
pub fn grammar_for_file(syntax: &str, ext: &str) -> Option<&'static Grammar> {
    let ext = ext.to_ascii_lowercase();
    let g = if ext.is_empty() {
        grammars().iter().find(|g| g.syntaxes.contains(&syntax))
    } else {
        grammars().iter().find(|g| g.ext.contains(&ext.as_str()))
    }?;
    grammar_for(g.name)
}

/// Files above this size keep the regex highlighting: the first parse is on the UI thread (~30 ms
/// for 300 KB of Python), and a tree takes ~10 times the text in memory.
pub const MAX_BYTES: usize = 1_000_000;

/// The tree of a document, kept up to date by its edits.
pub struct SyntaxTree {
    grammar: &'static Grammar,
    /// None while the first parse of a long text runs on a thread (it has the parser).
    parser: Option<Parser>,
    /// That first parse: its parser and tree come back here.
    first: Option<std::sync::mpsc::Receiver<(Parser, Option<Tree>)>>,
    tree: Option<Tree>,
    /// Edits since the last parse (applied to the tree before parsing again).
    pending: Vec<InputEdit>,
    /// The lines changed by the last parse, from the first one (their colors read again).
    pub changed_from: Option<usize>,
}

/// A text longer than this is parsed the first time on a thread (~50 ms for 300 KB of Python):
/// the file shows at once, its colors a moment later.
const PARSE_ON_A_THREAD: usize = 64 * 1024;

/// The bytes of `rope` from `byte` on, for the parser.
fn read_rope(rope: &Rope, byte: usize) -> &[u8] {
    if byte >= rope.len_bytes() {
        return &[];
    }
    let (chunk, start, _, _) = rope.chunk_at_byte(byte);
    &chunk.as_bytes()[byte - start..]
}

/// An edit of the text, for `SyntaxTree::edit`: computed by the document before it changes the rope.
pub fn input_edit(rope: &Rope, start_char: usize, end_char: usize, new_text: &str) -> InputEdit {
    let point = |byte: usize| {
        let row = rope.byte_to_line(byte);
        Point::new(row, byte - rope.line_to_byte(row))
    };
    let start_byte = rope.char_to_byte(start_char);
    let old_end_byte = rope.char_to_byte(end_char);
    let start_position = point(start_byte);
    let old_end_position = point(old_end_byte);
    // where the new text ends: from the start, by its lines
    let new_end_position = match new_text.rfind('\n') {
        Some(i) => Point::new(start_position.row + new_text.matches('\n').count(), new_text.len() - i - 1),
        None => Point::new(start_position.row, start_position.column + new_text.len()),
    };
    InputEdit { start_byte, old_end_byte, new_end_byte: start_byte + new_text.len(), start_position, old_end_position, new_end_position }
}

impl SyntaxTree {
    pub fn new(grammar: &'static Grammar) -> Option<Self> {
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        Some(SyntaxTree { grammar, parser: Some(parser), first: None, tree: None, pending: Vec::new(), changed_from: None })
    }

    pub fn name(&self) -> &'static str {
        self.grammar.name
    }

    pub fn grammar(&self) -> &'static Grammar {
        self.grammar
    }

    /// An edit about to be made to the text (`input_edit`).
    pub fn edit(&mut self, e: InputEdit) {
        // also while the first parse runs: applied to its tree once it is back
        if self.tree.is_some() || self.first.is_some() {
            self.pending.push(e);
        }
    }

    /// The first parse of a long text is running on a thread (no tree yet: ask again later).
    pub fn parsing(&self) -> bool {
        self.first.is_some()
    }

    /// The tree for `rope`: parsed again (incrementally) when the text changed. None: the parse
    /// failed (the text is too long, or the grammar refused it).
    pub fn sync(&mut self, rope: &Rope) -> Option<&Tree> {
        // the first parse of a long text: on a thread, the text drawn without colors meanwhile
        if let Some(rx) = &self.first {
            match rx.try_recv() {
                Ok((parser, tree)) => {
                    self.first = None;
                    self.parser = Some(parser);
                    self.tree = tree;
                    // the lines read again from the top; the edits made meanwhile applied below
                    self.changed_from = Some(0);
                    if self.tree.is_none() {
                        self.pending.clear();
                        return None;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.first = None;
                    return None;
                }
            }
        } else if self.tree.is_none() && rope.len_bytes() > PARSE_ON_A_THREAD {
            let Some(mut parser) = self.parser.take() else { return None };
            let text = rope.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            self.pending.clear();
            self.first = Some(rx);
            std::thread::spawn(move || {
                let tree = parser.parse_with_options(&mut |byte: usize, _: Point| read_rope(&text, byte), None, None);
                let _ = tx.send((parser, tree));
            });
            return None;
        }
        let stale = self.tree.as_ref().is_none_or(|t| !self.pending.is_empty() || t.root_node().end_byte() != rope.len_bytes());
        if stale {
            let mut old = self.tree.take();
            if let Some(t) = old.as_mut() {
                for e in self.pending.drain(..) {
                    t.edit(&e);
                }
                // an edit not told (the text replaced some other way): parsed from scratch
                if t.root_node().end_byte() != rope.len_bytes() {
                    old = None;
                }
            }
            self.pending.clear();
            let mut read = |byte: usize, _: Point| -> &[u8] { read_rope(rope, byte) };
            let new = self.parser.as_mut()?.parse_with_options(&mut read, old.as_ref(), None)?;
            let changed = match &old {
                None => Some(0),
                Some(o) => o.changed_ranges(&new).map(|r| r.start_point.row).min(),
            };
            self.changed_from = match (self.changed_from, changed) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            self.tree = Some(new);
        }
        self.tree.as_ref()
    }

    /// The tree as last parsed (without parsing again).
    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    /// The colored byte spans of lines `lines` (each line's spans relative to its start), the
    /// colors of the scopes given by `color`. The tree must be in sync with `rope`.
    pub fn line_spans(&self, rope: &Rope, lines: Range<usize>, color: &dyn Fn(&'static str) -> Color32) -> Vec<Vec<(Color32, Range<usize>)>> {
        let Some(tree) = &self.tree else { return Vec::new() };
        let lines = lines.start.min(rope.len_lines())..lines.end.min(rope.len_lines());
        if lines.is_empty() {
            return Vec::new();
        }
        let start = rope.line_to_byte(lines.start);
        let end = if lines.end >= rope.len_lines() { rope.len_bytes() } else { rope.line_to_byte(lines.end) };
        // the block's text, for the predicates of the query (#match?) and the colors byte by byte
        let text: Vec<u8> = rope.byte_slice(start..end).bytes().collect();
        let Some((query, scopes)) = self.grammar.compiled() else { return Vec::new() };
        let mut paint: Vec<(usize, usize, usize, u32)> = Vec::new(); // (size, pattern, capture, start..)
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start..end);
        let provider = RopeText(rope);
        let mut caps = cursor.captures(query, tree.root_node(), provider);
        while let Some((m, i)) = caps.next() {
            let c = m.captures()[*i];
            if scopes.get(c.index as usize).copied().flatten().is_none() {
                continue;
            }
            let (a, b) = (c.node.start_byte().max(start), c.node.end_byte().min(end));
            if a >= b {
                continue;
            }
            paint.push((b - a, m.pattern_index, c.index as usize, ranges.len() as u32));
            ranges.push((a - start, b - start));
        }
        // outer nodes first, then for the same node the later patterns (the precise ones) last:
        // what is painted last wins
        paint.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
        let mut colors: Vec<Option<Color32>> = vec![None; text.len()];
        for (_, _, cap, k) in paint {
            let (a, b) = ranges[k as usize];
            let col = color(scopes[cap].expect("scoped"));
            colors[a..b].iter_mut().for_each(|c| *c = Some(col));
        }
        // runs of one color, cut at the ends of lines
        let mut out = Vec::with_capacity(lines.len());
        let mut line_start = 0;
        for l in lines.clone() {
            let line_end = if l + 1 >= rope.len_lines() { text.len() } else { rope.line_to_byte(l + 1) - start };
            let mut spans: Vec<(Color32, Range<usize>)> = Vec::new();
            let mut i = line_start;
            while i < line_end {
                let c = colors[i];
                let mut j = i + 1;
                while j < line_end && colors[j] == c {
                    j += 1;
                }
                if let Some(c) = c {
                    spans.push((c, i - line_start..j - line_start));
                }
                i = j;
            }
            out.push(spans);
            line_start = line_end;
        }
        out
    }
}

/// The text of the nodes for the predicates of a query, read from the rope.
struct RopeText<'a>(&'a Rope);

impl<'a> tree_sitter::TextProvider<&'a [u8]> for RopeText<'a> {
    type I = RopeChunks<'a>;
    fn text(&mut self, node: Node) -> Self::I {
        RopeChunks { rope: self.0, at: node.start_byte(), end: node.end_byte().min(self.0.len_bytes()) }
    }
}

struct RopeChunks<'a> {
    rope: &'a Rope,
    at: usize,
    end: usize,
}

impl<'a> Iterator for RopeChunks<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.end {
            return None;
        }
        let (chunk, start, _, _) = self.rope.chunk_at_byte(self.at);
        let bytes = &chunk.as_bytes()[self.at - start..];
        let n = bytes.len().min(self.end - self.at);
        self.at += n;
        Some(&bytes[..n])
    }
}

/// The selection grown to the smallest syntax node around `a..b` (bytes) that is larger.
pub fn grow(tree: &Tree, a: usize, b: usize) -> Option<(usize, usize)> {
    let mut node = tree.root_node().descendant_for_byte_range(a, b)?;
    loop {
        if node.start_byte() < a || node.end_byte() > b {
            return Some((node.start_byte(), node.end_byte()));
        }
        node = node.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(text: &str) -> (Rope, Vec<Vec<(Color32, Range<usize>)>>) {
        let g = grammar_for("Python").expect("python");
        let rope = Rope::from_str(text);
        let mut t = SyntaxTree::new(g).unwrap();
        t.sync(&rope).unwrap();
        // one color per scope: the scope's length as red
        let spans = t.line_spans(&rope, 0..rope.len_lines(), &|s| Color32::from_rgb(s.len() as u8, 0, 0));
        (rope, spans)
    }

    fn scope_at(spans: &[(Color32, Range<usize>)], byte: usize) -> Option<u8> {
        spans.iter().find(|(_, r)| r.contains(&byte)).map(|(c, _)| c.r())
    }

    /// Keywords, functions, strings and comments get their scopes; the precise patterns win
    /// over the general ones (a call of a builtin is a builtin, not a plain call).
    #[test]
    fn python_is_colored_from_the_tree() {
        let (_, s) = colors("def f(x):\n    return len(x)  # n\ns = \"a\\n\"\n");
        let len = |sc: &str| Some(sc.len() as u8);
        assert_eq!(scope_at(&s[0], 0), len("keyword.control"), "def: {:?}", s[0]);
        assert_eq!(scope_at(&s[0], 4), len("entity.name.function"));
        assert_eq!(scope_at(&s[1], 11), len("support.function"), "len: {:?}", s[1]);
        assert_eq!(scope_at(&s[1], 20), len("comment"));
        assert_eq!(scope_at(&s[2], 4), len("string"));
        assert_eq!(scope_at(&s[2], 6), len("constant.character.escape"), "the escape inside the string: {:?}", s[2]);
    }

    /// Edits told to the tree: parsed again incrementally, same colors as a fresh parse.
    #[test]
    fn edits_keep_the_tree_right() {
        let g = grammar_for("Python").unwrap();
        let mut rope = Rope::from_str("x = 1\ny = 2\n");
        let mut t = SyntaxTree::new(g).unwrap();
        t.sync(&rope).unwrap();
        // « def » typed at the start of the second line, then a line removed
        for (a, b, text) in [(6usize, 6usize, "def g(): pass\n"), (0, 6, "")] {
            t.edit(input_edit(&rope, a, b, text));
            rope.remove(a..b);
            rope.insert(a, text);
            t.sync(&rope).unwrap();
        }
        assert_eq!(rope.to_string(), "def g(): pass\ny = 2\n");
        let fresh = colors(&rope.to_string()).1;
        let incr = t.line_spans(&rope, 0..rope.len_lines(), &|s| Color32::from_rgb(s.len() as u8, 0, 0));
        assert_eq!(incr, fresh);
    }

    /// The selection grows node by node: the word, the call, the statement.
    #[test]
    fn the_selection_grows_by_node() {
        let rope = Rope::from_str("y = foo(a, b)\n");
        let mut t = SyntaxTree::new(grammar_for("Python").unwrap()).unwrap();
        let tree = t.sync(&rope).unwrap().clone();
        let s = rope.to_string();
        let pick = |r: (usize, usize)| s[r.0..r.1].to_string();
        let w = grow(&tree, 8, 8).unwrap();
        assert_eq!(pick(w), "a");
        let args = grow(&tree, w.0, w.1).unwrap();
        assert_eq!(pick(args), "(a, b)");
        assert_eq!(pick(grow(&tree, args.0, args.1).unwrap()), "foo(a, b)");
    }

    /// Every grammar accepts its query (none falls back to the regexes), every grammar named by a
    /// pack exists, and each language colors a small sample: a keyword and a string at least.
    #[test]
    fn every_language_is_colored() {
        assert_eq!(names().len(), table().len(), "a query refused: {:?}", names());
        for p in crate::packs::shipped() {
            for g in &p.tree_sitter {
                assert!(names().contains(&g.as_str()), "{} names {g}", p.name);
            }
        }
        let samples: &[(&str, &str, &str, &str)] = &[
            ("Python", "py", "def f():\n    return \"s\"\n", "return"),
            ("JavaScript", "js", "function f() { return \"s\"; }\n", "return"),
            ("TypeScript", "ts", "function f(x: number): string { return \"s\"; }\n", "return"),
            ("TSX", "tsx", "const a = <div>{\"s\"}</div>;\nconst b = 1;\n", "const"),
            ("Rust", "rs", "fn f() -> &'static str { return \"s\"; }\n", "fn"),
            ("Go", "go", "package main\nfunc f() string { return \"s\" }\n", "func"),
            ("C", "c", "int f() { return \"s\"[0]; }\n", "return"),
            ("C++", "cpp", "class A {}; int f() { return \"s\"[0]; }\n", "class"),
            ("JSON", "json", "{\"key\": \"s\", \"n\": true}\n", "true"),
            ("Shell", "sh", "if true; then echo \"s\"; fi\n", "if"),
            ("CSS", "css", "@media screen { a { color: red; content: \"s\"; } }\n", "@media"),
            ("HTML", "html", "<a href=\"s\">x</a>\n", "a"),
            ("YAML", "yaml", "key: \"s\"\nn: true\n", "true"),
            ("XML", "xml", "<a b=\"s\">x</a>\n", "a"),
            ("TOML", "toml", "[t]\nkey = \"s\"\nn = true\n", "true"),
        ];
        assert_eq!(samples.len(), names().len());
        for (name, ext, code, word) in samples {
            let g = grammar_for_file("", ext).unwrap_or_else(|| panic!("{ext}"));
            assert_eq!(g.name, *name);
            let rope = Rope::from_str(code);
            let mut t = SyntaxTree::new(g).unwrap();
            t.sync(&rope).unwrap();
            let all: Vec<&'static str> = all_scopes();
            let spans = t.line_spans(&rope, 0..rope.len_lines(), &|sc| {
                let k = all.iter().position(|x| *x == sc).unwrap() as u8;
                Color32::from_rgb(k, 1, 0)
            });
            let scope_at = |byte: usize| -> Option<&'static str> {
                let mut off = 0;
                for (l, sp) in spans.iter().enumerate() {
                    let len = rope.line(l).len_bytes();
                    if byte < off + len {
                        return sp.iter().find(|(_, r)| r.contains(&(byte - off))).map(|(c, _)| all[c.r() as usize]);
                    }
                    off += len;
                }
                None
            };
            let q = code.find("\"s\"").unwrap() + 1;
            assert!(scope_at(q).is_some_and(|s| s.starts_with("string") || s.starts_with("support.type.property")), "{name}: the string at {q}: {spans:?}");
            let w = code.find(word).unwrap();
            assert!(scope_at(w).is_some(), "{name}: « {word} » has no color: {spans:?}");
        }
    }

    #[test]
    fn a_grammar_turned_off_is_not_used() {
        // another syntax than the ones the other tests use (they run at the same time)
        set_off(&["Rust".into()]);
        assert!(grammar_for("Python").is_some() && grammar_for("Rust").is_none());
        set_off(&[]);
    }
}
