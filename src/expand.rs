use crate::diag::{Diag, Source, Span};
use crate::lex::{Tok, Token, interp_end, lex};
use std::collections::HashMap;

pub const DEFAULT_BUDGET: u64 = 100_000;
const MAX_DEPTH: usize = 200;

/// Loaded is a source after includes and macro expansion: the spliced text, the expanded tokens over it, and diagnostics so far.
pub struct Loaded {
    pub src: Source,
    pub toks: Vec<Token>,
    pub diags: Vec<Diag>,
    pub defs: Vec<DefInfo>,
}

/// DefInfo describes one template for the editor: its full name, parameters, and where its `def` line sits.
#[derive(Clone, Debug)]
pub struct DefInfo {
    pub name: String,
    pub params: Vec<String>,
    pub span: Span,
}

pub fn load(path: &str, text: &str, budget: u64) -> Loaded {
    let (src, inc) = crate::include::expand(path, text);
    expand(src, inc, budget)
}

/// expand lexes `src` and runs its templates, loops, and conditions into a flat token stream.
pub fn expand(src: Source, mut diags: Vec<Diag>, budget: u64) -> Loaded {
    let (toks, lex_errs) = lex(&src.text);
    diags.extend(lex_errs);
    let mut b = Builder {
        toks: &toks,
        pos: 0,
        text: &src.text,
        src: &src,
        defs: HashMap::new(),
        errs: Vec::new(),
    };
    let items = b.block(None);
    let Builder { defs, errs, .. } = b;
    diags.extend(errs);
    let mut x = Expander {
        src: &src,
        defs: &defs,
        out: Vec::with_capacity(toks.len()),
        frames: vec![HashMap::new()],
        calls: Vec::new(),
        site: None,
        steps: 0,
        budget,
        stop: false,
        errs: Vec::new(),
    };
    let _ = x.run(&items);
    let end = Span::new(src.text.len(), src.text.len());
    let mut out = std::mem::take(&mut x.out);
    out.push(Token::new(Tok::Newline, end));
    out.push(Token::new(Tok::Eof, end));
    diags.extend(x.errs);
    let mut infos: Vec<DefInfo> = defs
        .iter()
        .map(|(k, d)| DefInfo {
            name: k.clone(),
            params: d.params.iter().map(|p| p.0.clone()).collect(),
            span: d.span,
        })
        .collect();
    infos.sort_by_key(|d| d.span.lo);
    Loaded {
        src,
        toks: out,
        diags,
        defs: infos,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(i64),
    Str(String),
    List(Vec<Val>),
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Val::Int(n) => write!(f, "{n}"),
            Val::Str(s) => f.write_str(s),
            Val::List(vs) => {
                for (k, v) in vs.iter().enumerate() {
                    if k > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{v}")?;
                }
                Ok(())
            }
        }
    }
}

impl Val {
    fn truthy(&self) -> bool {
        match self {
            Val::Int(n) => *n != 0,
            Val::Str(s) => !s.is_empty(),
            Val::List(vs) => !vs.is_empty(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Val::Int(_) => "a number",
            Val::Str(_) => "a string",
            Val::List(_) => "a list",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum T {
    Int(i64),
    Str(String),
    Id(String),
    Op(&'static str),
}

const OPS: [&str; 19] = [
    "..", "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "(", ")", "[", "]", ",", "=",
    ".",
];

fn tokens(s: &str) -> Result<Vec<T>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let lo = i;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            let w: String = cs[lo..i].iter().collect();
            out.push(T::Int(
                w.parse()
                    .map_err(|_| format!("number `{w}` is too large"))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let lo = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(T::Id(cs[lo..i].iter().collect()));
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') => break,
                    Some('\\') if i + 1 < cs.len() => {
                        s.push(cs[i + 1]);
                        i += 2;
                    }
                    Some(&o) => {
                        s.push(o);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push(T::Str(s));
        } else {
            for op in OPS {
                if op
                    .chars()
                    .enumerate()
                    .all(|(k, oc)| cs.get(i + k) == Some(&oc))
                {
                    out.push(T::Op(op));
                    i += op.len();
                    continue 'outer;
                }
            }
            return Err(format!("unexpected `{c}` in an expression"));
        }
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Expr {
    Lit(Val),
    Var(String),
    List(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct P {
    t: Vec<T>,
    i: usize,
}

fn op_str(o: &str) -> &'static str {
    OPS.iter().find(|x| **x == o).unwrap()
}

impl P {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }

    fn op(&mut self, o: &str) -> bool {
        if self.peek() == Some(&T::Op(op_str(o))) {
            self.i += 1;
            return true;
        }
        false
    }

    fn word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(T::Id(x)) if x == w) {
            self.i += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, o: &str) -> Result<(), String> {
        if self.op(o) {
            Ok(())
        } else {
            Err(match self.peek() {
                Some(t) => format!("expected `{o}`, found {}", show(t)),
                None => format!("expected `{o}`, found end of expression"),
            })
        }
    }

    fn done(&self) -> Result<(), String> {
        match self.peek() {
            None => Ok(()),
            Some(t) => Err(format!("unexpected {} after expression", show(t))),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let mut a = self.and()?;
        while self.word("or") {
            a = Expr::Bin("or", Box::new(a), Box::new(self.and()?));
        }
        Ok(a)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut a = self.compare()?;
        while self.word("and") {
            a = Expr::Bin("and", Box::new(a), Box::new(self.compare()?));
        }
        Ok(a)
    }

    fn compare(&mut self) -> Result<Expr, String> {
        let a = self.sum()?;
        for o in ["==", "!=", "<=", ">=", "<", ">"] {
            if self.op(o) {
                return Ok(Expr::Bin(op_str(o), Box::new(a), Box::new(self.sum()?)));
            }
        }
        Ok(a)
    }

    fn sum(&mut self) -> Result<Expr, String> {
        let mut a = self.product()?;
        loop {
            let op = if self.op("+") {
                "+"
            } else if self.op("-") {
                "-"
            } else {
                return Ok(a);
            };
            a = Expr::Bin(op, Box::new(a), Box::new(self.product()?));
        }
    }

    fn product(&mut self) -> Result<Expr, String> {
        let mut a = self.postfix()?;
        loop {
            let op = if self.op("*") {
                "*"
            } else if self.op("/") {
                "/"
            } else if self.op("%") {
                "%"
            } else {
                return Ok(a);
            };
            a = Expr::Bin(op, Box::new(a), Box::new(self.postfix()?));
        }
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut a = self.unary()?;
        while self.op("[") {
            let i = self.expr()?;
            self.expect("]")?;
            a = Expr::Index(Box::new(a), Box::new(i));
        }
        Ok(a)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(T::Int(n)) => {
                self.i += 1;
                Ok(Expr::Lit(Val::Int(n)))
            }
            Some(T::Str(s)) => {
                self.i += 1;
                Ok(Expr::Lit(Val::Str(s)))
            }
            Some(T::Op("-")) => {
                self.i += 1;
                let e = self.postfix()?;
                Ok(Expr::Bin(
                    "-",
                    Box::new(Expr::Lit(Val::Int(0))),
                    Box::new(e),
                ))
            }
            Some(T::Id(x)) if !matches!(x.as_str(), "and" | "or") => {
                self.i += 1;
                if self.op("(") {
                    return Ok(Expr::Call(x, self.list(")")?));
                }
                Ok(Expr::Var(x))
            }
            Some(T::Op("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(T::Op("[")) => {
                self.i += 1;
                Ok(Expr::List(self.list("]")?))
            }
            Some(t) => Err(format!("expected a value, found {}", show(&t))),
            None => Err("expected a value, found end of expression".into()),
        }
    }

    /// list reads comma-separated expressions through the closing `close`.
    fn list(&mut self, close: &str) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        loop {
            if self.op(close) {
                return Ok(out);
            }
            out.push(self.expr()?);
            if !self.op(",") {
                self.expect(close)?;
                return Ok(out);
            }
        }
    }
}

fn show(t: &T) -> String {
    match t {
        T::Int(n) => format!("`{n}`"),
        T::Str(s) => format!("\"{s}\""),
        T::Id(x) => format!("`{x}`"),
        T::Op(o) => format!("`{o}`"),
    }
}

fn parse_expr(s: &str) -> Result<Expr, String> {
    let mut p = P {
        t: tokens(s)?,
        i: 0,
    };
    if p.peek().is_none() {
        return Err("empty expression".into());
    }
    let e = p.expr()?;
    p.done()?;
    Ok(e)
}

#[derive(Clone, Debug)]
enum Iter {
    Range(Expr, Expr),
    List(Vec<Expr>),
}

const FOR_SHAPE: &str =
    "`for NAME in LO..HI`, `for NAME in LIST`, `for NAME in a, b, c`, or `for I, NAME in ...`";

/// parse_for reads the `NAMES in ITEMS` after `for`.
fn parse_for(s: &str) -> Result<(Vec<String>, Iter), String> {
    let mut p = P {
        t: tokens(s)?,
        i: 0,
    };
    let mut names = Vec::new();
    loop {
        match p.peek().cloned() {
            Some(T::Id(x)) if x != "in" => {
                p.i += 1;
                names.push(x);
            }
            _ => return Err(format!("expected a loop variable; write {FOR_SHAPE}")),
        }
        if !p.op(",") {
            break;
        }
    }
    if names.len() > 2 || !p.word("in") {
        return Err(format!("write {FOR_SHAPE}"));
    }
    let first = p.expr()?;
    let iter = if p.op("..") {
        Iter::Range(first, p.expr()?)
    } else {
        let mut vs = vec![first];
        while p.op(",") {
            vs.push(p.expr()?);
        }
        Iter::List(vs)
    };
    p.done()?;
    Ok((names, iter))
}

#[derive(Clone, Debug)]
enum Piece {
    Lit(String),
    Expr(Expr),
}

/// pieces splits text into literal runs and `${e}` holes; `$${` writes a literal `${`.
fn pieces(text: &str) -> Result<Vec<Piece>, String> {
    let cs: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < cs.len() {
        if cs[i] == '$' && cs.get(i + 1) == Some(&'$') && cs.get(i + 2) == Some(&'{') {
            lit += "${";
            i += 3;
            continue;
        }
        if cs[i] == '$' && cs.get(i + 1) == Some(&'{') {
            let end = interp_end(&cs, i).ok_or("`${` without a closing `}`")?;
            let body: String = cs[i + 2..end - 1].iter().collect();
            out.push(Piece::Lit(std::mem::take(&mut lit)));
            out.push(Piece::Expr(parse_expr(&body)?));
            i = end;
            continue;
        }
        lit.push(cs[i]);
        i += 1;
    }
    out.push(Piece::Lit(lit));
    Ok(out)
}

/// split_cells splits row text at commas outside quotes, braces, and `${}`.
fn split_cells(s: &str) -> Vec<String> {
    let cs: Vec<char> = s.chars().collect();
    let (mut out, mut cur) = (Vec::new(), String::new());
    let (mut quoted, mut depth, mut i) = (false, 0usize, 0);
    while i < cs.len() {
        let c = cs[i];
        if !quoted && let Some(end) = interp_end(&cs, i) {
            cur.extend(&cs[i..end]);
            i = end;
            continue;
        }
        match c {
            '"' => quoted = !quoted,
            '\\' if quoted => {
                cur.push(c);
                i += 1;
                if let Some(&n) = cs.get(i) {
                    cur.push(n);
                }
                i += 1;
                continue;
            }
            '{' if !quoted => depth += 1,
            '}' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                out.push(std::mem::take(&mut cur));
                i += 1;
                continue;
            }
            _ => {}
        }
        cur.push(c);
        i += 1;
    }
    out.push(cur);
    out.into_iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect()
}

/// Arg is one call argument as written: a token, or a `[...]` list of them.
#[derive(Clone, Debug)]
enum Arg {
    Tok(Token),
    List(Vec<Arg>),
}

#[derive(Clone, Debug)]
struct CallArg {
    key: Option<String>,
    val: Arg,
    span: Span,
}

#[derive(Clone, Debug)]
enum Item {
    Plain(Vec<Token>),
    For {
        names: Vec<String>,
        iter: Iter,
        body: Vec<Item>,
        span: Span,
    },
    If {
        arms: Vec<(Option<Expr>, Vec<Item>)>,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<CallArg>,
        children: Option<Vec<Item>>,
        span: Span,
    },
}

struct Def {
    params: Vec<(String, Option<Arg>)>,
    body: Vec<Item>,
    span: Span,
}

struct Builder<'a> {
    toks: &'a [Token],
    pos: usize,
    text: &'a str,
    src: &'a Source,
    defs: HashMap<String, Def>,
    errs: Vec<Diag>,
}

fn is_ident(w: &str) -> bool {
    w.chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && w.chars().all(|c| c.is_alphanumeric() || c == '_')
}

impl Builder<'_> {
    fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.pos + k).min(self.toks.len() - 1)].tok
    }

    fn peek(&self) -> &Tok {
        self.peek_at(0)
    }

    fn word(&self, k: usize) -> Option<&str> {
        let t = &self.toks[(self.pos + k).min(self.toks.len() - 1)];
        (t.tok == Tok::Ident).then(|| t.src(self.text))
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

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.errs
            .push(Diag::error("E0002", msg).primary(span, "not a valid macro line"));
    }

    /// skip_line drops the rest of a bad line, and a block it opens.
    fn skip_line(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof => return,
                Tok::Newline | Tok::RBrace if depth == 0 => return,
                Tok::LBrace => depth += 1,
                Tok::RBrace => depth -= 1,
                _ => {}
            }
            self.bump();
        }
    }

    fn expect_open(&mut self, what: &str) -> bool {
        if *self.peek() == Tok::LBrace {
            self.bump();
            return true;
        }
        let at = self.span();
        self.err(at, format!("expected `{{` to open the {what} block"));
        self.skip_line();
        false
    }

    /// block reads items through the `}` closing a macro block opened at `open`, or to the end of the file when `open` is none.
    fn block(&mut self, open: Option<Span>) -> Vec<Item> {
        let mut items = Vec::new();
        // depth counts plain `{` blocks, whose `}` belongs to the output.
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Newline => {
                    self.bump();
                    continue;
                }
                Tok::Eof => {
                    if let Some(at) = open {
                        self.errs.push(
                            Diag::error("E0002", "`{` is never closed")
                                .primary(at, "block opens here"),
                        );
                    }
                    return items;
                }
                Tok::RBrace => {
                    if depth == 0 && open.is_some() {
                        self.bump();
                        return items;
                    }
                    depth = depth.saturating_sub(1);
                    items.push(Item::Plain(vec![self.bump()]));
                    continue;
                }
                _ => {}
            }
            let span = self.span();
            match (self.word(0), self.peek_at(1)) {
                (Some("def"), _) => {
                    self.def(open.is_none());
                    continue;
                }
                (Some("for"), Tok::Expr(raw)) => {
                    let raw = raw.clone();
                    self.bump();
                    let at = self.bump().span;
                    if !self.expect_open("for") {
                        continue;
                    }
                    let body = self.block(Some(span));
                    match parse_for(&raw) {
                        Ok((names, iter)) => items.push(Item::For {
                            names,
                            iter,
                            body,
                            span: span.to(at),
                        }),
                        Err(m) => self.err(at, m),
                    }
                    continue;
                }
                (Some("if"), Tok::Expr(_)) => {
                    if let Some(it) = self.if_chain() {
                        items.push(it);
                    }
                    continue;
                }
                _ => {}
            }
            let call = self.word(0).is_some_and(is_ident)
                && (*self.peek_at(1) == Tok::LParen
                    || (*self.peek_at(1) == Tok::Dot
                        && self.word(2).is_some_and(is_ident)
                        && *self.peek_at(3) == Tok::LParen));
            if call {
                if let Some(it) = self.call() {
                    items.push(it);
                }
                continue;
            }
            let mut toks = Vec::new();
            loop {
                match self.peek() {
                    Tok::Newline | Tok::Eof | Tok::RBrace => break,
                    Tok::LBrace => {
                        toks.push(self.bump());
                        depth += 1;
                        break;
                    }
                    _ => toks.push(self.bump()),
                }
            }
            items.push(Item::Plain(toks));
        }
    }

    fn if_chain(&mut self) -> Option<Item> {
        let span = self.span();
        let mut arms = Vec::new();
        let mut ok = true;
        loop {
            // At `if EXPR {` or, after `else`, at `{`.
            let cond = if *self.peek() == Tok::LBrace {
                None
            } else {
                self.bump();
                let Tok::Expr(raw) = self.peek().clone() else {
                    let at = self.span();
                    self.err(at, "expected a condition after `if`");
                    self.skip_line();
                    return None;
                };
                let at = self.bump().span;
                match parse_expr(&raw) {
                    Ok(e) => Some(e),
                    Err(m) => {
                        self.err(at, m);
                        ok = false;
                        None
                    }
                }
            };
            let last = cond.is_none();
            if !self.expect_open("if") {
                return None;
            }
            let body = self.block(Some(span));
            arms.push((cond, body));
            let save = self.pos;
            while *self.peek() == Tok::Newline {
                self.bump();
            }
            if self.word(0) != Some("else") {
                self.pos = save;
                break;
            }
            let at = self.bump().span;
            if last {
                self.err(at, "`else` after `else`");
            }
            if self.word(0) != Some("if") && *self.peek() != Tok::LBrace {
                let at = self.span();
                self.err(at, "expected `if` or `{` after `else`");
                self.skip_line();
                break;
            }
        }
        ok.then_some(Item::If { arms, span })
    }

    fn def(&mut self, top: bool) {
        let span = self.bump().span;
        let Some(name) = self.word(0).filter(|w| is_ident(w)).map(String::from) else {
            let at = self.span();
            self.err(at, "expected a template name after `def`");
            self.skip_line();
            return;
        };
        let name_span = self.bump().span;
        let mut params: Vec<(String, Option<Arg>)> = Vec::new();
        if *self.peek() != Tok::LParen {
            let at = self.span();
            self.err(
                at,
                format!("expected `(` after `def {name}`; write `def {name}(a, b) {{`"),
            );
            self.skip_line();
            return;
        }
        self.bump();
        loop {
            match self.peek() {
                Tok::RParen => {
                    self.bump();
                    break;
                }
                Tok::Ident => {
                    let p = self.bump().src(self.text).to_string();
                    let default = if *self.peek() == Tok::Eq {
                        self.bump();
                        match self.arg() {
                            Some(a) => Some(a),
                            None => return,
                        }
                    } else {
                        None
                    };
                    if params.iter().any(|q| q.0 == p) {
                        self.err(name_span, format!("parameter `{p}` appears twice"));
                    }
                    params.push((p, default));
                    if *self.peek() == Tok::Comma {
                        self.bump();
                    }
                }
                _ => {
                    let at = self.span();
                    self.err(at, "expected a parameter name or `)`");
                    self.skip_line();
                    return;
                }
            }
        }
        if !self.expect_open("def") {
            return;
        }
        let body = self.block(Some(span));
        if !top {
            self.err(span, "`def` is allowed only at the top level");
            return;
        }
        let line = self.src.line_col(span.lo).0;
        let key = match self.src.module(line) {
            Some(m) => format!("{m}.{name}"),
            None => name,
        };
        if let Some(old) = self.defs.get(&key) {
            let (path, l) = self.src.file_line(self.src.line_col(old.span.lo).0);
            let d = Diag::error("E0012", format!("template `{key}` is defined twice"))
                .primary(name_span, "defined again here")
                .note(format!("first defined at {path}:{l}"));
            self.errs.push(d);
            return;
        }
        self.defs.insert(
            key,
            Def {
                params,
                body,
                span: span.to(name_span),
            },
        );
    }

    /// arg reads one argument value: a word, number, string, color, or `[...]` list.
    fn arg(&mut self) -> Option<Arg> {
        match self.peek() {
            Tok::Ident | Tok::Num | Tok::Str(_) | Tok::Color => Some(Arg::Tok(self.bump())),
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    if *self.peek() == Tok::RBracket {
                        self.bump();
                        return Some(Arg::List(items));
                    }
                    items.push(self.arg()?);
                    match self.peek() {
                        Tok::Comma => {
                            self.bump();
                        }
                        Tok::RBracket => {}
                        _ => {
                            let at = self.span();
                            self.err(at, "expected `,` or `]` in a list");
                            self.skip_line();
                            return None;
                        }
                    }
                }
            }
            _ => {
                let at = self.span();
                self.err(
                    at,
                    "expected an argument: a word, number, string, or `[...]` list",
                );
                self.skip_line();
                None
            }
        }
    }

    fn call(&mut self) -> Option<Item> {
        let first = self.bump();
        let mut name = first.src(self.text).to_string();
        let mut span = first.span;
        if *self.peek() == Tok::Dot {
            self.bump();
            let t = self.bump();
            name = format!("{name}.{}", t.src(self.text));
            span = span.to(t.span);
        }
        self.bump();
        let mut args = Vec::new();
        loop {
            if *self.peek() == Tok::RParen {
                span = span.to(self.bump().span);
                break;
            }
            let lo = self.span();
            let key = if *self.peek() == Tok::Ident && *self.peek_at(1) == Tok::Eq {
                let k = self.bump().src(self.text).to_string();
                self.bump();
                Some(k)
            } else {
                None
            };
            let val = self.arg()?;
            let hi = self.toks[self.pos - 1].span;
            args.push(CallArg {
                key,
                val,
                span: lo.to(hi),
            });
            match self.peek() {
                Tok::Comma => {
                    self.bump();
                }
                Tok::RParen => {}
                _ => {
                    let at = self.span();
                    self.err(at, format!("expected `,` or `)` in the call to `{name}`"));
                    self.skip_line();
                    return None;
                }
            }
        }
        let children = if *self.peek() == Tok::LBrace {
            let at = self.bump().span;
            Some(self.block(Some(at)))
        } else {
            None
        };
        if !matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::RBrace) {
            let at = self.span();
            self.err(
                at,
                format!(
                    "unexpected {} after the call",
                    crate::lex::describe(self.peek())
                ),
            );
            self.skip_line();
        }
        Some(Item::Call {
            name,
            args,
            children,
            span,
        })
    }
}

type Frame = HashMap<String, (Val, Option<Span>)>;

struct Expander<'a> {
    src: &'a Source,
    defs: &'a HashMap<String, Def>,
    out: Vec<Token>,
    frames: Vec<Frame>,
    calls: Vec<(String, Span)>,
    /// site is the outermost template call being expanded; tokens from template bodies take its span.
    site: Option<Span>,
    steps: u64,
    budget: u64,
    stop: bool,
    errs: Vec<Diag>,
}

type Run<T> = Result<T, ()>;

impl Expander<'_> {
    fn text(&self) -> &str {
        &self.src.text
    }

    fn fail(&mut self, span: Span, msg: impl Into<String>) {
        let mut d = Diag::error("E0012", msg).primary(span, "while expanding this line");
        for (name, at) in self.calls.iter().rev().take(4) {
            let (path, l) = self.src.file_line(self.src.line_col(at.lo).0);
            d = d.note(format!("in `{name}` at {path}:{l}"));
        }
        self.errs.push(d);
    }

    fn tick(&mut self, span: Span) -> Run<()> {
        self.steps += 1;
        if self.steps > self.budget {
            if !self.stop {
                self.stop = true;
                let d = Diag::error(
                    "E0013",
                    format!("macro expansion ran past {} steps", self.budget),
                )
                .primary(span, "expansion stopped here");
                self.errs.push(d.help(
                    "check for runaway recursion or a huge `for` range, or raise `--expand-budget`",
                ));
            }
            return Err(());
        }
        Ok(())
    }

    fn at(&self, span: Span) -> Span {
        self.site.unwrap_or(span)
    }

    /// emit copies a source token to the output, re-homed to the call site inside a template.
    fn emit(&mut self, t: &Token) {
        let mut t = t.clone();
        if let Some(s) = self.site {
            if t.text.is_none() && matches!(t.tok, Tok::Ident | Tok::Num | Tok::Color) {
                t.text = Some(t.src(&self.src.text).to_string());
            }
            t.span = s;
        }
        self.out.push(t);
    }

    fn newline(&mut self) {
        let at = self
            .out
            .last()
            .map_or(Span::default(), |t| Span::new(t.span.hi, t.span.hi));
        self.out.push(Token::new(Tok::Newline, at));
    }

    fn run(&mut self, items: &[Item]) -> Run<()> {
        for item in items {
            if self.item(item).is_err() && self.stop {
                return Err(());
            }
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&(Val, Option<Span>)> {
        self.frames
            .last()
            .unwrap()
            .get(name)
            .or_else(|| self.frames[0].get(name))
    }

    fn item(&mut self, item: &Item) -> Run<()> {
        match item {
            Item::Plain(toks) => {
                self.tick(toks.first().map_or(Span::default(), |t| t.span))?;
                self.plain(toks)
            }
            Item::For {
                names,
                iter,
                body,
                span,
            } => {
                self.tick(*span)?;
                let vals = self.iterate(*span, iter)?;
                for (k, v) in vals.into_iter().enumerate() {
                    self.tick(*span)?;
                    self.bind(names, k, v);
                    self.run(body)?;
                }
                Ok(())
            }
            Item::If { arms, span } => {
                self.tick(*span)?;
                for (cond, body) in arms {
                    let go = match cond {
                        Some(c) => self.eval(*span, c)?.truthy(),
                        None => true,
                    };
                    if go {
                        return self.run(body);
                    }
                }
                Ok(())
            }
            Item::Call {
                name,
                args,
                children,
                span,
            } => {
                self.tick(*span)?;
                self.call(name, args, children.as_deref(), *span)
            }
        }
    }

    fn bind(&mut self, names: &[String], k: usize, v: Val) {
        let frame = self.frames.last_mut().unwrap();
        if let [i, _] = names {
            frame.insert(i.clone(), (Val::Int(k as i64 + 1), None));
        }
        frame.insert(names.last().unwrap().clone(), (v, None));
    }

    fn iterate(&mut self, span: Span, iter: &Iter) -> Run<Vec<Val>> {
        match iter {
            Iter::Range(lo, hi) => match (self.eval(span, lo)?, self.eval(span, hi)?) {
                (Val::Int(a), Val::Int(b)) => {
                    // A huge range runs into the step budget, not into memory.
                    let n = (b - a + 1).clamp(0, self.budget as i64 + 1);
                    Ok((0..n).map(|k| Val::Int(a + k)).collect())
                }
                (a, b) => {
                    self.fail(
                        span,
                        format!("a range needs numbers, found {} and {}", a.kind(), b.kind()),
                    );
                    Err(())
                }
            },
            Iter::List(es) => {
                let mut vs = Vec::new();
                for e in es {
                    vs.push(self.eval(span, e)?);
                }
                match vs.as_slice() {
                    [Val::List(items)] => Ok(items.clone()),
                    _ => Ok(vs),
                }
            }
        }
    }

    fn interp(&mut self, span: Span, s: &str) -> Run<String> {
        if !s.contains("${") {
            return Ok(s.to_string());
        }
        let ps = match pieces(s) {
            Ok(p) => p,
            Err(m) => {
                self.fail(span, m);
                return Err(());
            }
        };
        let mut out = String::new();
        for p in &ps {
            match p {
                Piece::Lit(l) => out += l,
                Piece::Expr(e) => out += &self.eval(span, e)?.to_string(),
            }
        }
        Ok(out)
    }

    /// whole gives the expression of a word that is exactly one `${e}`.
    fn whole(s: &str) -> Option<Expr> {
        let cs: Vec<char> = s.chars().collect();
        if interp_end(&cs, 0)? != cs.len() {
            return None;
        }
        parse_expr(&s[2..s.len() - 1]).ok()
    }

    fn plain(&mut self, toks: &[Token]) -> Run<()> {
        let row = matches!(
            (toks.first().map(|t| &t.tok), toks.get(1).map(|t| &t.tok)),
            (Some(Tok::Tilde), Some(Tok::LBracket))
        );
        let mut in_row = false;
        let mut ok = true;
        let start = self.out.len();
        for t in toks {
            let r = match &t.tok {
                Tok::LBracket if row && !in_row => {
                    in_row = true;
                    self.emit(t);
                    Ok(())
                }
                Tok::RBracket if in_row => {
                    in_row = false;
                    self.emit(t);
                    Ok(())
                }
                Tok::Comma if in_row => Ok(()),
                Tok::Cell(raw) => self.cell(t, raw),
                Tok::Ident if t.src(self.text()).contains("${") => self.ident(t),
                Tok::Str(s) | Tok::Body(s) if s.contains("${") => {
                    let s = s.clone();
                    self.interp(t.span, &s).map(|v| {
                        let tok = match t.tok {
                            Tok::Str(_) => Tok::Str(v),
                            _ => Tok::Body(v),
                        };
                        let span = self.at(t.span);
                        self.out.push(Token::new(tok, span));
                    })
                }
                _ => {
                    self.emit(t);
                    Ok(())
                }
            };
            ok &= r.is_ok();
        }
        if !ok {
            self.out.truncate(start);
            return Err(());
        }
        self.newline();
        Ok(())
    }

    fn ident(&mut self, t: &Token) -> Run<()> {
        let w = t.src(self.text()).to_string();
        let mut span = self.at(t.span);
        let s = match Self::whole(&w) {
            Some(e) => {
                if let Expr::Var(x) = &e
                    && let Some((_, Some(s))) = self.get(x)
                {
                    span = *s;
                }
                // A list stands in as `a, b, c`, so `group T ${ids}` takes every member.
                self.eval(t.span, &e)?.to_string()
            }
            None => self.interp(t.span, &w)?,
        };
        if s.contains('\n') {
            self.fail(t.span, "an interpolated value holds a line break");
            return Err(());
        }
        let color = s.starts_with('#')
            && s.len() > 1
            && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
            && self.out.last().is_some_and(|t| t.tok == Tok::Eq);
        if color {
            self.out.push(Token {
                tok: Tok::Color,
                span,
                text: Some(s),
            });
            return Ok(());
        }
        let (toks, errs) = lex(&s);
        if let Some(e) = errs.first() {
            self.fail(t.span, format!("`{s}` does not fit here: {}", e.msg));
            return Err(());
        }
        for k in toks {
            if matches!(k.tok, Tok::Newline | Tok::Eof) {
                continue;
            }
            let text = k.src(&s).to_string();
            self.out.push(Token {
                tok: k.tok,
                span,
                text: Some(text),
            });
        }
        Ok(())
    }

    /// cell expands one row cell, which may hold `${e}` values or be a `for ... { CELLS }` loop.
    fn cell(&mut self, t: &Token, raw: &str) -> Run<()> {
        let r = raw.trim();
        let cells = if let Some(rest) = r.strip_prefix("for ")
            && r.ends_with('}')
            && let Some(open) = rest.find('{')
        {
            let (names, iter) = match parse_for(&rest[..open]) {
                Ok(x) => x,
                Err(m) => {
                    self.fail(t.span, m);
                    return Err(());
                }
            };
            let body = &rest[open + 1..rest.len() - 1];
            let mut out = Vec::new();
            for (k, v) in self.iterate(t.span, &iter)?.into_iter().enumerate() {
                self.tick(t.span)?;
                self.bind(&names, k, v);
                out.extend(split_cells(&self.interp(t.span, body)?));
            }
            out
        } else if r.contains("${") {
            split_cells(&self.interp(t.span, r)?)
        } else {
            vec![raw.to_string()]
        };
        let span = self.at(t.span);
        for c in cells {
            if self
                .out
                .last()
                .is_some_and(|t| matches!(t.tok, Tok::Cell(_)))
            {
                self.out.push(Token::new(Tok::Comma, span));
            }
            self.out.push(Token::new(Tok::Cell(c), span));
        }
        Ok(())
    }

    fn arg(&mut self, a: &Arg) -> Run<Val> {
        match a {
            Arg::List(items) => {
                let mut vs = Vec::new();
                for i in items {
                    vs.push(self.arg(i)?);
                }
                Ok(Val::List(vs))
            }
            Arg::Tok(t) => {
                let w = t.src(self.text()).to_string();
                Ok(match &t.tok {
                    Tok::Ident => match Self::whole(&w) {
                        Some(e) => self.eval(t.span, &e)?,
                        None => Val::Str(self.interp(t.span, &w)?),
                    },
                    Tok::Num if w.bytes().all(|b| b.is_ascii_digit()) => match w.parse() {
                        Ok(n) => Val::Int(n),
                        Err(_) => Val::Str(w),
                    },
                    Tok::Str(s) => {
                        let s = s.clone();
                        Val::Str(self.interp(t.span, &s)?)
                    }
                    _ => Val::Str(w),
                })
            }
        }
    }

    fn call(
        &mut self,
        name: &str,
        args: &[CallArg],
        children: Option<&[Item]>,
        span: Span,
    ) -> Run<()> {
        let defs = self.defs;
        let line = self.src.line_col(span.lo).0;
        let local = self
            .src
            .module(line)
            .filter(|_| !name.contains('.'))
            .and_then(|m| defs.get(&format!("{m}.{name}")));
        let Some(def) = local.or_else(|| defs.get(name)) else {
            let mut known: Vec<&String> = defs.keys().collect();
            known.sort();
            let hint = if known.is_empty() {
                String::new()
            } else {
                let ks: Vec<&str> = known.iter().map(|k| k.as_str()).collect();
                format!(" (templates: {})", ks.join(", "))
            };
            let d = if name.contains('.') {
                format!("unknown template `{name}`{hint}")
            } else {
                format!(
                    "unknown template `{name}`{hint}; a library template is called as `MOD.{name}(...)`"
                )
            };
            self.fail(span, d);
            return Err(());
        };
        let params: Vec<&str> = def.params.iter().map(|p| p.0.as_str()).collect();
        let mut vals: Vec<Option<(Val, Option<Span>)>> = vec![None; params.len()];
        let mut next = 0;
        for a in args {
            let k = match &a.key {
                Some(key) => match params.iter().position(|p| p == key) {
                    Some(k) => k,
                    None => {
                        self.fail(
                            a.span,
                            format!(
                                "`{name}` has no parameter `{key}` (it takes {})",
                                params.join(", ")
                            ),
                        );
                        return Err(());
                    }
                },
                None => {
                    while next < params.len() && vals[next].is_some() {
                        next += 1;
                    }
                    if next == params.len() {
                        let n = args.iter().filter(|a| a.key.is_none()).count();
                        self.fail(
                            span,
                            format!(
                                "`{name}` takes {} argument{} ({}), found {n}",
                                params.len(),
                                if params.len() == 1 { "" } else { "s" },
                                params.join(", "),
                            ),
                        );
                        return Err(());
                    }
                    next
                }
            };
            if vals[k].is_some() {
                self.fail(a.span, format!("`{}` is given twice", params[k]));
                return Err(());
            }
            let v = self.arg(&a.val)?;
            vals[k] = Some((v, Some(self.at(a.span))));
        }
        let mut frame = Frame::new();
        for (k, v) in vals.into_iter().enumerate() {
            let v = match (v, &def.params[k].1) {
                (Some(v), _) => v,
                (None, Some(d)) => (self.arg(d)?, None),
                (None, None) => {
                    self.fail(
                        span,
                        format!(
                            "`{name}` needs `{}` (it takes {})",
                            params[k],
                            params.join(", ")
                        ),
                    );
                    return Err(());
                }
            };
            frame.insert(params[k].to_string(), v);
        }
        if self.calls.len() >= MAX_DEPTH {
            self.fail(span, format!("templates nest more than {MAX_DEPTH} deep"));
            self.stop = true;
            return Err(());
        }
        let from = self.out.len();
        let site = self.site;
        self.site = Some(self.at(span));
        self.frames.push(frame);
        self.calls.push((name.to_string(), span));
        let r = self.run(&def.body);
        self.calls.pop();
        self.frames.pop();
        self.site = site;
        r?;
        if let Some(kids) = children {
            self.children(from, span, kids)?;
        }
        Ok(())
    }

    /// children puts a call's `{ ... }` statements in the block of the last node its expansion drew, opening one if needed.
    fn children(&mut self, from: usize, span: Span, kids: &[Item]) -> Run<()> {
        let mut closers = Vec::new();
        while self.out.len() > from
            && matches!(self.out.last().unwrap().tok, Tok::RBrace | Tok::Newline)
        {
            closers.push(self.out.pop().unwrap());
        }
        closers.reverse();
        let at = self.at(span);
        let k = self.out[from..]
            .iter()
            .rposition(|t| t.tok == Tok::Newline)
            .map_or(from, |p| from + p + 1);
        let last = &self.out[k..];
        let open = last.is_empty()
            || last.last().is_some_and(|t| t.tok == Tok::LBrace)
            || matches!(last[0].tok, Tok::Body(_) | Tok::Tilde);
        if open {
            if !last.is_empty() {
                self.newline();
            }
            let r = self.run(kids);
            self.out.extend(closers);
            return r;
        }
        self.out.push(Token::new(Tok::LBrace, at));
        self.newline();
        let r = self.run(kids);
        self.out.push(Token::new(Tok::RBrace, at));
        self.newline();
        self.out.extend(closers);
        r
    }

    fn eval(&mut self, span: Span, e: &Expr) -> Run<Val> {
        let int = |me: &mut Self, v: Val, what: &str| match v {
            Val::Int(n) => Ok(n),
            o => {
                me.fail(
                    span,
                    format!("{what} needs a number, found {} \"{o}\"", o.kind()),
                );
                Err(())
            }
        };
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Var(x) => match self.get(x) {
                Some(v) => v.0.clone(),
                None => {
                    let mut known: Vec<&String> = self
                        .frames
                        .last()
                        .unwrap()
                        .keys()
                        .chain(self.frames[0].keys())
                        .collect();
                    known.sort();
                    known.dedup();
                    let d = if known.is_empty() {
                        String::new()
                    } else {
                        let ks: Vec<&str> = known.iter().map(|k| k.as_str()).collect();
                        format!(" (known: {})", ks.join(", "))
                    };
                    self.fail(span, format!("unknown variable `{x}`{d}"));
                    return Err(());
                }
            },
            Expr::List(es) => {
                let mut vs = Vec::new();
                for e in es {
                    vs.push(self.eval(span, e)?);
                }
                Val::List(vs)
            }
            Expr::Index(a, i) => {
                let (a, i) = (self.eval(span, a)?, self.eval(span, i)?);
                let i = int(self, i, "an index")?;
                let n = match &a {
                    Val::List(vs) => vs.len(),
                    Val::Str(s) => s.chars().count(),
                    Val::Int(_) => {
                        self.fail(span, "a number cannot be indexed");
                        return Err(());
                    }
                };
                let k = usize::try_from(i).ok().filter(|&k| (1..=n).contains(&k));
                match (k, a) {
                    (Some(k), Val::List(vs)) => vs[k - 1].clone(),
                    (Some(k), Val::Str(s)) => Val::Str(s.chars().nth(k - 1).unwrap().to_string()),
                    _ => {
                        self.fail(span, format!("index {i} is outside 1..{n}"));
                        return Err(());
                    }
                }
            }
            Expr::Bin("and", a, b) => {
                let x = self.eval(span, a)?;
                if x.truthy() { self.eval(span, b)? } else { x }
            }
            Expr::Bin("or", a, b) => {
                let x = self.eval(span, a)?;
                if x.truthy() { x } else { self.eval(span, b)? }
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(span, a)?, self.eval(span, b)?);
                match (*op, x, y) {
                    ("==", x, y) => Val::Int((x == y) as i64),
                    ("!=", x, y) => Val::Int((x != y) as i64),
                    ("+", Val::List(mut x), Val::List(y)) => {
                        x.extend(y);
                        Val::List(x)
                    }
                    ("+", x @ Val::Str(_), y) | ("+", x, y @ Val::Str(_)) => {
                        Val::Str(format!("{x}{y}"))
                    }
                    (op, x, y) => {
                        let (m, n) = (
                            int(self, x, &format!("`{op}`"))?,
                            int(self, y, &format!("`{op}`"))?,
                        );
                        let r = match op {
                            "+" => m.checked_add(n),
                            "-" => m.checked_sub(n),
                            "*" => m.checked_mul(n),
                            "/" | "%" if n == 0 => {
                                self.fail(span, "division by zero");
                                return Err(());
                            }
                            "/" => m.checked_div_euclid(n),
                            "%" => m.checked_rem_euclid(n),
                            _ => Some(cmp(op, m.cmp(&n)) as i64),
                        };
                        match r {
                            Some(v) => Val::Int(v),
                            None => {
                                self.fail(span, format!("`{op}` overflows"));
                                return Err(());
                            }
                        }
                    }
                }
            }
            Expr::Call(f, args) => {
                let mut vs = Vec::new();
                for a in args {
                    vs.push(self.eval(span, a)?);
                }
                self.builtin(span, f, vs)?
            }
        })
    }

    fn builtin(&mut self, span: Span, f: &str, vs: Vec<Val>) -> Run<Val> {
        let arity = match f {
            "len" => 1,
            "split" => 2,
            "names" => 3,
            "count" | "part" => {
                let hint = match f {
                    "count" => "pass a `[...]` list and use `len(xs)`, or `len(split(s, sep))`",
                    _ => "pass a `[...]` list and use `xs[i]`, or `split(s, sep)[i]`",
                };
                self.fail(span, format!("`{f}` is gone; {hint}"));
                return Err(());
            }
            _ => {
                let hint = if self
                    .defs
                    .keys()
                    .any(|k| k == f || k.ends_with(&format!(".{f}")))
                {
                    format!("; `{f}` is a template, so call it as a statement on its own line")
                } else {
                    String::new()
                };
                self.fail(
                    span,
                    format!("unknown function `{f}`{hint} (functions: len, split, names)"),
                );
                return Err(());
            }
        };
        if vs.len() != arity {
            self.fail(
                span,
                format!(
                    "`{f}` takes {arity} argument{}, found {}",
                    if arity == 1 { "" } else { "s" },
                    vs.len()
                ),
            );
            return Err(());
        }
        if f == "len" {
            return Ok(Val::Int(match &vs[0] {
                Val::List(v) => v.len(),
                Val::Str(s) => s.chars().count(),
                Val::Int(n) => n.to_string().len(),
            } as i64));
        }
        if f == "names" {
            let (Val::Int(lo), Val::Int(hi)) = (&vs[1], &vs[2]) else {
                self.fail(span, "`names` needs a prefix and two numbers");
                return Err(());
            };
            let mut out = Vec::new();
            for k in *lo..=*hi {
                self.tick(span)?;
                out.push(Val::Str(format!("{}{k}", vs[0])));
            }
            return Ok(Val::List(out));
        }
        let (Val::Str(s), Val::Str(sep)) = (&vs[0], &vs[1]) else {
            self.fail(span, "`split` needs two strings");
            return Err(());
        };
        if sep.is_empty() {
            self.fail(span, "`split` needs a non-empty separator");
            return Err(());
        }
        // An empty or blank string has no fields, so a loop over it runs zero times.
        let fields: Vec<Val> = if s.trim().is_empty() {
            Vec::new()
        } else if sep.trim().is_empty() {
            s.split_whitespace().map(|f| Val::Str(f.into())).collect()
        } else {
            // Spaces past the one after the separator survive, so a body field can indent.
            s.split(sep.as_str())
                .map(|f| {
                    let f = f.trim_end();
                    Val::Str(f.strip_prefix(' ').unwrap_or(f).to_string())
                })
                .collect()
        };
        Ok(Val::List(fields))
    }
}

fn cmp(op: &str, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        "<" => o == Less,
        "<=" => o != Greater,
        ">" => o == Greater,
        _ => o != Less,
    }
}

pub fn expand_text(path: &str, text: &str, budget: u64) -> Loaded {
    expand(Source::new(path, text), Vec::new(), budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lines prints the expanded tokens one statement per line, for comparing against expected text.
    fn lines(text: &str) -> (Vec<String>, Vec<String>) {
        let l = expand_text("t.fitd", text, DEFAULT_BUDGET);
        let mut out = Vec::new();
        let mut cur: Vec<String> = Vec::new();
        for t in &l.toks {
            let w = match &t.tok {
                Tok::Newline | Tok::Eof => {
                    if !cur.is_empty() {
                        out.push(cur.join(" "));
                        cur.clear();
                    }
                    continue;
                }
                Tok::Str(s) => format!("{s:?}"),
                Tok::Body(s) => format!("| {s}"),
                Tok::Cell(s) => s.clone(),
                Tok::LBrace => "{".into(),
                Tok::RBrace => "}".into(),
                Tok::LBracket => "[".into(),
                Tok::RBracket => "]".into(),
                Tok::Comma => ",".into(),
                Tok::Eq => "=".into(),
                _ => t.src(&l.src.text).to_string(),
            };
            cur.push(w);
        }
        let errs = l
            .diags
            .iter()
            .map(|d| format!("{} {}", d.code, d.msg))
            .collect();
        (out, errs)
    }

    #[test]
    fn plain_files_pass_through() {
        let (out, errs) = lines("a \"A\" {\n  b\n}\na -> b [weight=2]\n");
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(out, ["a \"A\" {", "b", "}", "a -> b [ weight = 2 ]"]);
    }

    #[test]
    fn calls_bind_defaults_keys_and_lists() {
        let src = "def box(id, title=\"T\", tags=[]) {\n  ${id} \"${title} ${len(tags)}\"\n}\nbox(a)\nbox(b, title=\"B\", tags=[x, y])\nbox(c, \"C\")\n";
        let (out, errs) = lines(src);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(out, ["a \"T 0\"", "b \"B 2\"", "c \"C 0\""]);
    }

    #[test]
    fn children_land_in_the_last_node() {
        let src = "def t(id) {\n  with s {\n    ${id}\n  }\n}\nt(a) {\n  b\n}\n";
        let (out, errs) = lines(src);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(out, ["with s {", "a {", "b", "}", "}"]);
    }

    #[test]
    fn loops_conditions_and_rows() {
        let src = "for i, v in [x, y] {\n  if i == 2 {\n    n${i}_${v}\n  } else {\n    m${v}\n  }\n}\nfor k in 1..2 {\n  ~ [${k}, for c in split(\"p q\", \" \") { \"${c}\" }]\n}\n";
        let (out, errs) = lines(src.replace("[x, y]", "[\"x\", \"y\"]").as_str());
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(
            out,
            [
                "mx",
                "n2_y",
                "~ [ 1 , \"p\" , \"q\" ]",
                "~ [ 2 , \"p\" , \"q\" ]"
            ]
        );
    }

    #[test]
    fn errors_name_the_template_and_call() {
        let (_, errs) = lines("def t(a) {\n  ${b}\n}\nt(1)\n");
        assert!(
            errs.iter().any(|e| e.contains("unknown variable `b`")),
            "{errs:?}"
        );
        let (_, errs) = lines("t.box(1)\n");
        assert!(
            errs.iter().any(|e| e.contains("unknown template `t.box`")),
            "{errs:?}"
        );
        let (_, errs) = lines("${part(\"a\", \",\", 1)}\n");
        assert!(
            errs.iter().any(|e| e.contains("`part` is gone")),
            "{errs:?}"
        );
        let (_, errs) = lines("def f(n) {\n  f(${n + 1})\n}\nf(1)\n");
        assert!(
            errs.iter()
                .any(|e| e.starts_with("E0012") && e.contains("200 deep")),
            "{errs:?}"
        );
    }

    #[test]
    fn budget_stops_runaway_loops() {
        let l = expand_text("t.fitd", "for i in 1..1000000 {\n  n${i}\n}\n", 1000);
        assert!(l.diags.iter().any(|d| d.code == "E0013"));
    }
}
