use crate::ast::{Attr, Name, Rel, Stmt, StmtKind, Value};
use crate::diag::{Diag, Span};
use crate::lex::{Tok, Token, describe, lex};

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Token>,
    pos: usize,
}

type PResult<T> = Result<T, Diag>;

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
        let mut froms = vec![self.name("a node name")?];
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
        let (classes, attrs) = self.looks()?;
        let mut out = Vec::new();
        for (froms, link, directed, to, label) in hops {
            for from in froms {
                out.push(StmtKind::Edge {
                    from,
                    link,
                    directed,
                    to: to.clone(),
                    label: label.clone(),
                    classes: classes.clone(),
                    attrs: attrs.clone(),
                });
            }
        }
        Ok(out)
    }

    fn looks(&mut self) -> PResult<(Vec<Name>, Vec<Attr>)> {
        let (mut classes, mut attrs) = (Vec::new(), Vec::new());
        while !self.at_end() {
            if *self.peek() == Tok::Dot {
                self.bump();
                classes.push(self.name("a style name after `.`")?);
            } else {
                let key = self.name("`.style` or `key=value`")?;
                self.expect(Tok::Eq)?;
                attrs.push(Attr {
                    key,
                    value: self.value()?,
                });
            }
        }
        Ok((classes, attrs))
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

    fn stmt(&mut self) -> PResult<Vec<StmtKind>> {
        if *self.peek() == Tok::Ident
            && matches!(
                self.peek_at(1),
                Tok::Arrow | Tok::DashDash | Tok::DotArrow | Tok::DotDot
            )
        {
            return self.edge();
        }
        self.stmt1().map(|k| vec![k])
    }

    fn stmt1(&mut self) -> PResult<StmtKind> {
        if let Tok::Body(s) = self.peek().clone() {
            self.bump();
            return Ok(StmtKind::Body(s));
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
                name: self.name("a style name")?,
                attrs: self.attrs()?,
            },
            "node" => {
                let id = self.name("a node name")?;
                let title = match self.peek().clone() {
                    Tok::Str(s) => Some((s, self.bump().span)),
                    _ => None,
                };
                let (classes, attrs) = self.looks()?;
                StmtKind::Node {
                    id,
                    title,
                    classes,
                    attrs,
                }
            }
            "group" => {
                let title = match self.peek().clone() {
                    Tok::Str(s) => {
                        self.bump();
                        Some(s)
                    }
                    _ => None,
                };
                StmtKind::Group {
                    title,
                    items: self.targets()?,
                    attrs: self.attrs()?,
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
                return Err(d.help("statements: title, root, layout, page, font, theme, style, node, `|` body, `a -> b`, `a ..> b`, group, require, prefer, include"));
            }
        })
    }
}

pub fn parse(text: &str) -> (Vec<Stmt>, Vec<Diag>) {
    let (toks, mut errs) = lex(text);
    let mut p = Parser { text, toks, pos: 0 };
    let mut stmts = Vec::new();
    loop {
        while *p.peek() == Tok::Newline {
            p.bump();
        }
        if *p.peek() == Tok::Eof {
            break;
        }
        let lo = p.span().lo;
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
    fn parses_every_statement() {
        let src = "title \"T\"\nlayout radial root=a\npage a1 portrait margin=24pt\nfont 7pt..48pt wrap=24em family=\"DejaVu Sans\"\ntheme dark background=#000 palette=\"#f00 #0f0\"\nstyle hub shape=circle\nnode a \"A\" .hub color=#123456\n| body\na -> b, c \"why\"\na -- b c\na .. b\nrequire b left-of c gap=2em\nprefer align top(b, c) weight=3\nrequire a at top-left\ngroup \"G\" b, c color=#123\na -> c \"x\" .hub color=#f00\n";
        let (stmts, errs) = parse(src);
        assert!(
            errs.is_empty(),
            "{:?}",
            errs.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(stmts.len(), 16);
        assert!(
            matches!(&stmts[14].kind, StmtKind::Group { title: Some(_), items, attrs } if items.len() == 2 && attrs.len() == 1)
        );
        assert!(
            matches!(&stmts[15].kind, StmtKind::Edge { classes, attrs, .. } if classes.len() == 1 && attrs.len() == 1)
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
                directed: false,
                ..
            }
        ));
        assert!(matches!(
            &stmts[10].kind,
            StmtKind::Edge {
                link: true,
                directed: false,
                ..
            }
        ));
        assert!(matches!(
            &stmts[15].kind,
            StmtKind::Edge { directed: true, .. }
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
        let (stmts, errs) = parse("a -> b, c \"x\" -- d .hub color=#f00\n");
        assert!(errs.is_empty());
        assert_eq!(stmts.len(), 3);
        assert!(
            matches!(&stmts[0].kind, StmtKind::Edge { from, to, directed: true, label: Some(_), classes, .. } if from.text == "a" && to.len() == 2 && classes.len() == 1)
        );
        assert!(
            matches!(&stmts[2].kind, StmtKind::Edge { from, directed: false, label: None, attrs, .. } if from.text == "c" && attrs.len() == 1)
        );
        assert_eq!(parse("a -> b ->\n").1.len(), 1);
    }

    #[test]
    fn recovers_after_bad_line() {
        let (stmts, errs) = parse("node a\nnode = b\nnode c\nfrobnicate\n");
        assert_eq!(stmts.len(), 2);
        assert_eq!(errs.len(), 2);
    }
}
