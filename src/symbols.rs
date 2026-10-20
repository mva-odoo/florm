//! Symbols of a file (classes, functions, Odoo records, Markdown headings…) found with per-language
//! regexes, and indentation-based blocks, used by the symbol list (Ctrl+R), the breadcrumbs and folding.

use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Class,
    Function,
    Method,
    Record,
    Heading,
    Other,
}

impl Kind {
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Class => "C",
            Kind::Function => "ƒ",
            Kind::Method => "m",
            Kind::Record => "R",
            Kind::Heading => "#",
            Kind::Other => "·",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: Kind,
    /// 0-based line of the definition.
    pub line: usize,
    /// Last line of its block (inclusive).
    pub end: usize,
    /// Nesting depth (0 = top level).
    pub depth: usize,
    /// Extra information shown in the list (e.g. the model of an Odoo record).
    pub detail: String,
}

struct Rule {
    re: Regex,
    kind: Kind,
}

fn rules(syntax: &str) -> &'static [Rule] {
    static PY: OnceLock<Vec<Rule>> = OnceLock::new();
    static JS: OnceLock<Vec<Rule>> = OnceLock::new();
    static RS: OnceLock<Vec<Rule>> = OnceLock::new();
    static GO: OnceLock<Vec<Rule>> = OnceLock::new();
    static PHP: OnceLock<Vec<Rule>> = OnceLock::new();
    static RB: OnceLock<Vec<Rule>> = OnceLock::new();
    static SH: OnceLock<Vec<Rule>> = OnceLock::new();
    static XML: OnceLock<Vec<Rule>> = OnceLock::new();
    static CSS: OnceLock<Vec<Rule>> = OnceLock::new();
    static JAVA: OnceLock<Vec<Rule>> = OnceLock::new();
    let r = |re: &str, kind| Rule { re: Regex::new(re).unwrap(), kind };
    match syntax {
        "Python" => PY.get_or_init(|| {
            vec![
                r(r"^\s*class\s+(\w+)", Kind::Class),
                r(r"^\s*(?:async\s+)?def\s+(\w+)", Kind::Function),
                // Odoo model: _name = 'sale.order'
                r(r#"^\s+_name\s*=\s*['"]([\w.]+)['"]"#, Kind::Record),
            ]
        }),
        "JavaScript" | "TypeScript" | "JSX" | "TypeScriptReact" => JS.get_or_init(|| {
            vec![
                r(r"^\s*(?:export\s+)?(?:default\s+)?class\s+(\w+)", Kind::Class),
                r(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*(\w+)", Kind::Function),
                r(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(?:function\b|\([^)]*\)\s*=>|\w+\s*=>)", Kind::Function),
                r(
                    r"^\s+(?:static\s+)?(?:async\s+)?(?:get\s+|set\s+)?\*?((?:[A-Za-z_$][\w$]*))\s*\([^)]*\)\s*\{",
                    Kind::Method,
                ),
            ]
        }),
        "Rust" => RS.get_or_init(|| {
            vec![
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|union)\s+(\w+)", Kind::Class),
                r(r"^\s*impl(?:<[^>]*>)?\s+([\w:<>, ]+?)\s*(?:\{|where|$)", Kind::Class),
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)", Kind::Class),
                r(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+(\w+)", Kind::Function),
            ]
        }),
        "Go" => GO.get_or_init(|| {
            vec![r(r"^type\s+(\w+)", Kind::Class), r(r"^func\s+(?:\([^)]*\)\s*)?(\w+)", Kind::Function)]
        }),
        "PHP" => PHP.get_or_init(|| {
            vec![
                r(r"^\s*(?:abstract\s+|final\s+)?(?:class|interface|trait)\s+(\w+)", Kind::Class),
                r(r"^\s*(?:(?:public|private|protected|static|abstract|final)\s+)*function\s+(\w+)", Kind::Function),
            ]
        }),
        "Ruby" => RB.get_or_init(|| {
            vec![r(r"^\s*(?:class|module)\s+([\w:]+)", Kind::Class), r(r"^\s*def\s+((?:self\.)?[\w?!=]+)", Kind::Function)]
        }),
        "Bourne Again Shell (bash)" | "Shell-Unix-Generic" => SH.get_or_init(|| {
            vec![r(r"^\s*(?:function\s+)?([A-Za-z_][\w-]*)\s*\(\)", Kind::Function)]
        }),
        "XML" | "HTML" => XML.get_or_init(|| {
            vec![
                r(r#"<(?:record|template|menuitem|act_window|report)\b[^>]*?\bid="([^"]+)""#, Kind::Record),
                r(r#"\bt-name="([^"]+)""#, Kind::Record),
            ]
        }),
        "CSS" | "SCSS" | "Sass" | "LESS" => CSS.get_or_init(|| vec![r(r"^([^\s{}/@][^{};]*?)\s*\{", Kind::Other)]),
        "Java" | "C#" | "C++" | "C" | "Kotlin" => JAVA.get_or_init(|| {
            vec![
                r(r"^\s*(?:(?:public|private|protected|static|abstract|final|sealed|partial|internal)\s+)*(?:class|interface|struct|enum|record)\s+(\w+)", Kind::Class),
                r(
                    r"^\s*(?:(?:public|private|protected|static|virtual|override|async|final|inline|extern)\s+)+[\w<>\[\],.? *&]+\s+(\w+)\s*\([^;]*$",
                    Kind::Function,
                ),
            ]
        }),
        _ => &[],
    }
}

/// Keywords that look like method calls in JS (`if (…) {`).
const NOT_METHODS: &[&str] = &["if", "for", "while", "switch", "catch", "function", "return", "with", "else"];

pub fn indent_of(line: &str, tab: usize) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += tab - n % tab,
            _ => break,
        }
    }
    n
}

/// Last line of the block opened at `line` (by indentation: the following lines indented deeper,
/// plus a closing line at the same level — `}`, `)`, `]`, `</…>`). None when nothing is nested.
pub fn block_end(lines: &[&str], line: usize, tab: usize) -> Option<usize> {
    block_end_with(lines.len(), line, tab, |i| lines[i].to_string())
}

/// `block_end` reading the lines through `get` (e.g. from a rope).
pub fn block_end_with(n: usize, line: usize, tab: usize, get: impl Fn(usize) -> String) -> Option<usize> {
    if line >= n {
        return None;
    }
    let head = get(line);
    if head.trim().is_empty() {
        return None;
    }
    let ind = indent_of(&head, tab);
    let mut last = None;
    for i in line + 1..n {
        let l = get(i);
        let t = l.trim();
        if t.is_empty() {
            continue;
        }
        let li = indent_of(&l, tab);
        if li > ind {
            last = Some(i);
            continue;
        }
        // a closing line at the same level belongs to the block (kept visible when folded)
        if li == ind && last.is_some() && (t.starts_with(['}', ')', ']']) || t.starts_with("</")) {
            return Some(i);
        }
        break;
    }
    last
}

/// Markdown: a heading's section goes until the next heading of the same or a higher level.
pub fn heading_level(line: &str) -> Option<usize> {
    let t = line.trim_end();
    let n = t.chars().take_while(|&c| c == '#').count();
    (n > 0 && n <= 6 && t[n..].starts_with(' ')).then_some(n)
}

pub fn heading_end(lines: &[&str], line: usize) -> Option<usize> {
    heading_end_with(lines.len(), line, |i| lines[i].to_string())
}

pub fn heading_end_with(n: usize, line: usize, get: impl Fn(usize) -> String) -> Option<usize> {
    if line >= n {
        return None;
    }
    let lvl = heading_level(&get(line))?;
    let mut in_code = false;
    let mut end = line;
    for i in line + 1..n {
        let l = &get(i);
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
        }
        if !in_code && heading_level(l).is_some_and(|x| x <= lvl) {
            break;
        }
        if !l.trim().is_empty() {
            end = i;
        }
    }
    (end > line).then_some(end)
}

/// Syntax name (as `extract` expects it) of a file, from its extension.
pub fn syntax_for(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "py" | "pyi" => "Python",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => "JavaScript",
        "rs" => "Rust",
        "go" => "Go",
        "php" => "PHP",
        "rb" => "Ruby",
        "sh" | "bash" => "Bourne Again Shell (bash)",
        "xml" => "XML",
        "html" | "htm" => "HTML",
        "css" | "scss" | "less" => "CSS",
        "java" => "Java",
        "cs" => "C#",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" | "cxx" => "C++",
        "md" | "markdown" => "Markdown",
        _ => return None,
    })
}

/// Symbols for the project index: `extract`, plus Python class attributes (Odoo fields).
pub fn extract_for_index(text: &str, syntax: &str) -> Vec<Symbol> {
    let mut v = extract(text, syntax, 4);
    if syntax == "Python" {
        static FIELD: OnceLock<Regex> = OnceLock::new();
        let re = FIELD.get_or_init(|| Regex::new(r"^    (\w+)\s*=\s*(fields\.\w+|[\w.]*Field)\(").unwrap());
        for (i, l) in text.split('\n').enumerate() {
            if let Some(c) = re.captures(l) {
                v.push(Symbol { name: c[1].to_string(), kind: Kind::Other, line: i, end: i, depth: 1, detail: c[2].to_string() });
            }
        }
    }
    v
}

pub fn extract(text: &str, syntax: &str, tab: usize) -> Vec<Symbol> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    if syntax == "Markdown" || syntax == "MultiMarkdown" {
        let mut in_code = false;
        for (i, l) in lines.iter().enumerate() {
            if l.trim_start().starts_with("```") {
                in_code = !in_code;
            }
            if in_code {
                continue;
            }
            if let Some(lvl) = heading_level(l) {
                let name = l.trim_start_matches('#').trim().to_string();
                out.push(Symbol { name, kind: Kind::Heading, line: i, end: heading_end(&lines, i).unwrap_or(i), depth: lvl - 1, detail: String::new() });
            }
        }
        return out;
    }
    let rules = rules(syntax);
    if rules.is_empty() {
        return out;
    }
    let model_re = {
        static M: OnceLock<Regex> = OnceLock::new();
        M.get_or_init(|| Regex::new(r#"\bmodel="([^"]+)""#).unwrap())
    };
    for (i, l) in lines.iter().enumerate() {
        if l.len() > 2000 {
            continue;
        }
        for rule in rules {
            let Some(c) = rule.re.captures(l) else { continue };
            let name = c[1].trim().to_string();
            if rule.kind == Kind::Method && NOT_METHODS.contains(&name.as_str()) {
                continue;
            }
            let detail = if rule.kind == Kind::Record {
                model_re.captures(l).map(|m| m[1].to_string()).unwrap_or_else(|| {
                    l.trim_start().trim_start_matches('<').split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-').next().unwrap_or("").to_string()
                })
            } else {
                String::new()
            };
            let end = block_end(&lines, i, tab).unwrap_or(i);
            out.push(Symbol { name, kind: rule.kind, line: i, end, depth: 0, detail });
            break;
        }
    }
    // nesting: a symbol is inside the previous ones whose block contains it
    let mut stack: Vec<(usize, Kind)> = Vec::new();
    for s in out.iter_mut() {
        stack.retain(|&(end, _)| end >= s.line);
        s.depth = stack.len();
        if s.kind == Kind::Function && stack.last().is_some_and(|&(_, k)| k == Kind::Class) {
            s.kind = Kind::Method;
        }
        stack.push((s.end, s.kind));
    }
    out
}

/// Symbols whose block contains `line`, outermost first (for the breadcrumbs).
pub fn path_at(symbols: &[Symbol], line: usize) -> Vec<&Symbol> {
    let mut v: Vec<&Symbol> = symbols.iter().filter(|s| s.line <= line && line <= s.end).collect();
    v.sort_by_key(|s| (s.depth, s.line));
    // keep a proper chain (one symbol per depth, the innermost of each)
    let mut chain: Vec<&Symbol> = Vec::new();
    for s in v {
        if chain.last().is_some_and(|c| c.depth >= s.depth) {
            chain.retain(|c| c.depth < s.depth);
        }
        chain.push(s);
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_classes_and_methods() {
        let src = "import x\n\nclass Account(models.Model):\n    _name = 'a'\n\n    def compute(self):\n        if x:\n            pass\n\n    async def other(self):\n        pass\n\n\ndef helper():\n    return 1\n";
        let s = extract(src, "Python", 4);
        let v: Vec<(&str, Kind, usize, usize, usize)> = s.iter().map(|s| (s.name.as_str(), s.kind, s.line, s.end, s.depth)).collect();
        assert_eq!(
            v,
            [
                ("Account", Kind::Class, 2, 10, 0),
                ("a", Kind::Record, 3, 3, 1), // Odoo model (_name)
                ("compute", Kind::Method, 5, 7, 1),
                ("other", Kind::Method, 9, 10, 1),
                ("helper", Kind::Function, 13, 14, 0),
            ]
        );
        let p: Vec<&str> = path_at(&s, 7).iter().map(|s| s.name.as_str()).collect();
        assert_eq!(p, ["Account", "compute"]);
        assert!(path_at(&s, 12).is_empty());
    }

    #[test]
    fn odoo_xml_and_js_and_markdown() {
        let xml = "<odoo>\n    <record id=\"view_form\" model=\"ir.ui.view\">\n        <field name=\"arch\" type=\"xml\">\n            <form/>\n        </field>\n    </record>\n    <menuitem id=\"menu_root\" name=\"X\"/>\n</odoo>\n";
        let s = extract(xml, "XML", 4);
        assert_eq!((s[0].name.as_str(), s[0].detail.as_str(), s[0].end), ("view_form", "ir.ui.view", 5));
        assert_eq!((s[1].name.as_str(), s[1].detail.as_str()), ("menu_root", "menuitem"));

        let js = "export class Foo extends Component {\n    setup() {\n        if (x) {\n        }\n    }\n}\nconst bar = async (a) => a;\n";
        let s = extract(js, "JavaScript", 4);
        let v: Vec<(&str, Kind)> = s.iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(v, [("Foo", Kind::Class), ("setup", Kind::Method), ("bar", Kind::Function)]);
        assert_eq!(s[0].end, 5);

        let md = "# Titre\ntexte\n## Partie\n```\n# pas un titre\n```\n# Autre\n";
        let s = extract(md, "Markdown", 4);
        let v: Vec<(&str, usize, usize)> = s.iter().map(|s| (s.name.as_str(), s.depth, s.end)).collect();
        assert_eq!(v, [("Titre", 0, 5), ("Partie", 1, 5), ("Autre", 0, 6)]);
    }

    #[test]
    fn blocks_by_indentation() {
        let lines: Vec<&str> = "fn a() {\n    x;\n\n    y;\n}\nz\n".split('\n').collect();
        assert_eq!(block_end(&lines, 0, 4), Some(4));
        assert_eq!(block_end(&lines, 1, 4), None);
        let py: Vec<&str> = "def f():\n    a\n\n    b\n\nc\n".split('\n').collect();
        assert_eq!(block_end(&py, 0, 4), Some(3));
    }
}
