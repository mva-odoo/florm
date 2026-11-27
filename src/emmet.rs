//! Emmet: an abbreviation (`ul>li.item*3`, `field[name=partner_id]`) typed in HTML, JSX or XML and
//! turned into its tags by Tab. The result is a snippet (see `templates`): the empty attribute
//! values and contents are its places, Tab goes from one to the next.
//!
//! What is read: `>` child, `+` sibling, `^` back up, `*3` repeated, `$` its number (`$$` on two
//! digits), `(…)` a group, `#id`, `.class`, `[attr=value attr2]`, `{text}`, `!` an HTML page.

/// The kind of file the tags are written for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    Html,
    /// `className`, `<br />`.
    Jsx,
    /// Empty elements closed by `/>` where XML (and Odoo's views) expect it.
    Xml,
}

/// The flavor of a file by its extension, None where Emmet is not offered.
pub fn flavor(ext: &str) -> Option<Flavor> {
    match ext {
        "html" | "htm" | "xhtml" | "vue" | "svelte" | "php" | "erb" | "twig" | "jinja" | "j2" | "hbs" | "astro" => Some(Flavor::Html),
        "jsx" | "tsx" => Some(Flavor::Jsx),
        "xml" | "qweb" | "svg" => Some(Flavor::Xml),
        _ => None,
    }
}

#[derive(Default, Clone, Debug)]
struct Node {
    name: String,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
    text: Option<String>,
    children: Vec<Node>,
    count: usize,
    /// `(…)`: only its children are written.
    group: bool,
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"];
/// Odoo's elements usually written empty in its XML views and data.
const XML_EMPTY: &[&str] = &["field", "button", "menuitem", "filter", "separator", "newline", "img", "br", "hr", "input", "label", "act_window"];
const INLINE: &[&str] = &["a", "abbr", "b", "bdi", "bdo", "br", "cite", "code", "data", "em", "i", "img", "input", "kbd", "label", "mark", "q", "s", "small", "span", "strong", "sub", "sup", "time", "u", "var", "button", "select", "option", "textarea"];
/// Tags known without operators: a plain word expands only when it is one of them (any other word
/// followed by Tab is text, Tab indents).
const HTML_TAGS: &[&str] = &[
    "a", "abbr", "address", "article", "aside", "audio", "b", "blockquote", "body", "br", "button", "canvas", "caption", "code", "col", "dd", "details",
    "dialog", "div", "dl", "dt", "em", "fieldset", "figcaption", "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hr",
    "html", "i", "iframe", "img", "input", "label", "legend", "li", "link", "main", "meta", "nav", "ol", "option", "p", "pre", "section", "select",
    "small", "span", "strong", "summary", "table", "tbody", "td", "textarea", "tfoot", "th", "thead", "title", "tr", "ul", "video", "script", "style",
];
const ODOO_TAGS: &[&str] = &[
    "odoo", "data", "record", "field", "form", "tree", "list", "kanban", "search", "group", "notebook", "page", "sheet", "header", "footer", "button",
    "xpath", "template", "menuitem", "filter", "separator", "div", "span", "t", "templates", "function", "delete",
];

/// The tags of abbreviation `abbr` as a snippet, None when it is not one (or a plain word that is
/// not a known tag).
pub fn expand(abbr: &str, flavor: Flavor, indent_unit: &str) -> Option<String> {
    let abbr = abbr.trim();
    if abbr.is_empty() {
        return None;
    }
    if abbr == "!" {
        return (flavor == Flavor::Html).then(|| HTML5.replace("    ", indent_unit));
    }
    let plain = abbr.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_');
    if plain {
        let known = match flavor {
            Flavor::Xml => ODOO_TAGS.contains(&abbr) || HTML_TAGS.contains(&abbr),
            _ => HTML_TAGS.contains(&abbr) || (flavor == Flavor::Jsx && abbr.starts_with(|c: char| c.is_ascii_uppercase())),
        };
        if !known {
            return None;
        }
    }
    let chars: Vec<char> = abbr.chars().collect();
    let mut p = Parser { c: &chars, i: 0 };
    let nodes = p.group()?;
    if p.i < chars.len() || nodes.is_empty() {
        return None;
    }
    let mut w = Writer { flavor, unit: indent_unit.to_string(), out: String::new(), stop: 0 };
    w.nodes(&nodes, None, 0, &[]);
    Some(w.out.trim_end_matches('\n').to_string())
}

/// The abbreviation right before char `col` of `line`: from the last space outside brackets and
/// braces. None inside a tag being written (`<div cl|`) or when nothing is there.
pub fn abbreviation_before(line: &str, col: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = line.chars().take(col).collect();
    let (mut depth_sq, mut depth_br, mut start) = (0i32, 0i32, chars.len());
    while start > 0 {
        let c = chars[start - 1];
        match c {
            ']' => depth_sq += 1,
            '[' => depth_sq -= 1,
            '}' => depth_br += 1,
            '{' => depth_br -= 1,
            _ => {}
        }
        if depth_sq < 0 || depth_br < 0 {
            return None;
        }
        let inside = depth_sq > 0 || depth_br > 0;
        if !inside && (c.is_whitespace() || c == '<' || c == '"' || c == '\'' || c == '`' || c == ';' || c == ',') {
            break;
        }
        start -= 1;
    }
    let abbr: String = chars[start..].iter().collect();
    if abbr.is_empty() || !abbr.starts_with(|c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '#' | '[' | '(' | '{' | '!')) {
        return None;
    }
    // inside a tag being written (`<div cl`), or right after one (`<b>x`: not an abbreviation)
    let before: String = chars[..start].iter().collect();
    let open = before.rfind('<');
    let close = before.rfind('>');
    if open.is_some_and(|o| close.is_none_or(|c| c < o)) {
        return None;
    }
    Some((start, abbr))
}

struct Parser<'a> {
    c: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    /// Siblings and their children, up to the end or a `)`.
    fn group(&mut self) -> Option<Vec<Node>> {
        let mut root: Vec<Node> = Vec::new();
        // path of child indexes from `root` to the parent of the next element
        let mut path: Vec<usize> = Vec::new();
        loop {
            let node = self.item()?;
            let parent = children_at(&mut root, &path);
            parent.push(node);
            let last = parent.len() - 1;
            match self.peek() {
                Some('>') => {
                    self.i += 1;
                    path.push(last);
                }
                Some('+') => self.i += 1,
                Some('^') => {
                    while self.peek() == Some('^') {
                        self.i += 1;
                        path.pop();
                    }
                }
                _ => return Some(root),
            }
        }
    }

    /// An element or a group, with its `*N`.
    fn item(&mut self) -> Option<Node> {
        let mut node = if self.peek() == Some('(') {
            self.i += 1;
            let children = self.group()?;
            if self.peek() != Some(')') {
                return None;
            }
            self.i += 1;
            Node { children, group: true, ..Default::default() }
        } else {
            self.element()?
        };
        node.count = 1;
        if self.peek() == Some('*') {
            self.i += 1;
            let start = self.i;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
            node.count = self.c[start..self.i].iter().collect::<String>().parse().ok().filter(|n| *n > 0 && *n <= 500)?;
        }
        Some(node)
    }

    fn word(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '$' | '@' | '!')) {
            self.i += 1;
        }
        self.c[start..self.i].iter().collect()
    }

    fn element(&mut self) -> Option<Node> {
        let mut n = Node { name: self.word(), ..Default::default() };
        loop {
            match self.peek() {
                Some('#') => {
                    self.i += 1;
                    n.id = Some(self.word());
                }
                Some('.') => {
                    self.i += 1;
                    let c = self.word();
                    if c.is_empty() {
                        return None;
                    }
                    n.classes.push(c);
                }
                Some('[') => {
                    self.i += 1;
                    self.attrs(&mut n)?;
                }
                Some('{') => {
                    self.i += 1;
                    let start = self.i;
                    while self.peek().is_some_and(|c| c != '}') {
                        self.i += 1;
                    }
                    self.peek()?;
                    n.text = Some(self.c[start..self.i].iter().collect());
                    self.i += 1;
                }
                _ => break,
            }
        }
        let empty = n.name.is_empty() && n.id.is_none() && n.classes.is_empty() && n.attrs.is_empty();
        if empty && n.text.is_none() {
            return None;
        }
        Some(n)
    }

    /// `[a=1 b="x y" c]` (the `[` read).
    fn attrs(&mut self, n: &mut Node) -> Option<()> {
        loop {
            while self.peek() == Some(' ') {
                self.i += 1;
            }
            match self.peek()? {
                ']' => {
                    self.i += 1;
                    return Some(());
                }
                _ => {
                    let start = self.i;
                    while self.peek().is_some_and(|c| !matches!(c, '=' | ' ' | ']')) {
                        self.i += 1;
                    }
                    let name: String = self.c[start..self.i].iter().collect();
                    let value = if self.peek() == Some('=') {
                        self.i += 1;
                        Some(match self.peek()? {
                            q @ ('"' | '\'') => {
                                self.i += 1;
                                let s = self.i;
                                while self.peek().is_some_and(|c| c != q) {
                                    self.i += 1;
                                }
                                self.peek()?;
                                let v: String = self.c[s..self.i].iter().collect();
                                self.i += 1;
                                v
                            }
                            _ => {
                                // brackets inside the value (an XPath) are its own
                                let (s, mut depth) = (self.i, 0);
                                while let Some(c) = self.peek() {
                                    match c {
                                        '[' => depth += 1,
                                        ']' if depth == 0 => break,
                                        ']' => depth -= 1,
                                        ' ' if depth == 0 => break,
                                        _ => {}
                                    }
                                    self.i += 1;
                                }
                                self.c[s..self.i].iter().collect()
                            }
                        })
                    } else {
                        None
                    };
                    if name.is_empty() {
                        return None;
                    }
                    n.attrs.push((name, value));
                }
            }
        }
    }
}

fn children_at<'a>(root: &'a mut Vec<Node>, path: &[usize]) -> &'a mut Vec<Node> {
    let mut cur = root;
    for &k in path {
        cur = &mut cur[k].children;
    }
    cur
}

/// `$` (`$$`… padded, `$@-` counting down) replaced by the number of the repetition.
fn numbered(s: &str, nums: &[(usize, usize)]) -> String {
    let Some(&(k, total)) = nums.last() else { return s.replace('$', "1") };
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            let mut w = 0;
            while i < chars.len() && chars[i] == '$' {
                w += 1;
                i += 1;
            }
            let n = if chars.get(i) == Some(&'@') && chars.get(i + 1) == Some(&'-') {
                i += 2;
                total - k
            } else {
                k + 1
            };
            out.push_str(&format!("{n:0w$}"));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Text written as is in the snippet (its `$`, `}` and `\` escaped).
fn lit(s: &str) -> String {
    s.replace('\\', "\\\\").replace('$', "\\$").replace('}', "\\}")
}

struct Writer {
    flavor: Flavor,
    unit: String,
    out: String,
    /// The last tab stop written.
    stop: usize,
}

impl Writer {
    fn stop(&mut self) -> String {
        self.stop += 1;
        format!("${{{}}}", self.stop)
    }

    fn nodes(&mut self, nodes: &[Node], parent: Option<&str>, level: usize, nums: &[(usize, usize)]) {
        for n in nodes {
            for k in 0..n.count {
                let mut nums = nums.to_vec();
                if n.count > 1 {
                    nums.push((k, n.count));
                }
                if n.group {
                    self.nodes(&n.children, parent, level, &nums);
                } else {
                    self.node(n, parent, level, &nums);
                }
            }
        }
    }

    /// The default attributes of a tag (filled when not given).
    fn defaults(&self, name: &str) -> &'static [(&'static str, &'static str)] {
        match (self.flavor, name) {
            (Flavor::Xml, "field") => &[("name", "")],
            (Flavor::Xml, "button") => &[("name", ""), ("type", "object"), ("string", "")],
            (Flavor::Xml, "record") => &[("id", ""), ("model", "")],
            (Flavor::Xml, "menuitem") => &[("id", ""), ("name", ""), ("action", "")],
            (Flavor::Xml, "xpath") => &[("expr", ""), ("position", "inside")],
            (Flavor::Xml, "filter") => &[("name", ""), ("string", ""), ("domain", "")],
            (Flavor::Xml, "template") => &[("id", "")],
            (Flavor::Xml, "page") => &[("string", "")],
            (_, "a") => &[("href", "")],
            (_, "img") => &[("src", ""), ("alt", "")],
            (_, "input") => &[("type", "text")],
            (_, "link") => &[("rel", "stylesheet"), ("href", "")],
            (_, "label") => &[("for", "")],
            (_, "form") if self.flavor != Flavor::Xml => &[("action", "")],
            (_, "option") => &[("value", "")],
            _ => &[],
        }
    }

    fn node(&mut self, n: &Node, parent: Option<&str>, level: usize, nums: &[(usize, usize)]) {
        let name = if n.name.is_empty() {
            match parent {
                Some("ul" | "ol") => "li",
                Some("table" | "tbody" | "thead" | "tfoot") => "tr",
                Some("tr") => "td",
                Some("select" | "optgroup") => "option",
                _ => if n.text.is_some() && n.id.is_none() && n.classes.is_empty() && n.attrs.is_empty() { "" } else { "div" },
            }
            .to_string()
        } else {
            numbered(&n.name, nums)
        };
        let pad = self.unit.repeat(level);
        // a bare {text}: text only
        if name.is_empty() {
            let t = n.text.as_deref().map(|t| lit(&numbered(t, nums))).unwrap_or_default();
            self.out.push_str(&format!("{pad}{t}\n"));
            return;
        }
        let mut attrs = String::new();
        if let Some(id) = &n.id {
            attrs.push_str(&format!(" id=\"{}\"", lit(&numbered(id, nums))));
        }
        if !n.classes.is_empty() {
            let key = if self.flavor == Flavor::Jsx { "className" } else { "class" };
            let v: Vec<String> = n.classes.iter().map(|c| lit(&numbered(c, nums))).collect();
            attrs.push_str(&format!(" {key}=\"{}\"", v.join(" ")));
        }
        let mut given: Vec<(String, Option<String>)> = n.attrs.clone();
        for (k, v) in self.defaults(&name) {
            if !given.iter().any(|(g, _)| g == k) {
                given.push((k.to_string(), (!v.is_empty()).then(|| v.to_string())));
            }
        }
        for (k, v) in &given {
            let k = if self.flavor == Flavor::Jsx && k == "class" { "className".to_string() } else if self.flavor == Flavor::Jsx && k == "for" { "htmlFor".to_string() } else { k.clone() };
            let v = match v {
                Some(v) => lit(&numbered(v, nums)),
                None => self.stop(),
            };
            attrs.push_str(&format!(" {k}=\"{v}\""));
        }
        let void = match self.flavor {
            Flavor::Xml => XML_EMPTY.contains(&name.as_str()) && n.children.is_empty() && n.text.is_none(),
            _ => VOID.contains(&name.as_str()),
        };
        if void {
            let end = match self.flavor {
                Flavor::Html => ">",
                Flavor::Jsx => " />",
                Flavor::Xml => "/>",
            };
            self.out.push_str(&format!("{pad}<{name}{attrs}{end}\n"));
            return;
        }
        let text = n.text.as_deref().map(|t| lit(&numbered(t, nums)));
        if n.children.is_empty() {
            let inner = text.unwrap_or_else(|| self.stop());
            self.out.push_str(&format!("{pad}<{name}{attrs}>{inner}</{name}>\n"));
            return;
        }
        // inline children on the line of their parent, the others under it
        let all_inline = n.children.iter().all(|c| !c.group && INLINE.contains(&c.name.as_str()) && c.children.is_empty() && c.count == 1);
        if all_inline {
            let mut w = Writer { flavor: self.flavor, unit: self.unit.clone(), out: String::new(), stop: self.stop };
            w.nodes(&n.children, Some(&name), 0, nums);
            self.stop = w.stop;
            let inner: String = w.out.lines().collect();
            self.out.push_str(&format!("{pad}<{name}{attrs}>{}{inner}</{name}>\n", text.unwrap_or_default()));
            return;
        }
        self.out.push_str(&format!("{pad}<{name}{attrs}>{}\n", text.unwrap_or_default()));
        self.nodes(&n.children, Some(&name), level + 1, nums);
        self.out.push_str(&format!("{pad}</{name}>\n"));
    }
}

const HTML5: &str = "<!DOCTYPE html>\n<html lang=\"${1:fr}\">\n<head>\n    <meta charset=\"UTF-8\">\n    <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n    <title>${2:Document}</title>\n</head>\n<body>\n    $0\n</body>\n</html>";

#[cfg(test)]
mod tests {
    use super::*;

    fn html(a: &str) -> String {
        expand(a, Flavor::Html, "  ").unwrap()
    }

    #[test]
    fn children_siblings_and_repeats() {
        assert_eq!(html("ul>li.item$*3"), "<ul>\n  <li class=\"item1\">${1}</li>\n  <li class=\"item2\">${2}</li>\n  <li class=\"item3\">${3}</li>\n</ul>");
        assert_eq!(html("div#main.a.b+p{Hi $}"), "<div id=\"main\" class=\"a b\">${1}</div>\n<p>Hi 1</p>");
        // ^ goes back one level: footer next to ul, in nav; ^^ two
        assert_eq!(html("nav>ul>li^footer"), "<nav>\n  <ul>\n    <li>${1}</li>\n  </ul>\n  <footer>${2}</footer>\n</nav>");
        assert_eq!(html("nav>ul>li^^footer"), "<nav>\n  <ul>\n    <li>${1}</li>\n  </ul>\n</nav>\n<footer>${2}</footer>");
        assert_eq!(html("(dt+dd)*2"), "<dt>${1}</dt>\n<dd>${2}</dd>\n<dt>${3}</dt>\n<dd>${4}</dd>");
        // implicit tags, attributes, inline children, void elements
        assert_eq!(html(".card>.title"), "<div class=\"card\">\n  <div class=\"title\">${1}</div>\n</div>");
        assert_eq!(html("ol>.x"), "<ol>\n  <li class=\"x\">${1}</li>\n</ol>");
        assert_eq!(html("a[href=/ title='A b']"), "<a href=\"/\" title=\"A b\">${1}</a>");
        assert_eq!(html("p>a+b"), "<p><a href=\"${1}\">${2}</a><b>${3}</b></p>");
        assert_eq!(html("img"), "<img src=\"${1}\" alt=\"${2}\">");
        assert_eq!(html("li.n$$@-*2"), "<li class=\"n02\">${1}</li>\n<li class=\"n01\">${2}</li>");
        assert!(html("!").starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn jsx_and_odoo_xml() {
        assert_eq!(expand("div.row>input+label", Flavor::Jsx, "  ").unwrap(), "<div className=\"row\"><input type=\"text\" /><label htmlFor=\"${1}\">${2}</label></div>");
        assert_eq!(expand("div.row>p", Flavor::Jsx, "  ").unwrap(), "<div className=\"row\">\n  <p>${1}</p>\n</div>");
        let x = |a: &str| expand(a, Flavor::Xml, "    ").unwrap();
        assert_eq!(x("field[name=partner_id]"), "<field name=\"partner_id\"/>");
        assert_eq!(x("group>field*2"), "<group>\n    <field name=\"${1}\"/>\n    <field name=\"${2}\"/>\n</group>");
        assert_eq!(x("button"), "<button name=\"${1}\" type=\"object\" string=\"${2}\"/>");
        assert_eq!(x("xpath[expr=//field[@name='x']]"), "<xpath expr=\"//field[@name='x']\" position=\"inside\">${1}</xpath>");
    }

    #[test]
    fn not_abbreviations() {
        // a plain word that is not a tag: text, Tab indents
        assert!(expand("hello", Flavor::Html, "  ").is_none());
        assert!(expand("partner", Flavor::Xml, "  ").is_none());
        assert!(expand("ul>", Flavor::Html, "  ").is_none());
        assert!(expand("a[x", Flavor::Html, "  ").is_none());
        assert!(expand("li*0", Flavor::Html, "  ").is_none());
        assert_eq!(expand("MyCard", Flavor::Jsx, "  ").unwrap(), "<MyCard>${1}</MyCard>");
    }

    #[test]
    fn the_abbreviation_before_the_cursor() {
        let line = "    <div>ul>li.a*2";
        assert_eq!(abbreviation_before(line, line.chars().count()), None, "glued to a tag");
        let line = "    ul>li[title=\"a b\"]";
        assert_eq!(abbreviation_before(line, line.chars().count()), Some((4, "ul>li[title=\"a b\"]".into())));
        let line = "<p>Text and a.b";
        assert_eq!(abbreviation_before(line, line.chars().count()), Some((12, "a.b".into())));
        let line = "<div cl";
        assert_eq!(abbreviation_before(line, line.chars().count()), None, "inside a tag");
        assert_eq!(abbreviation_before("  ", 2), None);
    }
}
