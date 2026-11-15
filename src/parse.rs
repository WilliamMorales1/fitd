use crate::ast::{Attr, Name, Rel, Row, Stmt, StmtKind, Value};
use crate::diag::{Diag, Span};
use crate::graph::Cell;
use crate::lex::{Tok, Token, describe, lex};

/// Frame is one open `{` block: a node's children, a `with STYLE` block, or a block after an error.
enum Frame {
    Node(Name),
    Style(Name),
    Other,
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Token>,
    pos: usize,
    frames: Vec<(Frame, Span)>,
}

type PResult<T> = Result<T, Diag>;

/// KEYWORDS are the reserved words that start a statement or macro directive; no node may take one as its name.
pub const KEYWORDS: &[&str] = &[
    "title", "root", "layout", "page", "font", "theme", "style", "key", "group", "include",
    "require", "prefer", "with", "for", "if", "else", "def",
];

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.pos + k).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if t.tok != Tok::Eof {
            self.pos += 1;
        }
        t
    }

    fn tok_text(&self, k: usize) -> &str {
        self.toks[(self.pos + k).min(self.toks.len() - 1)].src(self.text)
    }

    fn unexpected(&self, want: &str) -> Diag {
        let got = describe(self.peek());
        Diag::error("E0002", format!("expected {want}, found {got}"))
            .primary(self.span(), format!("expected {want}"))
    }

    fn name(&mut self, what: &str) -> PResult<Name> {
        if *self.peek() != Tok::Ident {
            return Err(self.unexpected(what));
        }
        let t = self.bump();
        Ok(Name {
            text: t.src(self.text).to_string(),
            span: t.span,
        })
    }

    /// node_name reads a node name, which may not be a keyword.
    fn node_name(&mut self) -> PResult<Name> {
        let n = self.name("a node name")?;
        if KEYWORDS.contains(&n.text.as_str()) {
            return Err(Diag::error(
                "E0002",
                format!("`{}` is a keyword, so it cannot name a node", n.text),
            )
            .primary(n.span, "keyword")
            .help("rename the node, for example `{0}_` or `my_{0}`".replace("{0}", &n.text)));
        }
        Ok(n)
    }

    fn class_name(&mut self) -> PResult<Name> {
        let mut name = self.name("a style name")?;
        if *self.peek() == Tok::Dot {
            self.bump();
            let part = self.name("a style name after `.`")?;
            name.text.push('.');
            name.text.push_str(&part.text);
            name.span = name.span.to(part.span);
        }
        Ok(name)
    }

    fn keyword_is(&self, k: usize, kw: &str) -> bool {
        *self.peek_at(k) == Tok::Ident && self.tok_text(k) == kw
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::RBrace)
    }

    fn expect(&mut self, t: Tok) -> PResult<Span> {
        if *self.peek() != t {
            return Err(self.unexpected(describe(&t)));
        }
        Ok(self.bump().span)
    }

    fn string(&mut self) -> Option<(String, Span)> {
        match self.peek().clone() {
            Tok::Str(s) => Some((s, self.bump().span)),
            _ => None,
        }
    }

    fn value(&mut self) -> PResult<Value> {
        let t = self.bump();
        let v = match t.tok {
            Tok::Ident => Value::Name(Name {
                text: t.src(self.text).to_string(),
                span: t.span,
            }),
            Tok::Str(s) => Value::Str(s, t.span),
            Tok::Num => Value::Num(t.src(self.text).to_string(), t.span),
            Tok::Color => Value::Color(t.src(self.text).to_string(), t.span),
            _ => {
                self.pos -= 1;
                return Err(self.unexpected("a value"));
            }
        };
        if *self.peek() == Tok::DotDot {
            self.bump();
            let hi = self.value()?;
            return Ok(Value::Range(Box::new(v), Box::new(hi)));
        }
        Ok(v)
    }

    /// attrs reads an optional `[key=value, ...]` list.
    fn attrs(&mut self) -> PResult<Vec<Attr>> {
        let mut out = Vec::new();
        if *self.peek() != Tok::LBracket {
            return Ok(out);
        }
        self.bump();
        while *self.peek() != Tok::RBracket {
            let key = self.name("an attribute like `key=value`")?;
            self.expect(Tok::Eq)?;
            out.push(Attr {
                key,
                value: self.value()?,
            });
            if *self.peek() != Tok::Comma {
                break;
            }
            self.bump();
        }
        self.expect(Tok::RBracket)?;
        Ok(out)
    }

    fn targets(&mut self) -> PResult<Vec<Name>> {
        let mut out = vec![self.node_name()?];
        while matches!(self.peek(), Tok::Ident | Tok::Comma) {
            if *self.peek() == Tok::Comma {
                self.bump();
            }
            out.push(self.node_name()?);
        }
        Ok(out)
    }

    fn arrow(&self) -> Option<(bool, bool)> {
        match self.peek() {
            Tok::Arrow => Some((false, true)),
            Tok::DashDash => Some((false, false)),
            Tok::DotArrow => Some((true, true)),
            Tok::DotDot => Some((true, false)),
            _ => None,
        }
    }

    /// classes gives the styles of the open `with` blocks, innermost last.
    fn classes(&self) -> Vec<Name> {
        self.frames
            .iter()
            .filter_map(|(f, _)| match f {
                Frame::Style(c) => Some(c.clone()),
                _ => None,
            })
            .collect()
    }

    /// owner gives the node whose braces hold the current statement.
    fn owner(&self) -> Option<&Name> {
        self.frames.iter().rev().find_map(|(f, _)| match f {
            Frame::Node(n) => Some(n),
            _ => None,
        })
    }

    fn edge(&mut self) -> PResult<Vec<StmtKind>> {
        let mut froms = match self.owner() {
            Some(n) if self.arrow().is_some() => vec![Name {
                text: n.text.clone(),
                span: self.span(),
            }],
            None if self.arrow().is_some() => {
                return Err(Diag::error("E0002", "edge without a source")
                    .primary(self.span(), "no node encloses this line")
                    .help("write `a -> b`, or put `-> b` inside `a { }`"));
            }
            _ => vec![self.node_name()?],
        };
        while *self.peek() == Tok::Comma {
            self.bump();
            froms.push(self.node_name()?);
        }
        let mut hops = Vec::new();
        while let Some((link, directed)) = self.arrow() {
            self.bump();
            let to = self.targets()?;
            let label = self.string().map(|s| s.0);
            hops.push((froms, link, directed, to.clone(), label));
            froms = to;
        }
        if hops.is_empty() {
            return Err(self.unexpected("`->`, `--`, `..>`, or `..`"));
        }
        let (classes, attrs) = (self.classes(), self.attrs()?);
        let mut out = Vec::new();
        for (froms, link, directed, to, label) in hops {
            for from in froms {
                out.push(StmtKind::Edge {
                    from,
                    link,
                    directed: Some(directed),
                    to: to.clone(),
                    label: label.clone(),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                });
            }
        }
        Ok(out)
    }

    fn rel(&mut self) -> PResult<Rel> {
        if self.keyword_is(0, "align") && *self.peek_at(2) == Tok::LParen {
            self.bump();
            let side = self.name("an alignment like `top` or `row`")?;
            self.expect(Tok::LParen)?;
            let mut items = vec![self.node_name()?];
            while *self.peek() == Tok::Comma {
                self.bump();
                items.push(self.node_name()?);
            }
            self.expect(Tok::RParen)?;
            return Ok(Rel::Align { side, items });
        }
        let a = self.node_name()?;
        let op = self.name("a relation like `left-of`")?;
        if op.text == "at" {
            return Ok(Rel::At {
                a,
                place: self.name("a place like `top-left`")?,
            });
        }
        let mut nodes = vec![a, self.node_name()?];
        let mut ops = vec![op];
        while *self.peek() == Tok::Ident {
            ops.push(self.name("a relation like `left-of`")?);
            nodes.push(self.node_name()?);
        }
        Ok(Rel::Chain { nodes, ops })
    }

    /// edge_at reports whether an edge (`a, b -> c ...`) starts here.
    fn edge_at(&self) -> bool {
        let mut i = 0;
        while *self.peek_at(i) == Tok::Ident && *self.peek_at(i + 1) == Tok::Comma {
            i += 2;
        }
        *self.peek_at(i) == Tok::Ident
            && matches!(
                self.peek_at(i + 1),
                Tok::Arrow | Tok::DashDash | Tok::DotArrow | Tok::DotDot
            )
    }

    /// node reads `ID ["Title"] [attrs]`, preceded by the edge from the node whose braces hold it.
    fn node(&mut self) -> PResult<(Vec<StmtKind>, Option<Frame>)> {
        let id = self.node_name()?;
        let title = self.string();
        let attrs = self.attrs()?;
        let bare = title.is_none() && attrs.is_empty();
        let mut out: Vec<StmtKind> = self
            .owner()
            .map(|parent| StmtKind::Edge {
                from: Name {
                    text: parent.text.clone(),
                    span: id.span,
                },
                link: false,
                directed: None,
                to: vec![id.clone()],
                label: None,
                classes: Vec::new(),
                attrs: Vec::new(),
            })
            .into_iter()
            .collect();
        out.push(StmtKind::Node {
            id: id.clone(),
            title,
            classes: self.classes(),
            attrs,
            bare,
        });
        Ok((out, Some(Frame::Node(id))))
    }

    fn row(&mut self) -> PResult<Row> {
        if *self.peek() == Tok::LBracket {
            let open = self.bump().span;
            let mut cells = Vec::new();
            loop {
                match self.peek().clone() {
                    Tok::Cell(raw) => {
                        cells.push(cell(&raw));
                        self.bump();
                    }
                    Tok::Comma => {
                        self.bump();
                    }
                    Tok::RBracket => {
                        self.bump();
                        break;
                    }
                    _ => return Err(self.unexpected("a cell or `]`")),
                }
            }
            if cells.is_empty() {
                return Err(Diag::error("E0002", "figure row has no cells")
                    .primary(open, "expected cells like `[%100, ., _, GLU]`"));
            }
            return Ok(Row::Cells(cells));
        }
        if self.keyword_is(0, "image") {
            self.bump();
            let Some((path, _)) = self.string() else {
                return Err(self.unexpected("a quoted image path"));
            };
            return Ok(Row::Image {
                path,
                attrs: self.attrs()?,
            });
        }
        Err(self.unexpected("`[` cells or `image \"PATH\"`"))
    }

    /// stmt reads one statement and the block it may open with `{`.
    fn stmt(&mut self) -> PResult<(Vec<StmtKind>, Option<Frame>)> {
        let owner = self.owner().cloned();
        match self.peek().clone() {
            Tok::Body(text) => {
                self.bump();
                return Ok((vec![StmtKind::Body { text, owner }], None));
            }
            Tok::Tilde => {
                self.bump();
                let row = self.row()?;
                return Ok((vec![StmtKind::Row { row, owner }], None));
            }
            _ => {}
        }
        if self.edge_at() || self.arrow().is_some() {
            return Ok((self.edge()?, None));
        }
        if *self.peek() == Tok::Ident && !KEYWORDS.contains(&self.tok_text(0)) {
            return self.node();
        }
        let kw = self.name("a statement")?;
        let one = |k| Ok((vec![k], None));
        match kw.text.as_str() {
            "with" => {
                let name = self.class_name()?;
                Ok((vec![StmtKind::Open(name.clone())], Some(Frame::Style(name))))
            }
            "title" => match self.string() {
                Some((s, _)) => one(StmtKind::Title(s)),
                None => Err(self.unexpected("a quoted title")),
            },
            "root" => one(StmtKind::Root(self.node_name()?)),
            "layout" => one(StmtKind::Layout {
                kind: self.name("a layout like `radial`")?,
                attrs: self.attrs()?,
            }),
            "page" => {
                let size = self.value()?;
                let mut flags = Vec::new();
                while *self.peek() == Tok::Ident {
                    flags.push(self.name("a flag")?);
                }
                one(StmtKind::Page {
                    size,
                    flags,
                    attrs: self.attrs()?,
                })
            }
            "font" => one(StmtKind::Font {
                size: self.value()?,
                attrs: self.attrs()?,
            }),
            "theme" => {
                let preset = match self.peek() {
                    Tok::Ident => Some(self.name("a theme")?),
                    _ => None,
                };
                one(StmtKind::Theme {
                    preset,
                    attrs: self.attrs()?,
                })
            }
            "style" => one(StmtKind::Style {
                name: self.class_name()?,
                attrs: self.attrs()?,
            }),
            "key" => {
                let Some((label, _)) = self.string() else {
                    return Err(self.unexpected("a quoted key label"));
                };
                let (classes, attrs) = (self.classes(), self.attrs()?);
                one(StmtKind::Key {
                    label,
                    classes,
                    attrs,
                })
            }
            "group" => {
                let named = *self.peek() == Tok::Ident && matches!(self.peek_at(1), Tok::Str(_));
                let id = if named {
                    Some(self.name("a group name")?)
                } else {
                    None
                };
                let title = self.string().map(|s| s.0);
                let items = self.targets()?;
                let (classes, attrs) = (self.classes(), self.attrs()?);
                one(StmtKind::Group {
                    id,
                    title,
                    items,
                    classes,
                    attrs,
                })
            }
            "include" => match self.string() {
                Some((s, _)) => one(StmtKind::Include(s)),
                None => Err(self.unexpected("a quoted file name")),
            },
            "require" | "prefer" => one(StmtKind::Constraint {
                hard: kw.text == "require",
                rel: self.rel()?,
                attrs: self.attrs()?,
            }),
            _ => {
                let d = Diag::error("E0002", format!("unknown statement `{}`", kw.text))
                    .primary(kw.span, "not a statement");
                Err(d.help("statements: title, root, layout, page, font, theme, style, NODE, with, `|` body, `~` row, `a -> b`, group, key, require, prefer, include"))
            }
        }
    }

    /// recover skips to the end of the line, opening a block for a skipped `{` so its `}` still matches.
    fn recover(&mut self) {
        while !matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::RBrace) {
            if *self.peek() == Tok::LBrace {
                self.frames.push((Frame::Other, self.span()));
            }
            self.bump();
        }
    }
}

/// cell reads one figure cell: `%NN` shaded (`%100` solid), `.` outlined, `_` gap, a label, and a `*N` span.
pub fn cell(raw: &str) -> Cell {
    let mut s = raw.trim();
    let mut shade = 0.0;
    if let Some(r) = s.strip_prefix('%') {
        let n = r.bytes().take_while(u8::is_ascii_digit).count();
        if let Ok(v) = r[..n].parse::<f64>() {
            shade = (v / 100.0).clamp(0.0, 1.0);
            s = &r[n..];
        }
    }
    let mut cols = 1;
    let mut gap = false;
    let label = if let Some(r) = s.strip_prefix('"') {
        let (l, tail) = r.split_once('"').unwrap_or((r, ""));
        if let Some(n) = tail.strip_prefix('*').and_then(|n| n.parse().ok()) {
            cols = n;
        }
        Some(l.to_string())
    } else {
        let mut w = s;
        // `*N` is a span only when digits end the word, so a label like `a*t` survives.
        if let Some(i) = w.rfind('*') {
            let tail = &w[i + 1..];
            if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
                cols = tail.parse().unwrap_or(1);
                w = &w[..i];
            }
        }
        match w {
            "." | "" => None,
            "_" if shade == 0.0 => {
                gap = true;
                None
            }
            _ => Some(w.to_string()),
        }
    };
    Cell {
        shade,
        gap,
        span: cols.clamp(1, 64),
        label,
        image: None,
    }
}

pub fn parse(text: &str) -> (Vec<Stmt>, Vec<Diag>) {
    let (toks, errs) = lex(text);
    parse_tokens(text, toks, errs)
}

/// parse_tokens parses an already lexed (and possibly macro-expanded) token stream over `text`.
pub fn parse_tokens(text: &str, toks: Vec<Token>, mut errs: Vec<Diag>) -> (Vec<Stmt>, Vec<Diag>) {
    let mut p = Parser {
        text,
        toks,
        pos: 0,
        frames: Vec::new(),
    };
    let mut stmts = Vec::new();
    loop {
        match p.peek() {
            Tok::Newline => {
                p.bump();
                continue;
            }
            Tok::Eof => break,
            Tok::RBrace => {
                let at = p.bump().span;
                if p.frames.pop().is_none() {
                    errs.push(
                        Diag::error("E0002", "`}` closes no block").primary(at, "no `{` to close"),
                    );
                }
                continue;
            }
            _ => {}
        }
        let lo = p.span().lo;
        let start = p.pos;
        match p.stmt() {
            Ok((kinds, frame)) => {
                let hi = p.toks[p.pos - 1].span.hi;
                stmts.extend(kinds.into_iter().map(|kind| Stmt {
                    kind,
                    span: Span::new(lo.min(hi), hi.max(lo)),
                }));
                if *p.peek() == Tok::LBrace {
                    let at = p.bump().span;
                    match frame {
                        Some(f) => p.frames.push((f, at)),
                        None => {
                            errs.push(
                                Diag::error("E0002", "this statement takes no block")
                                    .primary(at, "unexpected `{`")
                                    .help("only nodes and `with STYLE` open a block"),
                            );
                            p.frames.push((Frame::Other, at));
                        }
                    }
                } else if !p.at_end() {
                    let head: String = (start..p.pos).map(|k| p.toks[k].src(p.text)).collect();
                    let head = head.as_str();
                    let bare = head
                        .chars()
                        .all(|c| c.is_alphanumeric() || "_-.".contains(c));
                    let d = if bare && *p.peek() == Tok::Ident {
                        // `pill a A`: a misspelt keyword or a template called without its module.
                        Diag::error("E0002", format!("unknown statement `{head}`"))
                            .primary(Span::new(lo, p.toks[p.pos - 1].span.hi.max(lo)), "not a statement or template")
                            .help("a node line is `ID \"Title\" [attrs]`; call a library template as `MOD.name`")
                    } else {
                        Diag::error(
                            "E0002",
                            format!("{} after the statement", describe(p.peek())),
                        )
                        .primary(p.span(), "expected end of line")
                    };
                    errs.push(d);
                    p.recover();
                }
            }
            Err(d) => {
                errs.push(d);
                p.recover();
            }
        }
    }
    for (_, at) in p.frames.drain(..).rev() {
        errs.push(Diag::error("E0002", "`{` is never closed").primary(at, "block opens here"));
    }
    (stmts, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relation_chains() {
        let (stmts, errs) = parse("require a left-of b left-of c [gap=2em]\n");
        assert!(errs.is_empty());
        assert!(
            matches!(&stmts[0].kind, StmtKind::Constraint { rel: Rel::Chain { nodes, ops }, attrs, .. }
            if nodes.len() == 3 && ops.len() == 2 && attrs.len() == 1)
        );
        assert_eq!(parse("require a left-of b left-of\n").1.len(), 1);
    }

    #[test]
    fn edge_sources_fan_out() {
        let (stmts, errs) = parse("a, b -> c, d \"l\"\n");
        assert!(errs.is_empty());
        let froms: Vec<_> = stmts
            .iter()
            .map(|s| match &s.kind {
                StmtKind::Edge {
                    from,
                    to,
                    label: Some(_),
                    ..
                } => (from.text.clone(), to.len()),
                _ => panic!("not an edge"),
            })
            .collect();
        assert_eq!(froms, [("a".into(), 2), ("b".into(), 2)]);
    }

    #[test]
    fn parses_every_statement() {
        let src = "title \"T\"\nlayout radial [root=a]\npage a1 portrait [margin=24pt]\nfont 7pt..48pt [wrap=24em, family=\"DejaVu Sans\"]\ntheme dark [background=#000, palette=\"#f00 #0f0\"]\n\nstyle hub [shape=circle]\na \"A\" [color=#123456] {\n  | body\n}\na -> b, c \"why\"\na -- b c\na .. b\nrequire b left-of c [gap=2em]\nprefer align top(b, c) [weight=3]\nrequire a at top-left\ngroup \"G\" b, c [color=#123]\nwith hub {\n  a -> c \"x\" [color=#f00]\n}\n";
        let (stmts, errs) = parse(src);
        assert!(
            errs.is_empty(),
            "{:?}",
            errs.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(stmts.len(), 17);
        assert!(
            matches!(&stmts[14].kind, StmtKind::Group { title: Some(_), items, attrs, .. } if items.len() == 2 && attrs.len() == 1)
        );
        assert!(
            matches!(&stmts[16].kind, StmtKind::Edge { classes, attrs, .. } if classes.len() == 1 && attrs.len() == 1)
        );
        assert!(
            matches!(&stmts[4].kind, StmtKind::Theme { preset: Some(_), attrs } if attrs.len() == 2)
        );
        assert!(
            matches!(&stmts[8].kind, StmtKind::Edge { to, label: Some(_), link: false, .. } if to.len() == 2)
        );
        assert!(matches!(
            &stmts[9].kind,
            StmtKind::Edge {
                link: false,
                directed: Some(false),
                ..
            }
        ));
        assert!(matches!(
            &stmts[10].kind,
            StmtKind::Edge {
                link: true,
                directed: Some(false),
                ..
            }
        ));
        assert!(matches!(
            &stmts[16].kind,
            StmtKind::Edge {
                directed: Some(true),
                ..
            }
        ));
        assert!(matches!(
            &stmts[12].kind,
            StmtKind::Constraint {
                hard: false,
                rel: Rel::Align { .. },
                ..
            }
        ));
    }

    #[test]
    fn parses_arrow_chains() {
        let (stmts, errs) =
            parse("style hub\nwith hub {\n  a -> b, c \"x\" -- d [color=#f00]\n}\n");
        assert!(errs.is_empty());
        assert_eq!(stmts.len(), 5);
        assert!(
            matches!(&stmts[2].kind, StmtKind::Edge { from, to, directed: Some(true), label: Some(_), classes, .. } if from.text == "a" && to.len() == 2 && classes.len() == 1)
        );
        assert!(
            matches!(&stmts[4].kind, StmtKind::Edge { from, directed: Some(false), label: None, attrs, .. } if from.text == "c" && attrs.len() == 1)
        );
        assert_eq!(parse("a -> b ->\n").1.len(), 1);
    }

    #[test]
    fn style_blocks_apply_to_lines() {
        let src = "style step\nstyle nn.block\nstyle radial\nwith step {\n  a \"A\" [color=#f00]\n  with nn.block {\n    e\n  }\n  a -> e\n}\nradial\nradial -> a\n";
        let (stmts, errs) = parse(src);
        assert!(errs.is_empty());
        let classes = |k: &StmtKind| match k {
            StmtKind::Node { classes, .. } | StmtKind::Edge { classes, .. } => {
                classes.iter().map(|c| c.text.clone()).collect::<Vec<_>>()
            }
            k => panic!("not a node or edge: {k:?}"),
        };
        let lines: Vec<_> = stmts
            .iter()
            .filter(|s| matches!(s.kind, StmtKind::Node { .. } | StmtKind::Edge { .. }))
            .map(|s| classes(&s.kind))
            .collect();
        assert_eq!(lines[0], vec!["step"]);
        assert_eq!(lines[1], vec!["step", "nn.block"]);
        assert_eq!(lines[2], vec!["step"]);
        assert!(
            lines[3].is_empty() && lines[4].is_empty(),
            "outside the block"
        );
        assert!(
            matches!(&stmts.last().unwrap().kind, StmtKind::Edge { from, .. } if from.text == "radial")
        );
    }

    #[test]
    fn indented_node_lines_nest() {
        let edges = |src: &str| {
            let (stmts, errs) = parse(src);
            assert!(
                errs.is_empty(),
                "{:?}",
                errs.iter().map(|d| &d.msg).collect::<Vec<_>>()
            );
            stmts
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::Edge {
                        from,
                        to,
                        directed,
                        label,
                        ..
                    } => Some(format!(
                        "{}{}{}{}",
                        from.text,
                        match directed {
                            Some(true) => ">",
                            Some(false) => "-",
                            None => "~",
                        },
                        to.iter()
                            .map(|t| t.text.as_str())
                            .collect::<Vec<_>>()
                            .join(","),
                        label.as_deref().unwrap_or("")
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let src = "style s\nr \"R\" {\n  | body\n  a \"A\"\n  with s {\n    b\n  }\n  c {\n    -> a \"L\"\n  }\n}\nr {\n  d\n}\n";
        assert_eq!(edges(src), ["r~a", "r~b", "r~c", "c>aL", "r~d"]);
        // The layout decides the nested kind in sema, wherever the `layout` line sits.
        let nd = |src: &str| crate::sema::nest_directed(&parse(src).0);
        assert!(nd("layout layered\n\na {\n  b\n}\n"));
        assert!(!nd("layout radial\n\na \"A\" {\n  b\n}\n"));
        assert!(!nd("a \"A\" {\n  b\n}\n"));
    }

    #[test]
    fn style_block_errors() {
        assert_eq!(parse("a\nb\na -> b : x\n").1.len(), 1, "`: NAME` is gone");
        let (stmts, errs) = parse("style s\nwith s {\n  b\n}\na\n");
        assert!(errs.is_empty());
        assert!(
            matches!(&stmts.last().unwrap().kind, StmtKind::Node { classes, .. } if classes.is_empty()),
            "a closing brace ends the block"
        );
        assert_eq!(parse("a\n}\n").1.len(), 1, "stray closing brace");
    }

    #[test]
    fn recovers_after_bad_line() {
        let (stmts, errs) = parse("a\nb = c\nc\nfrobnicate x\n");
        assert_eq!(stmts.len(), 4, "a, b, c, and frobnicate");
        assert_eq!(errs.len(), 2);
    }
}
