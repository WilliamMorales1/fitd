//! fix: rewrites a file in the indentation syntax into the brace syntax.

use crate::ast::StmtKind;
use crate::parse::KEYWORDS;
use std::collections::HashSet;

/// Names is what the whole program, includes and expansions too, declares.
struct Names {
    styles: HashSet<String>,
    templates: HashSet<String>,
    nodes: HashSet<String>,
    module: Option<String>,
}

impl Names {
    fn of(path: &str, text: &str) -> Names {
        let module = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|m| crate::stdlib::get(m).is_some_and(|src| src == text))
            .map(String::from);
        let (src, _) = crate::include::expand(path, text);
        let (mut styles, mut templates) = (HashSet::new(), HashSet::new());
        for (k, l) in src.text.lines().enumerate() {
            let t = l.trim();
            if let Some(r) = t.strip_prefix("style ") {
                styles.extend(r.split_whitespace().next().map(String::from));
            }
            if let Some(r) = t.strip_prefix("def ") {
                let n: String = r
                    .trim_start()
                    .chars()
                    .take_while(|&c| c.is_alphanumeric() || c == '_')
                    .collect();
                let m = src.module(k + 1).map(String::from).or(module.clone());
                templates.insert(match m {
                    Some(m) => format!("{m}.{n}"),
                    None => n,
                });
            }
        }
        let loaded = crate::expand::load(path, text, 1_000_000);
        let nodes = crate::parse::parse_tokens(&loaded.src.text, loaded.toks, Vec::new())
            .0
            .into_iter()
            .filter_map(|s| match s.kind {
                // A lone name opened a style block in the old syntax, so it declares nothing.
                StmtKind::Node {
                    id, bare: false, ..
                } => Some(id.text),
                _ => None,
            })
            .collect();
        Names {
            styles,
            templates,
            nodes,
            module,
        }
    }

    /// calls reports whether a line opening with `head` calls a template, as the macro layer decides.
    fn calls(&self, head: &str, rest: &str, top: bool) -> bool {
        let r = rest.trim_start();
        let ok = match head.split_once('.') {
            Some((m, n)) => ident(m) && ident(n) && self.templates.contains(head),
            None => {
                ident(head)
                    && !KEYWORDS.contains(&head)
                    && (self.templates.contains(head)
                        || self
                            .module
                            .as_ref()
                            .is_some_and(|m| self.templates.contains(&format!("{m}.{head}"))))
            }
        };
        let style = r.is_empty() && (self.module.is_some() || !top) && self.styles.contains(head);
        ok && !style && !["->", "--", ".."].iter().any(|o| r.starts_with(o))
    }
}

fn ident(w: &str) -> bool {
    !w.is_empty()
        && !w.starts_with(|c: char| c.is_ascii_digit())
        && w.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// name reports whether `w` can be a node or style name, interpolations included.
fn name(w: &str) -> bool {
    !w.is_empty()
        && !w.starts_with(|c: char| c.is_ascii_digit() || c == '-')
        && strip_interp(w)
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn strip_interp(w: &str) -> String {
    let mut o = String::new();
    let mut depth = 0;
    let mut cs = w.chars().peekable();
    while let Some(c) = cs.next() {
        if c == '$' && cs.peek() == Some(&'{') {
            cs.next();
            depth += 1;
            o.push('x');
        } else if c == '}' && depth > 0 {
            depth -= 1;
        } else if depth == 0 {
            o.push(c);
        }
    }
    o
}

/// words splits `s` at spaces outside strings and `${...}`, giving each word with its byte offset.
fn words(s: &str) -> Vec<(usize, &str)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let lo = i;
        let (mut in_str, mut interp) = (false, 0usize);
        while i < b.len() && (in_str || interp > 0 || !b[i].is_ascii_whitespace()) {
            match b[i] {
                b'\\' if in_str => i += 1,
                b'"' if interp == 0 => in_str = !in_str,
                b'$' if b.get(i + 1) == Some(&b'{') => {
                    interp += 1;
                    i += 1;
                }
                b'}' if interp > 0 => interp -= 1,
                _ => {}
            }
            i += 1;
        }
        let hi = i.min(b.len());
        out.push((lo, &s[lo..hi]));
    }
    out
}

/// attr reports whether word `w` is one `key=value` attribute.
fn attr(w: &str) -> bool {
    match w.split_once('=') {
        Some((k, v)) => {
            !v.is_empty()
                && !k.is_empty()
                && !k.starts_with(|c: char| c.is_ascii_digit() || c == '-')
                && k.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        }
        None => false,
    }
}

/// bracket wraps the trailing `key=value` words of statement `code` in `[...]`.
fn bracket(code: &str) -> String {
    let ws = words(code);
    let k = ws
        .iter()
        .rposition(|(_, w)| !attr(w))
        .map_or(0, |k| k + 1)
        .max(1);
    if k >= ws.len() {
        return code.to_string();
    }
    let attrs: Vec<&str> = ws[k..].iter().map(|&(_, w)| w).collect();
    format!("{} [{}]", code[..ws[k].0].trim_end(), attrs.join(", "))
}

/// cells rewrites the text after `~` as a comma list, or an image row with bracketed attributes.
fn cells(rest: &str) -> String {
    let ws = words(rest);
    if ws.first().is_some_and(|(_, w)| *w == "image") {
        return bracket(rest);
    }
    let cs: Vec<&str> = ws.iter().map(|&(_, w)| w).collect();
    format!("[{}]", cs.join(", "))
}

enum Kind {
    Blank,
    Note,
    Stmt,
    /// Block always prints braces: `def`, `for`, `if`, `else`.
    Block,
}

struct Item {
    head: String,
    note: String,
    kind: Kind,
    kids: Vec<usize>,
}

struct Frame {
    col: usize,
    item: usize,
    block: bool,
}

struct Fix<'a> {
    names: &'a Names,
    items: Vec<Item>,
    stack: Vec<Frame>,
    last_node: Option<usize>,
}

impl Fix<'_> {
    fn add(&mut self, to: usize, head: String, note: &str, kind: Kind) -> usize {
        self.items.push(Item {
            head,
            note: note.trim().to_string(),
            kind,
            kids: Vec::new(),
        });
        let id = self.items.len() - 1;
        self.items[to].kids.push(id);
        id
    }

    fn here(&self) -> usize {
        self.stack.last().unwrap().item
    }

    /// tail takes the trailing blank and comment lines off item `it`.
    fn tail(&mut self, it: usize) -> Vec<usize> {
        let mut tail = Vec::new();
        while let Some(&k) = self.items[it].kids.last() {
            if !matches!(self.items[k].kind, Kind::Blank | Kind::Note) {
                break;
            }
            tail.insert(0, k);
            self.items[it].kids.pop();
        }
        tail
    }

    /// pop closes the top frame; an indentation block's trailing blank and comment lines move out to the parent.
    fn pop(&mut self) {
        let f = self.stack.pop().unwrap();
        if !f.block {
            let tail = self.tail(f.item);
            let parent = self.here();
            self.items[parent].kids.extend(tail);
        }
    }

    /// offside closes the indentation blocks that a line at `col` ends, up to the innermost `def`/`for`/`if`.
    fn offside(&mut self, col: usize) {
        while self.stack.len() > 1 && self.stack.last().is_some_and(|f| !f.block && f.col >= col) {
            self.pop();
        }
    }

    fn open(&mut self, col: usize, item: usize, block: bool) {
        self.stack.push(Frame { col, item, block });
    }

    fn line(&mut self, lines: &[&str], i: &mut usize) {
        let raw = lines[*i];
        *i += 1;
        let col = raw.len() - raw.trim_start().len();
        let t = raw.trim();
        let here = self.here();
        if t.is_empty() {
            self.add(here, String::new(), "", Kind::Blank);
            return;
        }
        if t.starts_with("//") {
            self.add(here, t.to_string(), "", Kind::Note);
            return;
        }
        if t.starts_with('|') || t.starts_with('~') {
            let to = self.last_node.unwrap_or(here);
            let head = match t.strip_prefix('~') {
                Some(rest) => format!("~ {}", cells(rest.trim())),
                None => t.to_string(),
            };
            self.add(to, head, "", Kind::Stmt);
            return;
        }
        let cut = crate::fmt::code_end(t);
        let (code, note) = (t[..cut].trim_end(), &t[cut..]);
        let first = code.split_whitespace().next().unwrap_or("");
        let rest = code[first.len()..].trim_start();
        if let Some(kw) = keyword(code) {
            match kw {
                "end" => {
                    while self.stack.len() > 1 && !self.stack.last().unwrap().block {
                        self.pop();
                    }
                    if self.stack.len() > 1 {
                        self.pop();
                    }
                }
                "else" => {
                    while self.stack.len() > 1 && !self.stack.last().unwrap().block {
                        self.pop();
                    }
                    if self.stack.len() > 1 {
                        self.pop();
                    }
                    let to = self.here();
                    let it = self.add(to, code.to_string(), note, Kind::Block);
                    self.open(col, it, true);
                }
                _ => {
                    self.offside(col);
                    let to = self.here();
                    let it = self.add(to, code.to_string(), note, Kind::Block);
                    self.open(col, it, true);
                }
            }
            self.last_node = None;
            return;
        }
        self.offside(col);
        let to = self.here();
        let top = !self.stack.iter().any(|f| !f.block);
        if self.names.calls(first, rest, top) {
            self.rows(lines, i, col, code, note);
            self.last_node = None;
            return;
        }
        if rest.is_empty() && name(first) {
            let reopen = self.names.nodes.contains(first) && !self.names.styles.contains(first);
            if reopen {
                let it = self.add(to, first.to_string(), note, Kind::Stmt);
                self.open(col, it, false);
                return;
            }
            if self.names.styles.contains(first) || top {
                let it = self.add(to, format!("with {first}"), note, Kind::Stmt);
                self.open(col, it, false);
                return;
            }
        }
        let ws = words(code);
        let bare = ws
            .first()
            .is_some_and(|(_, w)| name(w) && !KEYWORDS.contains(w))
            && ws.get(1).is_none_or(|(_, w)| w.starts_with('"') || attr(w));
        if bare || first == "node" && ws.len() > 1 {
            // An ID that is a keyword, or interpolated and so maybe one, keeps its `node`.
            let keep = !bare && (KEYWORDS.contains(&ws[1].1) || ws[1].1.contains("${"));
            let code = if bare || keep { code } else { rest };
            let it = self.add(to, bracket(code), note, Kind::Stmt);
            self.open(col, it, false);
            self.last_node = Some(it);
            return;
        }
        self.add(to, bracket(code), note, Kind::Stmt);
        self.last_node = None;
    }

    /// rows reads a template call and the rows indented under it, which nest by indent.
    fn rows(&mut self, lines: &[&str], i: &mut usize, col: usize, code: &str, note: &str) {
        let to = self.here();
        let opener = self.add(to, code.to_string(), note, Kind::Stmt);
        let mut cols: Vec<(usize, usize)> = vec![(col, opener)];
        while *i < lines.len() {
            let raw = lines[*i];
            let t = raw.trim();
            let c = raw.len() - raw.trim_start().len();
            if !(t.is_empty() || t.starts_with("//")) && c <= col {
                break;
            }
            *i += 1;
            let at = cols.last().unwrap().1;
            if t.is_empty() {
                self.add(at, String::new(), "", Kind::Blank);
                continue;
            }
            if t.starts_with("//") {
                self.add(at, t.to_string(), "", Kind::Note);
                continue;
            }
            while cols.len() > 1 && cols.last().is_some_and(|&(x, _)| x >= c) {
                cols.pop();
            }
            let parent = cols.last().unwrap().1;
            let cut = crate::fmt::code_end(t);
            let row = self.add(
                parent,
                t[..cut].trim_end().to_string(),
                &t[cut..],
                Kind::Stmt,
            );
            cols.push((c, row));
        }
        // Trailing blank and comment lines belong after the call.
        for &(_, it) in cols.iter().rev() {
            let tail = self.tail(it);
            self.items[to].kids.extend(tail);
        }
    }

    fn print(&self, it: usize, depth: usize, o: &mut Vec<String>) {
        let pad = "  ".repeat(depth);
        for &k in &self.items[it].kids {
            let item = &self.items[k];
            match item.kind {
                Kind::Blank => o.push(String::new()),
                Kind::Note => o.push(format!("{pad}{}", item.head)),
                Kind::Stmt | Kind::Block => {
                    let note = if item.note.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", item.note)
                    };
                    let braces = matches!(item.kind, Kind::Block) || !item.kids.is_empty();
                    if !braces {
                        o.push(format!("{pad}{}{note}", item.head));
                        continue;
                    }
                    let open = format!("{} {{{note}", item.head);
                    match o.last_mut() {
                        Some(l) if item.head.starts_with("else") && l.trim() == "}" => {
                            *l = format!("{pad}}} {open}");
                        }
                        _ => o.push(format!("{pad}{open}")),
                    }
                    self.print(k, depth + 1, o);
                    o.push(format!("{pad}}}"));
                }
            }
        }
    }
}

/// keyword gives the macro directive that `code` opens with, if any.
fn keyword(code: &str) -> Option<&str> {
    let w = code
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("");
    if !matches!(w, "for" | "if" | "else" | "end" | "def") {
        return None;
    }
    let rest = &code[w.len()..];
    let r = rest.trim_start();
    if rest.starts_with('-') || ["->", "--", ".."].iter().any(|o| r.starts_with(o)) {
        return None;
    }
    Some(w)
}

/// fix rewrites old-syntax `text` from file `path` into brace syntax: braces for blocks, `with` for style blocks, `[...]` for attributes and figure rows.
pub fn fix(path: &str, text: &str) -> String {
    let names = Names::of(path, text);
    let mut f = Fix {
        names: &names,
        items: vec![Item {
            head: String::new(),
            note: String::new(),
            kind: Kind::Block,
            kids: Vec::new(),
        }],
        stack: vec![Frame {
            col: 0,
            item: 0,
            block: true,
        }],
        last_node: None,
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        f.line(&lines, &mut i);
    }
    while f.stack.len() > 1 {
        f.pop();
    }
    let mut o = Vec::new();
    f.print(0, 0, &mut o);
    crate::fmt::fmt(&(o.join("\n") + "\n"))
}

#[cfg(test)]
mod tests {
    use super::fix;

    fn run(src: &str) -> String {
        fix("t.fitd", src)
    }

    #[test]
    fn nesting_becomes_braces() {
        let src = "layout radial root=a\na \"A\" color=#f00\n  b \"B\"\n    c\n  d\nnode e\n";
        assert_eq!(
            run(src),
            "layout radial [root=a]\n\na \"A\" [color=#f00] {\n  b \"B\" {\n    c\n  }\n  d\n}\ne\n"
        );
    }

    #[test]
    fn style_blocks_take_with() {
        let src = "style io shape=box\nio\n  a \"A\"\n    -> b\na -> b \"x\" line=dashed\n";
        assert_eq!(
            run(src),
            "style io [shape=box]\nwith io {\n  a \"A\" {\n    -> b\n  }\n}\na -> b \"x\" [line=dashed]\n"
        );
    }

    #[test]
    fn body_and_cells_join_their_node() {
        let src =
            "node a \"A\"\n| one\n~ %100 . \"d 64\"*2 _\nnode b\n~ image \"p.png\" width=9em\n";
        assert_eq!(
            run(src),
            "a \"A\" {\n  | one\n  ~ [%100, ., \"d 64\"*2, _]\n}\nb {\n  ~ image \"p.png\" [width=9em]\n}\n"
        );
    }

    #[test]
    fn directives_take_braces() {
        let src = "def t(a, b)\nnode ${a} \"${b}\"\nend\nfor i in 1..2\nif i == 1\nn${i}\nelse\nm${i}\nend\nend\nt x X\n";
        assert_eq!(
            run(src),
            "def t(a, b) {\n  node ${a} \"${b}\"\n}\nfor i in 1..2 {\n  if i == 1 {\n    with n${i}\n  } else {\n    with m${i}\n  }\n}\nt x X\n"
        );
    }

    #[test]
    fn template_rows_nest() {
        let src = "include \"tree\"\ntree.unit ceo Ada CEO\n  cto Lin CTO // tech\n    web \"Web team\" \"6\"\n  cfo Sam CFO\n\n// next\numl\n";
        assert_eq!(
            run(src),
            "include \"tree\"\n\ntree.unit ceo Ada CEO {\n  cto Lin CTO { // tech\n    web \"Web team\" \"6\"\n  }\n  cfo Sam CFO\n}\n\n// next\nwith uml\n"
        );
    }

    #[test]
    fn comments_leave_blocks() {
        let src = "a\n  b\n\n// about c\nnode c\n";
        assert_eq!(run(src), "with a {\n  b\n}\n\n// about c\nc\n");
    }
}
