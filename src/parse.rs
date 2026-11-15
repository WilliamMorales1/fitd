use crate::ast::{Attr, Name, Rel, Stmt, StmtKind, Value};
use crate::diag::{Diag, Span};
use crate::lex::{Tok, Token, describe, lex};

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Token>,
    pos: usize,
    styles: std::collections::HashSet<String>,
    open: Vec<(Name, usize)>,
    /// scope holds the node lines that later, deeper lines nest under, with their columns.
    scope: Vec<(Name, usize)>,
    known: std::collections::HashSet<String>,
}

type PResult<T> = Result<T, Diag>;

/// KEYWORDS are the words that start a statement or macro directive, so no style may take one.
pub const KEYWORDS: &[&str] = &[
    "title", "root", "layout", "page", "font", "theme", "style", "node", "key", "group", "include",
    "require", "prefer", "for", "if", "else", "end", "def",
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

    fn text(&self, s: Span) -> &str {
        &self.text[s.lo..s.hi]
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
            text: self.text(t.span).to_string(),
            span: t.span,
        })
    }

    fn class_name(&mut self) -> PResult<Name> {
        let mut name = self.name("a class name")?;
        if *self.peek() == Tok::Dot {
            self.bump();
            let part = self.name("a class name after `.`")?;
            name.text.push('.');
            name.text.push_str(&part.text);
            name.span = name.span.to(part.span);
        }
        Ok(name)
    }

    fn keyword_is(&self, k: usize, kw: &str) -> bool {
        *self.peek_at(k) == Tok::Ident && self.text(self.toks[self.pos + k].span) == kw
    }

    fn at_end(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof)
    }

    fn expect(&mut self, t: Tok) -> PResult<Span> {
        if *self.peek() != t {
            return Err(self.unexpected(describe(&t)));
        }
        Ok(self.bump().span)
    }

    fn value(&mut self) -> PResult<Value> {
        let t = self.bump();
        let v = match t.tok {
            Tok::Ident => Value::Name(Name {
                text: self.text(t.span).to_string(),
                span: t.span,
            }),
            Tok::Str(s) => Value::Str(s, t.span),
            Tok::Num => Value::Num(self.text(t.span).to_string(), t.span),
            Tok::Color => Value::Color(self.text(t.span).to_string(), t.span),
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

    fn attrs(&mut self) -> PResult<Vec<Attr>> {
        let mut out = Vec::new();
        while !self.at_end() {
            let key = self.name("an attribute like `key=value`")?;
            self.expect(Tok::Eq)?;
            out.push(Attr {
                key,
                value: self.value()?,
            });
        }
        Ok(out)
    }

    fn targets(&mut self) -> PResult<Vec<Name>> {
        let mut out = vec![self.name("a node name")?];
        while (*self.peek() == Tok::Ident && *self.peek_at(1) != Tok::Eq)
            || *self.peek() == Tok::Comma
        {
            if *self.peek() == Tok::Comma {
                self.bump();
            }
            out.push(self.name("a node name")?);
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

    fn edge(&mut self) -> PResult<Vec<StmtKind>> {
        let mut froms = match self.scope.last() {
            Some((n, _)) if self.arrow().is_some() => vec![Name {
                text: n.text.clone(),
                span: self.span(),
            }],
            _ => vec![self.name("a node name")?],
        };
        while *self.peek() == Tok::Comma {
            self.bump();
            froms.push(self.name("a node name")?);
        }
        let mut hops = Vec::new();
        while self.arrow().is_some() {
            let (link, directed) = self.arrow().unwrap();
            self.bump();
            let to = self.targets()?;
            let label = match self.peek().clone() {
                Tok::Str(s) => {
                    self.bump();
                    Some(s)
                }
                _ => None,
            };
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

    /// classes gives the styles of the open blocks, innermost last.
    fn classes(&self) -> Vec<Name> {
        self.open
            .iter()
            .map(|(c, _)| c)
            .filter(|c| self.is_style(&c.text))
            .cloned()
            .collect()
    }

    /// is_style reports whether `name` is declared anywhere in the file.
    fn is_style(&self, name: &str) -> bool {
        self.styles.contains(name)
    }

    /// col gives the indent of the current token's line in characters.
    fn col(&self) -> usize {
        let lo = self.span().lo;
        lo - self.text[..lo].rfind('\n').map_or(0, |k| k + 1)
    }

    /// lone_name gives the token count of a line that holds only a name or `MOD.name`.
    fn lone_name(&self) -> Option<usize> {
        if *self.peek() != Tok::Ident || KEYWORDS.contains(&self.text(self.span())) {
            return None;
        }
        let k = if *self.peek_at(1) == Tok::Dot && *self.peek_at(2) == Tok::Ident {
            3
        } else {
            1
        };
        matches!(self.peek_at(k), Tok::Newline | Tok::Eof).then_some(k)
    }

    fn rel(&mut self) -> PResult<Rel> {
        if self.keyword_is(0, "align") && *self.peek_at(2) == Tok::LParen {
            self.bump();
            let side = self.name("an alignment like `top` or `row`")?;
            self.expect(Tok::LParen)?;
            let mut items = vec![self.name("a node name")?];
            while *self.peek() == Tok::Comma {
                self.bump();
                items.push(self.name("a node name")?);
            }
            self.expect(Tok::RParen)?;
            return Ok(Rel::Align { side, items });
        }
        let a = self.name("a node name")?;
        let op = self.name("a relation like `left-of`")?;
        if op.text == "at" {
            return Ok(Rel::At {
                a,
                place: self.name("a place like `top-left`")?,
            });
        }
        let mut nodes = vec![a, self.name("a node name")?];
        let mut ops = vec![op];
        while *self.peek() == Tok::Ident && *self.peek_at(1) != Tok::Eq {
            ops.push(self.name("a relation like `left-of`")?);
            nodes.push(self.name("a node name")?);
        }
        Ok(Rel::Chain { nodes, ops })
    }

    /// edge_at reports whether an edge (`a, b -> c ...`) starts at token offset i.
    fn edge_at(&self, mut i: usize) -> bool {
        while *self.peek_at(i) == Tok::Ident && *self.peek_at(i + 1) == Tok::Comma {
            i += 2;
        }
        *self.peek_at(i) == Tok::Ident
            && matches!(
                self.peek_at(i + 1),
                Tok::Arrow | Tok::DashDash | Tok::DotArrow | Tok::DotDot
            )
    }

    /// nest records a node line at `col` and gives the edge from the node it is indented under.
    fn nest(&mut self, id: &Name, col: usize) -> Option<StmtKind> {
        let edge = self.scope.last().map(|(parent, _)| StmtKind::Edge {
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
        });
        self.scope.push((id.clone(), col));
        edge
    }

    fn stmt(&mut self) -> PResult<Vec<StmtKind>> {
        if self.edge_at(0) || (self.arrow().is_some() && !self.scope.is_empty()) {
            return self.edge();
        }
        let col = self.col();
        if self.lone_name().is_some() {
            let at = self.pos;
            let name = self.class_name()?;
            if self.known.contains(&name.text) && !self.is_style(&name.text) {
                return Ok(self.nest(&name, col).into_iter().collect());
            }
            if self.is_style(&name.text) || (self.open.is_empty() && self.scope.is_empty()) {
                self.open.push((name.clone(), col));
                return Ok(vec![StmtKind::Open(name)]);
            }
            self.pos = at;
        }
        let bare = *self.peek() == Tok::Ident
            && !KEYWORDS.contains(&self.text(self.span()))
            && (matches!(self.peek_at(1), Tok::Str(_) | Tok::Newline | Tok::Eof)
                || (*self.peek_at(1) == Tok::Ident && *self.peek_at(2) == Tok::Eq));
        if bare || self.keyword_is(0, "node") {
            if !bare {
                self.bump();
            }
            let node = self.node()?;
            let StmtKind::Node { id, .. } = &node else {
                unreachable!()
            };
            self.known.insert(id.text.clone());
            let id = id.clone();
            return Ok(self.nest(&id, col).into_iter().chain([node]).collect());
        }
        self.stmt1().map(|k| vec![k])
    }

    fn node(&mut self) -> PResult<StmtKind> {
        let id = self.name("a node name")?;
        let title = match self.peek().clone() {
            Tok::Str(s) => Some((s, self.bump().span)),
            _ => None,
        };
        Ok(StmtKind::Node {
            id,
            title,
            classes: self.classes(),
            attrs: self.attrs()?,
        })
    }

    fn stmt1(&mut self) -> PResult<StmtKind> {
        match self.peek().clone() {
            Tok::Body(s) => {
                self.bump();
                return Ok(StmtKind::Body(s));
            }
            Tok::Cells(s) => {
                self.bump();
                return Ok(StmtKind::Cells(s));
            }
            _ => {}
        }
        let kw = self.name("a statement")?;
        Ok(match kw.text.as_str() {
            "title" => match self.peek().clone() {
                Tok::Str(s) => {
                    self.bump();
                    StmtKind::Title(s)
                }
                _ => return Err(self.unexpected("a quoted title")),
            },
            "root" => StmtKind::Root(self.name("a node name")?),
            "layout" => StmtKind::Layout {
                kind: self.name("a layout like `radial`")?,
                attrs: self.attrs()?,
            },
            "page" => {
                let size = self.value()?;
                let mut flags = Vec::new();
                while *self.peek() == Tok::Ident && *self.peek_at(1) != Tok::Eq {
                    flags.push(self.name("a flag")?);
                }
                StmtKind::Page {
                    size,
                    flags,
                    attrs: self.attrs()?,
                }
            }
            "font" => StmtKind::Font {
                size: self.value()?,
                attrs: self.attrs()?,
            },
            "theme" => {
                let preset = if *self.peek() == Tok::Ident && *self.peek_at(1) != Tok::Eq {
                    Some(self.name("a theme")?)
                } else {
                    None
                };
                StmtKind::Theme {
                    preset,
                    attrs: self.attrs()?,
                }
            }
            "style" => StmtKind::Style {
                name: self.class_name()?,
                attrs: self.attrs()?,
            },
            "key" => {
                let label = match self.peek().clone() {
                    Tok::Str(s) => {
                        self.bump();
                        s
                    }
                    _ => return Err(self.unexpected("a quoted key label")),
                };
                let (classes, attrs) = (self.classes(), self.attrs()?);
                StmtKind::Key {
                    label,
                    classes,
                    attrs,
                }
            }
            "group" => {
                let named = *self.peek() == Tok::Ident && matches!(self.peek_at(1), Tok::Str(_));
                let id = if named {
                    Some(self.name("a group name")?)
                } else {
                    None
                };
                let title = match self.peek().clone() {
                    Tok::Str(s) => {
                        self.bump();
                        Some(s)
                    }
                    _ => None,
                };
                let items = self.targets()?;
                let (classes, attrs) = (self.classes(), self.attrs()?);
                StmtKind::Group {
                    id,
                    title,
                    items,
                    classes,
                    attrs,
                }
            }
            "include" => match self.peek().clone() {
                Tok::Str(s) => {
                    self.bump();
                    StmtKind::Include(s)
                }
                _ => return Err(self.unexpected("a quoted file name")),
            },
            "require" | "prefer" => StmtKind::Constraint {
                hard: kw.text == "require",
                rel: self.rel()?,
                attrs: self.attrs()?,
            },
            _ => {
                let d = Diag::error("E0002", format!("unknown statement `{}`", kw.text))
                    .primary(kw.span, "not a statement");
                return Err(d.help("statements: title, root, layout, page, font, theme, style, node, `|` body, `a -> b`, `a ..> b`, group, key, require, prefer, include"));
            }
        })
    }
}

pub fn parse(text: &str) -> (Vec<Stmt>, Vec<Diag>) {
    let (toks, mut errs) = lex(text);
    let styles = (0..toks.len())
        .filter(|&i| i == 0 || toks[i - 1].tok == Tok::Newline)
        .filter(|&i| {
            toks[i].tok == Tok::Ident && &text[toks[i].span.lo..toks[i].span.hi] == "style"
        })
        .filter_map(|i| {
            toks.get(i + 1)
                .filter(|t| t.tok == Tok::Ident)
                .map(|t| (i + 1, t))
        })
        .map(|(k, t)| match (toks.get(k + 1), toks.get(k + 2)) {
            (Some(d), Some(p)) if d.tok == Tok::Dot && p.tok == Tok::Ident => {
                text[t.span.lo..p.span.hi].to_string()
            }
            _ => text[t.span.lo..t.span.hi].to_string(),
        })
        .collect();
    let mut p = Parser {
        text,
        toks,
        pos: 0,
        styles,
        open: Vec::new(),
        scope: Vec::new(),
        known: Default::default(),
    };
    let mut stmts = Vec::new();
    loop {
        while *p.peek() == Tok::Newline {
            p.bump();
        }
        if *p.peek() == Tok::Eof {
            break;
        }
        let lo = p.span().lo;
        // Off-side rule: a line indented no deeper than a block's opener closes that block.
        let col = p.col();
        if !matches!(p.peek(), Tok::Body(_) | Tok::Cells(_)) {
            while p.open.last().is_some_and(|&(_, c)| c >= col) {
                p.open.pop();
            }
            while p.scope.last().is_some_and(|&(_, c)| c >= col) {
                p.scope.pop();
            }
        }
        match p.stmt() {
            Ok(kinds) if p.at_end() => {
                let hi = p.toks[p.pos - 1].span.hi;
                stmts.extend(kinds.into_iter().map(|kind| Stmt {
                    kind,
                    span: Span::new(lo, hi),
                }));
            }
            Ok(_) => errs.push(
                Diag::error(
                    "E0002",
                    format!("unexpected {} after statement", describe(p.peek())),
                )
                .primary(p.span(), "expected end of line"),
            ),
            Err(d) => errs.push(d),
        }
        while !p.at_end() {
            p.bump();
        }
    }
    (stmts, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relation_chains() {
        let (stmts, errs) = parse("require a left-of b left-of c gap=2em\n");
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
        let src = "title \"T\"\nlayout radial root=a\npage a1 portrait margin=24pt\nfont 7pt..48pt wrap=24em family=\"DejaVu Sans\"\ntheme dark background=#000 palette=\"#f00 #0f0\"\nstyle hub shape=circle\nnode a \"A\" color=#123456\n| body\na -> b, c \"why\"\na -- b c\na .. b\nrequire b left-of c gap=2em\nprefer align top(b, c) weight=3\nrequire a at top-left\ngroup \"G\" b, c color=#123\nhub\n  a -> c \"x\" color=#f00\n";
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
        let (stmts, errs) = parse("style hub\nhub\n  a -> b, c \"x\" -- d color=#f00\n");
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
        let src = "style step\nstyle nn.block\nstyle radial\nstep\n  a \"A\" color=#f00\n  nn.block\n    e\n  a -> e\nnode radial\nradial -> a\n";
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
        let src = "style s\nr \"R\"\n  | body\n    a \"A\"\n    s\n      b\n  c\n    -> a \"L\"\nr\n  d\n";
        assert_eq!(edges(src), ["r~a", "r~b", "r~c", "c>aL", "r~d"]);
        // The layout decides the nested kind in sema, wherever the `layout` line sits.
        let nd = |src: &str| crate::sema::nest_directed(&parse(src).0);
        assert!(nd("node a\n  b\nlayout layered\n"));
        assert!(!nd("layout radial\na \"A\"\n  b\n"));
        assert!(!nd("a \"A\"\n  b\n"));
    }

    #[test]
    fn style_block_errors() {
        assert_eq!(
            parse("node a\nnode b\na -> b : x\n").1.len(),
            1,
            "`: NAME` is gone"
        );
        let (stmts, errs) = parse("style s\ns\nnode a\n");
        assert!(errs.is_empty());
        assert!(
            matches!(&stmts[2].kind, StmtKind::Node { classes, .. } if classes.is_empty()),
            "dedent closes"
        );
        assert_eq!(parse("node a\nend\n").1.len(), 1, "stray `end`");
    }

    #[test]
    fn recovers_after_bad_line() {
        let (stmts, errs) = parse("node a\nnode = b\nnode c\nfrobnicate x\n");
        assert_eq!(stmts.len(), 2);
        assert_eq!(errs.len(), 2);
    }
}
