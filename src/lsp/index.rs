//! Index maps every name, keyword, and attribute in a source back to file positions.

use crate::ast::{Attr, Rel, StmtKind, Value as AValue};
use crate::diag::{Source, Span};
use std::collections::HashMap;

/// Loc is a range in one file, in LSP coordinates (0-based lines, UTF-16 columns).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Loc {
    pub path: String,
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl Loc {
    pub fn range(&self) -> serde_json::Value {
        serde_json::json!({
            "start": { "line": self.start.0, "character": self.start.1 },
            "end": { "line": self.end.0, "character": self.end.1 },
        })
    }

    pub fn contains(&self, path: &str, line: usize, ch: usize) -> bool {
        self.path == path && (self.start..=self.end).contains(&(line, ch))
    }
}

/// locate maps a span of `src` back to the file that holds it.
pub fn locate(src: &Source, span: Span) -> Loc {
    let at = |off: usize| {
        let (line, col) = src.line_col(off);
        let units: usize = src
            .line_text(line)
            .chars()
            .take(col - 1)
            .map(char::len_utf16)
            .sum();
        let (path, n) = src.file_line(line);
        (path.to_string(), (n - 1, units))
    };
    let (path, start) = at(span.lo);
    let (end_path, end) = at(span.hi.max(span.lo));
    let end = if end_path == path { end } else { start };
    Loc { path, start, end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Node,
    Style,
    Macro,
}

/// Occ is one declaration or use of a node, style, or template name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Occ {
    pub kind: Kind,
    pub name: String,
    pub decl: bool,
    pub loc: Loc,
}

/// Word is a fixed-vocabulary token, tagged with what reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Word {
    Keyword(String),
    Key { stmt: &'static str, key: String },
    Value { key: String, text: String },
    Role(&'static str, String),
    Include(String),
}

#[derive(Clone, Debug)]
pub struct Spot {
    pub word: Word,
    pub loc: Loc,
}

/// Info holds the source text that declares a node or style, with its body lines.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub text: Vec<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Def {
    pub name: String,
    pub params: Vec<String>,
    pub doc: String,
}

/// Symbol is one outline entry.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub detail: String,
    pub kind: u32,
    pub whole: Loc,
    pub pick: Loc,
}

#[derive(Default)]
pub struct Index {
    pub occs: Vec<Occ>,
    pub spots: Vec<Spot>,
    pub nodes: HashMap<String, Info>,
    pub styles: HashMap<String, Info>,
    /// node_order and style_order keep declaration order for completion.
    pub node_order: Vec<String>,
    pub style_order: Vec<String>,
    pub defs: Vec<Def>,
    pub symbols: Vec<Symbol>,
}

/// Hit is whatever sits under the cursor.
pub enum Hit<'a> {
    Occ(&'a Occ),
    Spot(&'a Spot),
}

/// Files reads the text each location must match, preferring open buffers.
pub struct Files<'a> {
    pub open: &'a dyn Fn(&str) -> Option<String>,
    cache: HashMap<String, Option<String>>,
}

impl<'a> Files<'a> {
    pub fn new(open: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Files {
            open,
            cache: HashMap::new(),
        }
    }

    pub fn text(&mut self, path: &str) -> Option<&str> {
        let open = self.open;
        self.cache
            .entry(path.to_string())
            .or_insert_with(|| {
                open(path)
                    .or_else(|| crate::stdlib::at_path(path).map(str::to_string))
                    .or_else(|| std::fs::read_to_string(path).ok())
            })
            .as_deref()
    }

    /// shows reports whether `loc` covers exactly `want` in its file.
    pub fn shows(&mut self, loc: &Loc, want: &str) -> bool {
        if loc.start.0 != loc.end.0 {
            return false;
        }
        let Some(line) = self
            .text(&loc.path)
            .and_then(|t| t.lines().nth(loc.start.0))
        else {
            return false;
        };
        slice16(line, loc.start.1, loc.end.1) == Some(want)
    }
}

/// slice16 cuts `line` between two UTF-16 columns.
pub fn slice16(line: &str, lo: usize, hi: usize) -> Option<&str> {
    let byte = |u: usize| {
        let mut n = 0;
        for (i, c) in line.char_indices() {
            if n == u {
                return Some(i);
            }
            if n > u {
                return None;
            }
            n += c.len_utf16();
        }
        (n == u).then_some(line.len())
    };
    line.get(byte(lo)?..byte(hi)?)
}

fn utf16(s: &str) -> usize {
    s.encode_utf16().count()
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// template_name reads `name` or `MOD.name` from the start of `s`.
pub fn template_name(s: &str) -> String {
    let head: String = s.chars().take_while(|&c| is_ident(c)).collect();
    let tail: String = s[head.len()..]
        .strip_prefix('.')
        .map(|r| r.chars().take_while(|&c| is_ident(c)).collect())
        .unwrap_or_default();
    if head.is_empty() || tail.is_empty() {
        head
    } else {
        format!("{head}.{tail}")
    }
}

struct Builder<'s, 'f, 'o> {
    src: &'s Source,
    parse: &'s str,
    files: &'f mut Files<'o>,
    out: Index,
    path: String,
}

impl Builder<'_, '_, '_> {
    fn loc(&mut self, span: Span, want: &str) -> Option<Loc> {
        let l = locate(self.src, span);
        self.files.shows(&l, want).then_some(l)
    }

    fn occ(&mut self, kind: Kind, name: &crate::ast::Name, decl: bool) {
        if let Some(loc) = self.loc(name.span, &name.text) {
            self.out.occs.push(Occ {
                kind,
                name: name.text.clone(),
                decl,
                loc,
            });
        }
    }

    fn spot(&mut self, word: Word, span: Span, want: &str) {
        if let Some(loc) = self.loc(span, want) {
            self.out.spots.push(Spot { word, loc });
        }
    }

    fn keyword(&mut self, stmt: Span) {
        let text = self.parse.get(stmt.lo..stmt.hi).unwrap_or("");
        let w: String = text.chars().take_while(|&c| is_ident(c)).collect();
        if !w.is_empty() {
            let span = Span::new(stmt.lo, stmt.lo + w.len());
            self.spot(Word::Keyword(w.clone()), span, &w);
        }
    }

    fn attrs(&mut self, stmt: &'static str, attrs: &[Attr]) {
        for a in attrs {
            self.spot(
                Word::Key {
                    stmt,
                    key: a.key.text.clone(),
                },
                a.key.span,
                &a.key.text,
            );
            match &a.value {
                AValue::Name(n) if stmt == "layout" && a.key.text == "root" => {
                    self.occ(Kind::Node, n, false)
                }
                AValue::Name(n) => self.spot(
                    Word::Value {
                        key: a.key.text.clone(),
                        text: n.text.clone(),
                    },
                    n.span,
                    &n.text,
                ),
                _ => {}
            }
        }
    }

    fn classes(&mut self, cs: &[crate::ast::Name]) {
        for c in cs {
            if let Some(loc) = self.loc(c.span, &c.text) {
                self.out.occs.push(Occ {
                    kind: Kind::Style,
                    name: c.text.clone(),
                    decl: false,
                    loc,
                });
            }
        }
    }

    fn symbol(&mut self, name: String, detail: String, kind: u32, stmt: Span, pick: Span) {
        let (whole, pick) = (locate(self.src, stmt), locate(self.src, pick));
        if whole.path == self.path && pick.path == self.path {
            self.out.symbols.push(Symbol {
                name,
                detail,
                kind,
                whole,
                pick,
            });
        }
    }

    fn text(&self, span: Span) -> String {
        self.parse[span.lo..span.hi.min(self.parse.len())]
            .trim()
            .to_string()
    }
}

/// build indexes `text`, the buffer at `path`, with its includes and macros.
pub fn build(path: &str, text: &str, files: &mut Files) -> Index {
    let opts = crate::compile::Options {
        images: false,
        ..Default::default()
    };
    let crate::compile::Front { src, stmts, .. } = crate::compile::check(path, text, &opts);
    let parse = src.text.clone();
    files.cache.insert(path.to_string(), Some(text.to_string()));
    let mut b = Builder {
        src: &src,
        parse: &parse,
        files,
        out: Index::default(),
        path: path.to_string(),
    };
    for st in &stmts {
        let k = &st.kind;
        let keyword = !matches!(
            k,
            StmtKind::Node { .. }
                | StmtKind::Body { .. }
                | StmtKind::Row { .. }
                | StmtKind::Edge { .. }
        );
        if keyword {
            b.keyword(st.span);
        }
        match k {
            StmtKind::Body { owner, .. } | StmtKind::Row { owner, .. } => {
                let line = b.text(st.span);
                if let Some(o) = owner
                    && let Some(i) = b.out.nodes.get_mut(&o.text)
                {
                    i.text.push(line);
                }
                continue;
            }
            StmtKind::Title(_) => {}
            StmtKind::Open(c) => b.classes(std::slice::from_ref(c)),
            StmtKind::Root(n) => b.occ(Kind::Node, n, false),
            StmtKind::Layout { kind, attrs } => {
                b.spot(
                    Word::Role("layout", kind.text.clone()),
                    kind.span,
                    &kind.text,
                );
                b.attrs("layout", attrs);
            }
            StmtKind::Page { size, flags, attrs } => {
                if let AValue::Name(n) = size {
                    b.spot(Word::Role("page", n.text.clone()), n.span, &n.text);
                }
                for f in flags {
                    b.spot(Word::Role("flag", f.text.clone()), f.span, &f.text);
                }
                b.attrs("page", attrs);
            }
            StmtKind::Font { attrs, .. } => b.attrs("font", attrs),
            StmtKind::Theme { preset, attrs } => {
                if let Some(p) = preset {
                    b.spot(Word::Role("theme", p.text.clone()), p.span, &p.text);
                }
                b.attrs("theme", attrs);
            }
            StmtKind::Style { name, attrs } => {
                b.occ(Kind::Style, name, true);
                b.attrs("style", attrs);
                let text = b.text(st.span);
                if !b.out.styles.contains_key(&name.text) {
                    b.out.style_order.push(name.text.clone());
                    b.out.styles.insert(
                        name.text.clone(),
                        Info {
                            text: vec![text],
                            title: None,
                        },
                    );
                }
                let detail = attrs
                    .iter()
                    .map(|a| b.text(a.key.span.to(a.value.span())))
                    .collect::<Vec<_>>()
                    .join(" ");
                b.symbol(name.text.clone(), detail, 5, st.span, name.span);
                continue;
            }
            StmtKind::Node {
                id,
                title,
                classes: _,
                attrs,
                ..
            } => {
                b.occ(Kind::Node, id, true);
                b.attrs("node", attrs);
                let text = b.text(st.span);
                let fresh = !b.out.nodes.contains_key(&id.text);
                if fresh {
                    b.out.node_order.push(id.text.clone());
                    b.out.nodes.insert(
                        id.text.clone(),
                        Info {
                            text: vec![text],
                            title: title.as_ref().map(|t| t.0.clone()),
                        },
                    );
                }
                let detail = title.as_ref().map_or(String::new(), |t| t.0.clone());
                b.symbol(id.text.clone(), detail, 19, st.span, id.span);
                continue;
            }
            StmtKind::Key {
                label,
                classes: _,
                attrs,
            } => {
                b.attrs("key", attrs);
                b.symbol(label.clone(), "key".into(), 14, st.span, st.span);
            }
            StmtKind::Edge {
                from,
                to,
                classes: _,
                attrs,
                ..
            } => {
                b.occ(Kind::Node, from, false);
                for t in to {
                    b.occ(Kind::Node, t, false);
                }
                b.attrs("edge", attrs);
            }
            StmtKind::Group {
                id,
                title,
                items,
                classes: _,
                attrs,
            } => {
                if let Some(id) = id {
                    b.occ(Kind::Node, id, true);
                }
                for n in items {
                    b.occ(Kind::Node, n, false);
                }
                b.attrs("group", attrs);
                let names: Vec<&str> = items.iter().map(|n| n.text.as_str()).collect();
                let name = title.clone().unwrap_or_else(|| "group".into());
                b.symbol(name, names.join(", "), 3, st.span, st.span);
            }
            StmtKind::Constraint { hard, rel, attrs } => {
                match rel {
                    Rel::Chain { nodes, ops } => {
                        for n in nodes {
                            b.occ(Kind::Node, n, false);
                        }
                        for o in ops {
                            b.spot(Word::Role("relation", o.text.clone()), o.span, &o.text);
                        }
                    }
                    Rel::Align { side, items } => {
                        b.spot(Word::Role("side", side.text.clone()), side.span, &side.text);
                        for n in items {
                            b.occ(Kind::Node, n, false);
                        }
                    }
                    Rel::At { a, place } => {
                        b.occ(Kind::Node, a, false);
                        b.spot(
                            Word::Role("place", place.text.clone()),
                            place.span,
                            &place.text,
                        );
                    }
                }
                b.attrs(if *hard { "require" } else { "prefer" }, attrs);
            }
            StmtKind::Include(target) => {
                let text = parse.get(st.span.lo..st.span.hi).unwrap_or("");
                if let Some(q) = text.find('"') {
                    let lo = st.span.lo + q + 1;
                    let span = Span::new(lo, lo + target.len());
                    b.spot(Word::Include(target.clone()), span, target);
                }
            }
        }
    }
    macro_lines(&mut b, path, text);
    let mut out = b.out;
    out.occs.sort();
    out.occs.dedup();
    let mut seen = std::collections::HashSet::new();
    out.spots.retain(|s| seen.insert(s.loc.clone()));
    out
}

/// macro_lines indexes `def` and template call lines, which never reach the parser.
fn macro_lines(b: &mut Builder<'_, '_, '_>, path: &str, text: &str) {
    let (src, _) = crate::include::expand(path, text);
    let lines: Vec<&str> = src.text.lines().collect();
    let key = |module: Option<&str>, n: &str| match module {
        Some(m) if !n.contains('.') => format!("{m}.{n}"),
        _ => n.to_string(),
    };
    let names: std::collections::HashSet<String> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let n = template_name(l.trim_start().strip_prefix("def ")?.trim_start());
            (!n.is_empty()).then(|| key(src.module(i + 1), &n))
        })
        .collect();
    for (i, line) in lines.iter().enumerate() {
        let (file, n) = src.file_line(i + 1);
        let file = file.to_string();
        let lead = line.len() - line.trim_start().len();
        let t = &line[lead..];
        let module = src.module(i + 1);
        let at = |lo: usize, hi: usize| Loc {
            path: file.clone(),
            start: (n - 1, utf16(&line[..lo])),
            end: (n - 1, utf16(&line[..hi])),
        };
        let call = template_name(t);
        let after = &t[call.len()..];
        let row = after.starts_with('(')
            && !crate::parse::KEYWORDS.contains(&call.as_str())
            && !["->", "--", ".."]
                .iter()
                .any(|o| after.trim_start().starts_with(o))
            && (names.contains(&call) || names.contains(&key(module, &call)));
        let (w, name_lo) = if row {
            ("call".to_string(), lead)
        } else {
            let w: String = t.chars().take_while(|&c| is_ident(c)).collect();
            if !super::docs::MACRO_WORDS.contains(&w.as_str()) || t[w.len()..].starts_with('-') {
                continue;
            }
            b.out.spots.push(Spot {
                word: Word::Keyword(w.clone()),
                loc: at(lead, lead + w.len()),
            });
            if w != "def" {
                continue;
            }
            let rest = &t[w.len()..];
            (w, lead + (t.len() - rest.trim_start().len()))
        };
        let written = template_name(&line[name_lo..]);
        if written.is_empty() {
            continue;
        }
        let loc = at(name_lo, name_lo + written.len());
        let name = match module {
            Some(_) if !names.contains(&written) => key(module, &written),
            _ => written.clone(),
        };
        if w == "def" {
            let after = &line[name_lo + written.len()..];
            let params = after
                .trim_start()
                .strip_prefix('(')
                .and_then(|p| p.split(')').next())
                .map(|p| {
                    p.split(',')
                        .map(|s| s.split('=').next().unwrap().trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let mut doc = Vec::new();
            for prev in lines[..i].iter().rev() {
                match prev.trim_start().strip_prefix("//") {
                    Some(c) => doc.push(c.strip_prefix(' ').unwrap_or(c).to_string()),
                    None => break,
                }
            }
            doc.reverse();
            b.out.defs.push(Def {
                name: name.clone(),
                params,
                doc: doc.join("\n"),
            });
            if file == path {
                let whole = at(lead, line.len());
                b.out.symbols.push(Symbol {
                    name: name.clone(),
                    detail: line[name_lo + written.len()..].trim().to_string(),
                    kind: 12,
                    whole,
                    pick: loc.clone(),
                });
            }
        }
        b.out.occs.push(Occ {
            kind: Kind::Macro,
            name,
            decl: w == "def",
            loc,
        });
    }
}

impl Index {
    /// at finds the name or word under (line, ch) in `path`.
    pub fn at(&self, path: &str, line: usize, ch: usize) -> Option<Hit<'_>> {
        if let Some(o) = self.occs.iter().find(|o| o.loc.contains(path, line, ch)) {
            return Some(Hit::Occ(o));
        }
        self.spots
            .iter()
            .find(|s| s.loc.contains(path, line, ch))
            .map(Hit::Spot)
    }

    /// all lists every occurrence of one name.
    pub fn all(&self, kind: Kind, name: &str) -> impl Iterator<Item = &Occ> {
        self.occs
            .iter()
            .filter(move |o| o.kind == kind && o.name == name)
    }

    pub fn def(&self, name: &str) -> Option<&Def> {
        self.defs.iter().find(|d| d.name == name)
    }
}
