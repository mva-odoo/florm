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

/// A grammar: the syntect syntax it replaces, its language, its highlight query.
pub struct Grammar {
    pub syntax: &'static str,
    pub language: Language,
    pub query: Query,
    /// The theme scope of each capture of the query (same order).
    pub scopes: Vec<Option<&'static str>>,
}


fn grammars() -> &'static Vec<Grammar> {
    static G: OnceLock<Vec<Grammar>> = OnceLock::new();
    G.get_or_init(|| {
        let mut v = Vec::new();
        let py: Language = tree_sitter_python::LANGUAGE.into();
        if let Ok(q) = Query::new(&py, tree_sitter_python::HIGHLIGHTS_QUERY) {
            let scopes = q.capture_names().iter().map(|n| scope_of(n)).collect();
            v.push(Grammar { syntax: "Python", language: py, query: q, scopes });
        }
        v
    })
}

/// The theme scope (TextMate) of a capture of a highlight query: the themes of Florm color the
/// tree-sitter captures as they color the regex scopes. None: the text color.
pub fn scope_of(capture: &str) -> Option<&'static str> {
    Some(match capture {
        "keyword" | "keyword.control" | "conditional" | "repeat" | "include" | "exception" => "keyword.control",
        "keyword.function" | "keyword.operator" => "storage.type",
        "operator" => "keyword.operator",
        "function" | "function.method" | "method" => "entity.name.function",
        "function.builtin" => "support.function",
        "constructor" => "entity.name.class",
        "type" | "type.builtin" => "support.type",
        "string" | "string.special" => "string",
        "escape" | "string.escape" => "constant.character.escape",
        "comment" => "comment",
        "number" | "float" => "constant.numeric",
        "constant" => "constant.other",
        "constant.builtin" | "boolean" => "constant.language",
        "property" | "field" => "variable.other.member",
        "variable.builtin" => "variable.language",
        "variable.parameter" | "parameter" => "variable.parameter",
        "punctuation.special" | "embedded" => "punctuation.section.embedded",
        "attribute" => "entity.other.attribute-name",
        "tag" => "entity.name.tag",
        _ => return None,
    })
}

/// Every scope the grammars use (their colors are read once per theme).
pub fn all_scopes() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = grammars().iter().flat_map(|g| g.scopes.iter().flatten().copied()).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The syntaxes whose grammar is turned off (language packs), set by the app.
fn off() -> &'static RwLock<HashSet<String>> {
    static OFF: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    OFF.get_or_init(|| RwLock::new(HashSet::new()))
}

pub fn set_off(syntaxes: &[String]) {
    if let Ok(mut o) = off().write() {
        *o = syntaxes.iter().cloned().collect();
    }
}

/// The grammar used for `syntax` (compiled in and not turned off).
pub fn grammar_for(syntax: &str) -> Option<&'static Grammar> {
    if off().read().is_ok_and(|o| o.contains(syntax)) {
        return None;
    }
    grammars().iter().find(|g| g.syntax == syntax)
}

/// Files above this size keep the regex highlighting: the first parse is on the UI thread (~30 ms
/// for 300 KB of Python), and a tree takes ~10 times the text in memory.
pub const MAX_BYTES: usize = 1_000_000;

/// The tree of a document, kept up to date by its edits.
pub struct SyntaxTree {
    grammar: &'static Grammar,
    parser: Parser,
    tree: Option<Tree>,
    /// Edits since the last parse (applied to the tree before parsing again).
    pending: Vec<InputEdit>,
    /// The lines changed by the last parse, from the first one (their colors read again).
    pub changed_from: Option<usize>,
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
        Some(SyntaxTree { grammar, parser, tree: None, pending: Vec::new(), changed_from: None })
    }

    pub fn syntax(&self) -> &'static str {
        self.grammar.syntax
    }

    /// An edit about to be made to the text (`input_edit`).
    pub fn edit(&mut self, e: InputEdit) {
        if self.tree.is_some() {
            self.pending.push(e);
        }
    }

    /// The tree for `rope`: parsed again (incrementally) when the text changed. None: the parse
    /// failed (the text is too long, or the grammar refused it).
    pub fn sync(&mut self, rope: &Rope) -> Option<&Tree> {
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
            let mut read = |byte: usize, _: Point| -> &[u8] {
                if byte >= rope.len_bytes() {
                    return &[];
                }
                let (chunk, start, _, _) = rope.chunk_at_byte(byte);
                &chunk.as_bytes()[byte - start..]
            };
            let new = self.parser.parse_with_options(&mut read, old.as_ref(), None)?;
            self.changed_from = match &old {
                None => Some(0),
                Some(o) => o.changed_ranges(&new).map(|r| r.start_point.row).min(),
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
        let g = self.grammar;
        let mut paint: Vec<(usize, usize, usize, u32)> = Vec::new(); // (size, pattern, capture, start..)
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start..end);
        let provider = RopeText(rope);
        let mut caps = cursor.captures(&g.query, tree.root_node(), provider);
        while let Some((m, i)) = caps.next() {
            let c = m.captures()[*i];
            if g.scopes.get(c.index as usize).copied().flatten().is_none() {
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
            let col = color(g.scopes[cap].expect("scoped"));
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

    #[test]
    fn a_grammar_turned_off_is_not_used() {
        // another syntax than the ones the other tests use (they run at the same time)
        set_off(&["Rust".into()]);
        assert!(grammar_for("Python").is_some() && grammar_for("Rust").is_none());
        set_off(&[]);
    }
}
