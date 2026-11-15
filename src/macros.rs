use crate::diag::{Diag, Seg, Source, Span};
use std::collections::HashMap;

pub const DEFAULT_BUDGET: u64 = 100_000;
const MAX_DEPTH: usize = 200;

pub struct Loaded {
    pub src: Source,
    pub parse: String,
    pub diags: Vec<Diag>,
}

pub fn load(path: &str, text: &str, budget: u64) -> Loaded {
    let (a, inc) = crate::include::expand(path, text);
    expand(a, inc, budget)
}

#[cfg(test)]
pub fn expand_text(path: &str, text: &str, budget: u64) -> Loaded {
    expand(Source::new(path, text), Vec::new(), budget)
}

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(i64),
    Str(String),
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Val::Int(n) => write!(f, "{n}"),
            Val::Str(s) => f.write_str(s),
        }
    }
}

impl Val {
    fn truthy(&self) -> bool {
        match self {
            Val::Int(n) => *n != 0,
            Val::Str(s) => !s.is_empty(),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Val::Int(_) => "a number",
            Val::Str(_) => "a string",
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

const OPS: [&str; 17] = [
    "..", "==", "!=", "<=", ">=", "+", "-", "*", "/", "%", "<", ">", "(", ")", ",", "=", ".",
];

fn tokens(s: &str) -> Result<Vec<T>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    'outer: while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            break;
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
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct P {
    t: Vec<T>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&T> {
        self.t.get(self.i)
    }

    fn op(&mut self, o: &str) -> bool {
        if self.peek()
            == Some(&T::Op(match OPS.iter().find(|x| **x == o) {
                Some(x) => x,
                None => return false,
            }))
        {
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

    fn name(&mut self, what: &str) -> Result<String, String> {
        match self.peek().cloned() {
            Some(T::Id(x)) => {
                self.i += 1;
                Ok(x)
            }
            _ => Err(format!("expected {what}")),
        }
    }

    fn expect(&mut self, o: &str) -> Result<(), String> {
        if self.op(o) {
            Ok(())
        } else {
            Err(format!("expected `{o}`"))
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
                let op = OPS.iter().find(|x| **x == o).unwrap();
                return Ok(Expr::Bin(op, Box::new(a), Box::new(self.sum()?)));
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
        let mut a = self.unary()?;
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
            a = Expr::Bin(op, Box::new(a), Box::new(self.unary()?));
        }
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
            Some(T::Id(x)) if !matches!(x.as_str(), "and" | "or") => {
                self.i += 1;
                if self.op("(") {
                    return Ok(Expr::Call(x, self.args()?));
                }
                Ok(Expr::Var(x))
            }
            Some(T::Op("(")) => {
                self.i += 1;
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(t) => Err(format!("expected a value, found {}", show(&t))),
            None => Err("expected a value, found end of line".into()),
        }
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        if self.op(")") {
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            if self.op(")") {
                return Ok(out);
            }
            self.expect(",")?;
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
    let e = p.expr()?;
    p.done()?;
    Ok(e)
}

#[derive(Clone, Debug)]
enum Piece {
    Lit(String),
    Expr(Expr),
}

fn pieces(line: &str) -> Result<Option<Vec<Piece>>, String> {
    if !line.contains("${") {
        return Ok(None);
    }
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut rest = line;
    while let Some(k) = rest.find("${") {
        if rest[..k].ends_with('$') {
            lit += &rest[..k - 1];
            lit += "${";
            rest = &rest[k + 2..];
            continue;
        }
        lit += &rest[..k];
        let body = &rest[k + 2..];
        let mut in_str = false;
        let mut end = None;
        let mut prev = ' ';
        for (j, c) in body.char_indices() {
            match c {
                '"' if prev != '\\' => in_str = !in_str,
                '}' if !in_str => {
                    end = Some(j);
                    break;
                }
                _ => {}
            }
            prev = c;
        }
        let end = end.ok_or("`${` without a closing `}`")?;
        out.push(Piece::Lit(std::mem::take(&mut lit)));
        out.push(Piece::Expr(parse_expr(&body[..end])?));
        rest = &body[end + 1..];
    }
    lit += rest;
    out.push(Piece::Lit(lit));
    Ok(Some(out))
}

fn comment_start(line: &str) -> usize {
    let (mut in_str, mut depth, mut prev) = (false, 0, ' ');
    let mut it = line.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        match c {
            '\\' if in_str => {
                it.next();
            }
            '"' => in_str = !in_str,
            '$' if !in_str && it.peek().is_some_and(|&(_, n)| n == '{') => {
                it.next();
                depth += 1;
            }
            '}' if depth > 0 && !in_str => depth -= 1,
            '#' if !in_str && depth == 0 && prev != '=' => return i,
            _ => {}
        }
        prev = c;
    }
    line.len()
}

#[derive(Clone, Debug)]
enum Iter {
    Range(Expr, Expr),
    List(Vec<Val>),
}

#[derive(Clone, Debug)]
enum Kind {
    Plain,
    Text(Vec<Piece>),
    For(Vec<String>, Iter, Vec<Item>),
    If(Vec<(Option<Expr>, Vec<Item>)>),
    Rows(Vec<Row>),
    Skip,
}

/// Row is one call in a template block: its line, nesting depth, default template, and text.
#[derive(Clone, Debug)]
struct Row {
    line: usize,
    depth: usize,
    name: String,
    text: String,
}

#[derive(Clone, Debug)]
struct Item {
    lo: usize,
    hi: usize,
    kind: Kind,
}

struct Def {
    params: Vec<String>,
    body: Vec<Item>,
    line: usize,
}

enum Line {
    Item(Kind),
    For(Vec<String>, Iter),
    If(Expr),
    ElseIf(Expr),
    Else,
    End,
    Def(String, Vec<String>),
}

fn is_kw_line(t: &str) -> Option<(&str, &str)> {
    let w = t
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or("");
    if !matches!(w, "for" | "if" | "else" | "end" | "def") {
        return None;
    }
    let rest = &t[w.len()..];
    let r = rest.trim_start();
    if rest.starts_with('-') || r.starts_with("->") || r.starts_with("--") || r.starts_with("..") {
        return None;
    }
    Some((w, r))
}

/// block_name gives the `MOD.name` that line `t` holds alone, a possible template block opener.
fn block_name(t: &str) -> Option<&str> {
    let (m, n) = t.split_once('.')?;
    let id = |w: &str| {
        !w.is_empty()
            && !w.starts_with(|c: char| c.is_ascii_digit())
            && w.chars().all(|c| c.is_alphanumeric() || c == '_')
    };
    (id(m) && id(n)).then_some(t)
}

/// row_words splits a template row into words, each with whether it was quoted; `${...}` stays raw, quotes and all.
fn row_words(t: &str) -> Result<Vec<(bool, String)>, String> {
    let cs: Vec<char> = t.chars().collect();
    let mut words = Vec::new();
    let mut i = 0;
    // skip_expr copies `${...}` from cs[i] through its `}` into w.
    let skip_expr = |i: &mut usize, w: &mut String| -> Result<(), String> {
        let mut in_str = false;
        while let Some(&c) = cs.get(*i) {
            w.push(c);
            *i += 1;
            match c {
                '\\' if in_str => {
                    if let Some(&n) = cs.get(*i) {
                        w.push(n);
                        *i += 1;
                    }
                }
                '"' => in_str = !in_str,
                '}' if !in_str => return Ok(()),
                _ => {}
            }
        }
        Err("`${` without a closing `}`".into())
    };
    let opens =
        |i: usize| cs[i] == '$' && cs.get(i + 1) == Some(&'{') && (i == 0 || cs[i - 1] != '$');
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '#' {
            break;
        } else if c == '"' {
            let mut w = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => return Err("unterminated string".into()),
                    Some('"') => break,
                    Some('\\') if i + 1 < cs.len() => {
                        w.push(cs[i + 1]);
                        i += 2;
                    }
                    Some(_) if opens(i) => skip_expr(&mut i, &mut w)?,
                    Some(&o) => {
                        w.push(o);
                        i += 1;
                    }
                }
            }
            i += 1;
            words.push((true, w));
        } else {
            let mut w = String::new();
            while i < cs.len() && !cs[i].is_whitespace() && cs[i] != '"' {
                if opens(i) {
                    skip_expr(&mut i, &mut w)?;
                } else {
                    w.push(cs[i]);
                    i += 1;
                }
            }
            words.push((false, w));
        }
    }
    Ok(words)
}

fn classify(line: &str) -> Result<Line, String> {
    let t = line.trim();
    let verbatim = t.starts_with('|') || t.starts_with('~');
    if t.starts_with('#') || verbatim && !t.contains("${") {
        return Ok(Line::Item(Kind::Plain));
    }
    let Some((w, rest)) = is_kw_line(t) else {
        let cut = if verbatim {
            line.len()
        } else {
            comment_start(line)
        };
        return Ok(Line::Item(match pieces(&line[..cut])? {
            Some(mut p) => {
                p.push(Piece::Lit(line[cut..].to_string()));
                Kind::Text(p)
            }
            None => Kind::Plain,
        }));
    };
    if w == "for" {
        return for_line(rest);
    }
    let mut p = P {
        t: tokens(rest)?,
        i: 0,
    };
    Ok(match w {
        "end" => {
            p.done()?;
            Line::End
        }
        "else" => {
            if p.word("if") {
                let e = p.expr()?;
                p.done()?;
                Line::ElseIf(e)
            } else {
                p.done()?;
                Line::Else
            }
        }
        "if" => {
            let e = p.expr()?;
            p.done()?;
            Line::If(e)
        }
        _ => {
            let name = p.name("a template name: `def NAME(a, b)`")?;
            p.expect("(")?;
            let mut params = Vec::new();
            if !p.op(")") {
                loop {
                    params.push(p.name("a parameter name")?);
                    if p.op(")") {
                        break;
                    }
                    p.expect(",")?;
                }
            }
            p.done()?;
            Line::Def(name, params)
        }
    })
}

const FOR_SHAPE: &str = "`for NAME in LO..HI`, `for NAME in a, b, c`, or `for I, NAME in a, b, c`";

fn for_line(rest: &str) -> Result<Line, String> {
    let padded = format!(" {rest} ");
    let Some(k) = padded.find(" in ") else {
        return Err(format!("expected `in`: {FOR_SHAPE}"));
    };
    let vars: Vec<String> = padded[..k]
        .split(',')
        .map(|v| v.trim().to_string())
        .collect();
    let ok = |v: &String| {
        !v.is_empty()
            && v.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !v.starts_with(|c: char| c.is_ascii_digit())
    };
    if vars.len() > 2 || !vars.iter().all(ok) {
        return Err(format!("expected one or two variable names: {FOR_SHAPE}"));
    }
    let var = vars;
    let items = padded[k + 4..].trim();
    if let Ok(t) = tokens(items)
        && t.contains(&T::Op(".."))
    {
        let mut p = P { t, i: 0 };
        let lo = p.sum()?;
        p.expect("..")?;
        let hi = p.sum()?;
        p.done()?;
        return Ok(Line::For(var, Iter::Range(lo, hi)));
    }
    Ok(Line::For(var, Iter::List(list(items)?)))
}

fn list(s: &str) -> Result<Vec<Val>, String> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() || c == ',' {
            i += 1;
        } else if c == '#' {
            break;
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
            out.push(Val::Str(s));
        } else {
            let lo = i;
            while i < cs.len() && !(cs[i].is_whitespace() || cs[i] == ',' || cs[i] == '#') {
                i += 1;
            }
            let w: String = cs[lo..i].iter().collect();
            out.push(match w.parse() {
                Ok(n) => Val::Int(n),
                Err(_) => Val::Str(w),
            });
        }
    }
    if out.is_empty() {
        return Err("`for ... in` needs a range `LO..HI` or a list of items".into());
    }
    Ok(out)
}

struct Builder<'a> {
    lines: &'a [&'a str],
    mods: &'a [Option<&'a str>],
    i: usize,
    defs: HashMap<String, Def>,
    /// names holds every template key, gathered before building so a call may precede its `def`.
    names: std::collections::HashSet<String>,
    /// styles holds every `style NAME` declared, so a module line naming one opens a style block.
    styles: std::collections::HashSet<String>,
    errs: Vec<(usize, Diag)>,
}

enum Stop {
    Eof,
    End,
    Else(Option<Expr>),
}

impl Builder<'_> {
    fn err(&mut self, line: usize, msg: impl Into<String>) {
        self.errs.push((line, Diag::error("E0002", msg)));
    }

    fn block(&mut self, top: bool) -> (Vec<Item>, Stop, usize) {
        let mut items = Vec::new();
        while self.i < self.lines.len() {
            let at = self.i;
            self.i += 1;
            let t = self.lines[at].trim();
            let head = t.split_whitespace().next().unwrap_or("");
            if self.calls(at, head, &t[head.len()..]) {
                let kind = self.rows(at, head.to_string(), &t[head.len()..]);
                items.push(Item {
                    lo: at,
                    hi: self.i - 1,
                    kind,
                });
                continue;
            }
            let line = match classify(self.lines[at]) {
                Ok(l) => l,
                Err(m) => {
                    self.err(at, m);
                    items.push(Item {
                        lo: at,
                        hi: at,
                        kind: Kind::Skip,
                    });
                    continue;
                }
            };
            let kind = match line {
                Line::Item(k) => k,
                Line::End | Line::Else | Line::ElseIf(_) if top => {
                    self.err(at, "`end` or `else` with no open `for`, `if`, or `def`");
                    Kind::Skip
                }
                Line::End => return (items, Stop::End, at),
                Line::Else => return (items, Stop::Else(None), at),
                Line::ElseIf(e) => return (items, Stop::Else(Some(e)), at),
                Line::For(var, it) => {
                    let (body, stop, _) = self.block(false);
                    self.close(at, &stop, "for");
                    Kind::For(var, it, body)
                }
                Line::If(cond) => {
                    let mut arms = Vec::new();
                    let mut cond = Some(cond);
                    loop {
                        let (body, stop, end) = self.block(false);
                        let had_else = cond.is_none();
                        arms.push((cond.take(), body));
                        match stop {
                            Stop::Else(next) if !had_else => cond = next,
                            Stop::Else(_) => self.err(end, "`else` after `else`"),
                            Stop::End => break,
                            Stop::Eof => {
                                self.close(at, &Stop::Eof, "if");
                                break;
                            }
                        }
                    }
                    Kind::If(arms)
                }
                Line::Def(name, params) => {
                    let (body, stop, _) = self.block(false);
                    self.close(at, &stop, "def");
                    if !top {
                        self.err(at, "`def` is allowed only at the top level");
                    } else {
                        let key = match self.mods[at] {
                            Some(m) => format!("{m}.{name}"),
                            None => name,
                        };
                        if let Some(old) = self.defs.get(&key) {
                            let line = old.line;
                            self.errs.push((
                                at,
                                Diag::error("E0012", format!("template `{key}` is defined twice"))
                                    .note(format!("first defined at {}", at_marker(line))),
                            ));
                        } else {
                            let def = Def {
                                params,
                                body,
                                line: at,
                            };
                            self.defs.insert(key, def);
                        }
                    }
                    Kind::Skip
                }
            };
            items.push(Item {
                lo: at,
                hi: self.i - 1,
                kind,
            });
        }
        (items, Stop::Eof, self.lines.len())
    }

    /// calls reports whether a line starting with `head` calls a template; an edge from a node named like one, or a bare keyword, does not.
    fn calls(&self, at: usize, head: &str, rest: &str) -> bool {
        let r = rest.trim_start();
        let id = |w: &str| {
            !w.is_empty()
                && !w.starts_with(|c: char| c.is_ascii_digit())
                && w.chars().all(|c| c.is_alphanumeric() || c == '_')
        };
        let ok = match head.split_once('.') {
            Some((m, n)) => id(m) && id(n) && self.names.contains(head),
            None => {
                id(head)
                    && !crate::parse::KEYWORDS.contains(&head)
                    && (self.names.contains(head)
                        || self.mods[at]
                            .is_some_and(|m| self.names.contains(&format!("{m}.{head}"))))
            }
        };
        let style = r.is_empty() && self.mods[at].is_some() && self.styles.contains(head);
        ok && !style && !["->", "--", ".."].iter().any(|o| r.starts_with(o))
    }

    /// rows reads the lines indented under template block opener `at`, one call each; with none it is one call without arguments.
    fn rows(&mut self, at: usize, name: String, rest: &str) -> Kind {
        let col = indent(self.lines[at]);
        let mut rows = Vec::new();
        let mut cols: Vec<usize> = Vec::new();
        if !rest.trim().is_empty() {
            rows.push(Row {
                line: at,
                depth: 0,
                name: name.clone(),
                text: rest.to_string(),
            });
            cols.push(col);
        }
        let mut k = self.i;
        while k < self.lines.len() {
            let l = self.lines[k];
            let t = l.trim();
            k += 1;
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let c = indent(l);
            if c <= col {
                break;
            }
            self.i = k;
            while cols.last().is_some_and(|&x| x >= c) {
                cols.pop();
            }
            let depth = cols.len();
            cols.push(c);
            rows.push(Row {
                line: k - 1,
                depth,
                name: name.clone(),
                text: t.to_string(),
            });
        }
        if rows.is_empty() {
            rows.push(Row {
                line: at,
                depth: 0,
                name,
                text: String::new(),
            });
        }
        Kind::Rows(rows)
    }

    fn close(&mut self, at: usize, stop: &Stop, what: &str) {
        match stop {
            Stop::End => {}
            Stop::Eof => self.err(at, format!("`{what}` has no matching `end`")),
            Stop::Else(_) => self.err(at, format!("`else` inside `{what}` belongs to no `if`")),
        }
    }
}

struct Expander<'a> {
    lines: &'a [&'a str],
    mods: &'a [Option<&'a str>],
    defs: &'a HashMap<String, Def>,
    text: String,
    parse: String,
    origin: Vec<usize>,
    raw: Vec<usize>,
    frames: Vec<HashMap<String, Val>>,
    calls: Vec<(String, usize)>,
    steps: u64,
    budget: u64,
    out_of_budget: bool,
    errs: Vec<(usize, Diag)>,
    /// shift re-bases expanded lines: drop the first indent of a body, add the indent of its directive.
    shift: (usize, usize),
}

fn indent(s: &str) -> usize {
    s.len() - s.trim_start().len()
}

type Run<T> = Result<T, ()>;

impl Expander<'_> {
    fn emit(&mut self, line: usize, s: &str, hidden: bool) {
        let owned;
        let s = if self.shift == (0, 0) || s.trim().is_empty() {
            s
        } else {
            owned = format!("{}{}", " ".repeat(self.at(s)), s.trim_start());
            &owned
        };
        self.text += s;
        self.text.push('\n');
        if hidden {
            self.parse.extend(std::iter::repeat_n(' ', s.len()));
        } else {
            self.parse += s;
        }
        self.parse.push('\n');
        self.origin.push(line);
    }

    /// at gives the indent line `s` gets once expanded.
    fn at(&self, s: &str) -> usize {
        self.shift.1 + indent(s).saturating_sub(self.shift.0)
    }

    /// nest runs `body` re-based under the directive on line `at`, as if its lines stood at that indent.
    fn nest(&mut self, at: usize, body: &[Item]) -> Run<()> {
        self.nest_at(self.at(self.lines[at]), body)
    }

    /// nest_at runs `body` re-based to indent `base`.
    fn nest_at(&mut self, base: usize, body: &[Item]) -> Run<()> {
        let first = body
            .iter()
            .map(|it| self.lines[it.lo])
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .map(indent)
            .min()
            .unwrap_or(0);
        let old = self.shift;
        self.shift = (first, base);
        let r = self.run(body, false);
        self.shift = old;
        r
    }

    /// invoke expands template `name` called on line `at`, at indent `base` when given.
    fn invoke(&mut self, at: usize, name: &str, args: &[Expr], base: Option<usize>) -> Run<()> {
        let defs = self.defs;
        let local = self.mods[at]
            .filter(|_| !name.contains('.'))
            .and_then(|m| defs.get(&format!("{m}.{name}")));
        let Some(def) = local.or_else(|| defs.get(name)) else {
            let mut known: Vec<&String> = defs.keys().collect();
            known.sort();
            let hint = if known.is_empty() {
                String::new()
            } else {
                format!(
                    " (templates: {})",
                    known
                        .iter()
                        .map(|k| k.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            self.fail(at, format!("unknown template `{name}`{hint}"));
            return Err(());
        };
        if args.len() != def.params.len() {
            self.fail(
                at,
                format!(
                    "`{name}` takes {} argument{} ({}), found {}",
                    def.params.len(),
                    if def.params.len() == 1 { "" } else { "s" },
                    def.params.join(", "),
                    args.len()
                ),
            );
            return Err(());
        }
        if self.calls.len() >= MAX_DEPTH {
            self.fail(at, format!("templates nest more than {MAX_DEPTH} deep"));
            self.out_of_budget = true;
            return Err(());
        }
        let mut frame = HashMap::new();
        for (p, a) in def.params.iter().zip(args) {
            let v = self.eval(at, a)?;
            frame.insert(p.clone(), v);
        }
        self.frames.push(frame);
        self.calls.push((name.to_string(), at));
        let r = match base {
            Some(b) => self.nest_at(b, &def.body),
            None => self.nest(at, &def.body),
        };
        self.calls.pop();
        self.frames.pop();
        r
    }

    fn raw(&mut self, item: &Item) {
        for k in item.lo..=item.hi {
            self.raw[k] = self.text.len();
            let line = self.lines[k];
            self.emit(k, line, !matches!(item.kind, Kind::Plain));
        }
    }

    fn fail(&mut self, line: usize, msg: impl Into<String>) {
        let mut d = Diag::error("E0012", msg);
        for (name, at) in self.calls.iter().rev().take(4) {
            d = d.note(format!("in `{name}` at {}", at_marker(*at)));
        }
        self.errs.push((line, d));
    }

    fn tick(&mut self, line: usize) -> Run<()> {
        self.steps += 1;
        if self.steps > self.budget {
            if !self.out_of_budget {
                self.out_of_budget = true;
                let d = Diag::error(
                    "E0013",
                    format!("macro expansion ran past {} steps", self.budget),
                );
                self.errs.push((line, d.help("check for runaway recursion or a huge `for` range, or raise `--expand-budget`")));
            }
            return Err(());
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&Val> {
        self.frames
            .last()
            .unwrap()
            .get(name)
            .or_else(|| self.frames[0].get(name))
    }

    fn eval(&mut self, line: usize, e: &Expr) -> Run<Val> {
        let int = |me: &mut Self, v: Val, what: &str| match v {
            Val::Int(n) => Ok(n),
            o => {
                me.fail(
                    line,
                    format!("{what} needs a number, found {} \"{o}\"", o.kind()),
                );
                Err(())
            }
        };
        Ok(match e {
            Expr::Lit(v) => v.clone(),
            Expr::Var(x) => match self.get(x) {
                Some(v) => v.clone(),
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
                        format!(
                            " (known: {})",
                            known
                                .iter()
                                .map(|k| k.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    self.fail(line, format!("unknown variable `{x}`{d}"));
                    return Err(());
                }
            },
            Expr::Bin("and", a, b) => {
                let x = self.eval(line, a)?;
                if x.truthy() { self.eval(line, b)? } else { x }
            }
            Expr::Bin("or", a, b) => {
                let x = self.eval(line, a)?;
                if x.truthy() { x } else { self.eval(line, b)? }
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = (self.eval(line, a)?, self.eval(line, b)?);
                match *op {
                    "==" => Val::Int((x == y) as i64),
                    "!=" => Val::Int((x != y) as i64),
                    "+" if matches!((&x, &y), (Val::Str(_), _) | (_, Val::Str(_))) => {
                        Val::Str(format!("{x}{y}"))
                    }
                    _ => {
                        let (m, n) = (
                            int(self, x, &format!("`{op}`"))?,
                            int(self, y, &format!("`{op}`"))?,
                        );
                        let r = match *op {
                            "+" => m.checked_add(n),
                            "-" => m.checked_sub(n),
                            "*" => m.checked_mul(n),
                            "/" | "%" if n == 0 => {
                                self.fail(line, "division by zero");
                                return Err(());
                            }
                            "/" => m.checked_div_euclid(n),
                            "%" => m.checked_rem_euclid(n),
                            _ => Some(cmp(op, m.cmp(&n)) as i64),
                        };
                        match r {
                            Some(v) => Val::Int(v),
                            None => {
                                self.fail(line, format!("`{op}` overflows"));
                                return Err(());
                            }
                        }
                    }
                }
            }
            Expr::Call(f, args) => {
                let mut vs = Vec::new();
                for a in args {
                    vs.push(self.eval(line, a)?);
                }
                self.call(line, f, vs)?
            }
        })
    }

    fn call(&mut self, line: usize, f: &str, vs: Vec<Val>) -> Run<Val> {
        let arity = match f {
            "names" | "part" => 3,
            "count" => 2,
            _ => {
                let hint = if self.defs.contains_key(f) {
                    format!("; `{f}` is a template, so write `{f} ARGS` on its own line")
                } else {
                    String::new()
                };
                self.fail(line, format!("unknown function `{f}`{hint}"));
                return Err(());
            }
        };
        if vs.len() != arity {
            self.fail(
                line,
                format!("`{f}` takes {arity} arguments, found {}", vs.len()),
            );
            return Err(());
        }
        let num = |me: &mut Self, v: &Val| match v {
            Val::Int(n) => Ok(*n),
            o => {
                me.fail(
                    line,
                    format!("`{f}` needs a number, found {} \"{o}\"", o.kind()),
                );
                Err(())
            }
        };
        let text = |me: &mut Self, v: &Val| match v {
            Val::Str(s) => Ok(s.clone()),
            o => {
                me.fail(
                    line,
                    format!("`{f}` needs a string, found {} \"{o}\"", o.kind()),
                );
                Err(())
            }
        };
        if f != "names" {
            let (s, sep) = (text(self, &vs[0])?, text(self, &vs[1])?);
            if sep.is_empty() {
                self.fail(line, format!("`{f}` needs a non-empty separator"));
                return Err(());
            }
            // An empty or blank string has no fields, so `for i in 1..count(s, ";")` runs zero times.
            let fields: Vec<&str> = if s.trim().is_empty() {
                Vec::new()
            } else if sep.trim().is_empty() {
                s.split_whitespace().collect()
            } else {
                // Spaces past the one after the separator survive, so a body field can indent.
                s.split(sep.as_str())
                    .map(|f| {
                        let f = f.trim_end();
                        f.strip_prefix(' ').unwrap_or(f)
                    })
                    .collect()
            };
            if f == "count" {
                return Ok(Val::Int(fields.len() as i64));
            }
            let i = num(self, &vs[2])?;
            return match usize::try_from(i)
                .ok()
                .and_then(|i| fields.get(i.wrapping_sub(1)))
            {
                Some(p) => Ok(Val::Str(p.to_string())),
                None => {
                    self.fail(
                        line,
                        format!("`part` index {i} is outside 1..{}", fields.len()),
                    );
                    Err(())
                }
            };
        }
        let (lo, hi) = (num(self, &vs[1])?, num(self, &vs[2])?);
        let mut s = String::new();
        for k in lo..=hi {
            self.tick(line)?;
            if k > lo {
                s += ", ";
            }
            s += &format!("{}{k}", vs[0]);
        }
        Ok(Val::Str(s))
    }

    fn run(&mut self, items: &[Item], top: bool) -> Run<()> {
        for (k, item) in items.iter().enumerate() {
            if top {
                self.raw(item);
            }
            if self.item(item, top).is_err() && self.out_of_budget {
                if top {
                    items[k + 1..].iter().for_each(|it| self.raw(it));
                }
                return Err(());
            }
        }
        Ok(())
    }

    /// word evaluates a template row word: a lone unquoted `${e}` keeps its type, unquoted digits are a number, the rest is text.
    fn word(&mut self, line: usize, quoted: bool, w: &str) -> Run<Val> {
        let ps = match pieces(w) {
            Ok(p) => p.unwrap_or_else(|| vec![Piece::Lit(w.to_string())]),
            Err(m) => {
                self.fail(line, m);
                return Err(());
            }
        };
        if let (false, [Piece::Lit(a), Piece::Expr(e), Piece::Lit(b)]) = (quoted, &ps[..])
            && a.is_empty()
            && b.is_empty()
        {
            return self.eval(line, e);
        }
        let mut s = String::new();
        for p in &ps {
            match p {
                Piece::Lit(l) => s += l,
                Piece::Expr(e) => s += &self.eval(line, e)?.to_string(),
            }
        }
        Ok(match s.parse() {
            Ok(n) if !quoted && s.bytes().all(|b| b.is_ascii_digit()) => Val::Int(n),
            _ => Val::Str(s),
        })
    }

    fn item(&mut self, item: &Item, top: bool) -> Run<()> {
        let at = item.lo;
        self.tick(at)?;
        match &item.kind {
            Kind::Plain if !top => {
                let line = self.lines[at];
                self.emit(at, line, false);
            }
            Kind::Plain | Kind::Skip => {}
            Kind::Text(ps) => {
                let mut s = String::new();
                for p in ps {
                    match p {
                        Piece::Lit(l) => s += l,
                        Piece::Expr(e) => s += &self.eval(at, e)?.to_string(),
                    }
                }
                if s.contains('\n') {
                    self.fail(at, "an interpolated value holds a line break");
                    return Err(());
                }
                self.emit(at, &s, false);
            }
            Kind::For(var, it, body) => {
                let vals: Box<dyn Iterator<Item = Val>> = match it {
                    Iter::Range(lo, hi) => {
                        let lo = self.eval(at, lo)?;
                        let hi = self.eval(at, hi)?;
                        match (lo, hi) {
                            (Val::Int(a), Val::Int(b)) => Box::new((a..=b).map(Val::Int)),
                            (a, b) => {
                                self.fail(
                                    at,
                                    format!(
                                        "a range needs numbers, found {} and {}",
                                        a.kind(),
                                        b.kind()
                                    ),
                                );
                                return Err(());
                            }
                        }
                    }
                    Iter::List(vs) => Box::new(vs.clone().into_iter()),
                };
                for (k, v) in vals.enumerate() {
                    self.tick(at)?;
                    let frame = self.frames.last_mut().unwrap();
                    if let [i, _] = &var[..] {
                        frame.insert(i.clone(), Val::Int(k as i64 + 1));
                    }
                    frame.insert(var.last().unwrap().clone(), v);
                    self.nest(at, body)?;
                }
            }
            Kind::If(arms) => {
                for (cond, body) in arms {
                    let go = match cond {
                        Some(c) => self.eval(at, c)?.truthy(),
                        None => true,
                    };
                    if go {
                        return self.nest(at, body);
                    }
                }
            }
            Kind::Rows(rows) => {
                let mut bases = vec![self.at(self.lines[at])];
                for r in rows {
                    bases.truncate(r.depth + 1);
                    let mut words = match row_words(&r.text) {
                        Ok(w) => w,
                        Err(m) => {
                            self.fail(r.line, m);
                            return Err(());
                        }
                    };
                    let name = match words.first() {
                        Some((false, w))
                            if block_name(w).is_some() && self.defs.contains_key(w) =>
                        {
                            words.remove(0).1
                        }
                        _ => r.name.clone(),
                    };
                    let mut args = Vec::new();
                    for (quoted, w) in &words {
                        args.push(Expr::Lit(self.word(r.line, *quoted, w)?));
                    }
                    let from = self.text.len();
                    self.invoke(r.line, &name, &args, Some(bases[r.depth]))?;
                    // Children sit right of every line the parent emitted, so its style opener cannot close the parent's scope.
                    let deep = self.text[from..]
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(indent)
                        .max();
                    bases.push(deep.map_or(bases[r.depth], |d| d + 1));
                }
            }
        }
        Ok(())
    }
}

fn at_marker(line: usize) -> String {
    format!("\u{1}{line}\u{1}")
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

fn expand(a: Source, inc: Vec<Diag>, budget: u64) -> Loaded {
    let lines: Vec<&str> = a.text.split('\n').collect();
    let lines = if a.text.ends_with('\n') {
        &lines[..lines.len() - 1]
    } else {
        &lines[..]
    };
    if !lines.iter().any(|l| {
        let t = l.trim();
        let head = t.split_whitespace().next().unwrap_or("");
        l.contains("${") || is_kw_line(t).is_some() || block_name(head).is_some()
    }) {
        let parse = a.text.clone();
        return Loaded {
            src: a,
            parse,
            diags: inc,
        };
    }
    let mods: Vec<Option<&str>> = (0..lines.len()).map(|i| a.module(i + 1)).collect();
    let mut b = Builder {
        lines,
        mods: &mods,
        i: 0,
        defs: HashMap::new(),
        names: lines
            .iter()
            .enumerate()
            .filter_map(|(k, l)| {
                let (w, rest) = is_kw_line(l.trim())?;
                let n: String = rest
                    .chars()
                    .take_while(|&c| c.is_alphanumeric() || c == '_')
                    .collect();
                (w == "def" && !n.is_empty()).then(|| match mods[k] {
                    Some(m) => format!("{m}.{n}"),
                    None => n,
                })
            })
            .collect(),
        styles: lines
            .iter()
            .filter_map(|l| l.trim().strip_prefix("style "))
            .filter_map(|r| r.split_whitespace().next())
            .map(str::to_string)
            .collect(),
        errs: Vec::new(),
    };
    let (items, _, _) = b.block(true);
    let mut x = Expander {
        lines,
        mods: &mods,
        defs: &b.defs,
        text: String::new(),
        parse: String::new(),
        origin: Vec::new(),
        raw: vec![0; lines.len()],
        frames: vec![HashMap::new()],
        calls: Vec::new(),
        steps: 0,
        budget,
        out_of_budget: false,
        errs: Vec::new(),
        shift: (0, 0),
    };
    let _ = x.run(&items, true);
    let mut segs: Vec<Seg> = Vec::new();
    for (i, &o) in x.origin.iter().enumerate() {
        let (path, fl) = a.file_line(o + 1);
        let line = i + 1;
        if let Some(s) = segs.last()
            && s.path == path
            && s.file_line + (line - s.line) == fl
        {
            continue;
        }
        segs.push(Seg {
            line,
            path: path.to_string(),
            file_line: fl,
            module: None,
        });
    }
    if segs.is_empty() {
        segs.push(Seg {
            line: 1,
            path: a.path.clone(),
            file_line: 1,
            module: None,
        });
    }
    let src = Source::spliced(&a.path, &x.text, segs);
    let at_line = |line: usize| {
        let lo = x.raw[line];
        let t = lines[line];
        let lead = t.len() - t.trim_start().len();
        Span::new(lo + lead, lo + t.trim_end().len().max(lead))
    };
    let mut diags = Vec::new();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(a.text.match_indices('\n').map(|(k, _)| k + 1))
        .collect();
    for mut d in inc {
        for l in &mut d.labels {
            let line = line_starts.partition_point(|&s| s <= l.span.lo) - 1;
            if line < lines.len() {
                let base = x.raw[line] as isize - line_starts[line] as isize;
                l.span = Span::new(
                    (l.span.lo as isize + base) as usize,
                    (l.span.hi as isize + base) as usize,
                );
            }
        }
        diags.push(d);
    }
    let mut seen = std::collections::HashSet::new();
    for (line, mut d) in b.errs.into_iter().chain(x.errs) {
        if !seen.insert((line, d.msg.clone())) {
            continue;
        }
        for n in &mut d.notes {
            let parts: Vec<String> = n.split('\u{1}').map(String::from).collect();
            *n = parts
                .iter()
                .enumerate()
                .map(|(k, p)| match (k % 2, p.parse::<usize>()) {
                    (1, Ok(l)) => {
                        let (path, fl) = a.file_line(l + 1);
                        format!("{path}:{fl}")
                    }
                    _ => p.clone(),
                })
                .collect();
        }
        let label = match d.code {
            "E0013" => "expansion stopped here",
            "E0002" => "not a valid macro line",
            _ => "while expanding this line",
        };
        diags.push(d.primary(at_line(line), label));
    }
    Loaded {
        src,
        parse: x.parse,
        diags,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Loaded {
        expand(Source::new("t.fitd", text), Vec::new(), DEFAULT_BUDGET)
    }

    fn visible(l: &Loaded) -> Vec<String> {
        l.parse
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    #[test]
    fn plain_files_pass_through() {
        let text = "node end\nend -> a\nnode a \"x^{2}\"\n| cost $5\n";
        let l = run(text);
        assert_eq!(l.parse, text);
        assert!(l.diags.is_empty());
    }

    #[test]
    fn count_and_part_split_lists() {
        let l = run(
            "def row(xs)\nfor i in 1..count(xs, \";\")\n| ${part(xs, \";\", i)}\nend\nend\nnode a\nrow \"x: int; y: int\"\nrow \"\"\n",
        );
        assert!(
            l.diags.is_empty(),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(visible(&l), ["node a", "| x: int", "| y: int"]);
        let bad = run("node a \"${part(\"a;b\", \";\", 3)}\"\n");
        assert!(bad.diags.iter().any(|d| d.msg.contains("outside 1..2")));
        let ws = run("node a \"${count(\" 90  5 5 \", \" \")}\"\n");
        assert_eq!(visible(&ws), ["node a \"3\""]);
    }

    #[test]
    fn template_rows_call_and_nest() {
        let go = |body: &str| {
            load(
                "t.fitd",
                &format!("include \"tree\"\n{body}"),
                DEFAULT_BUDGET,
            )
        };
        let l = go(
            "tree.unit a A \"x y\"\n  b \"B\" r\n    tree.word c c\n  d D r\nfor i in 1..2\n  tree.unit\n    s${i} \"S ${i}\" r\nend\n",
        );
        let msgs: Vec<_> = l.diags.iter().map(|d| &d.msg).collect();
        assert!(msgs.is_empty(), "{msgs:?}");
        let got: Vec<&str> = l
            .parse
            .lines()
            .filter(|s| s.trim_start().starts_with(['n', '|']))
            .collect();
        assert_eq!(
            got,
            [
                "  node a \"A\"",
                "  | x y",
                "     node b \"B\"",
                "     | r",
                "        node c \"c\"",
                "     node d \"D\"",
                "     | r",
                "  node s1 \"S 1\"",
                "  | r",
                "  node s2 \"S 2\"",
                "  | r",
            ]
        );
        assert!(
            go("tree.unit a b\n")
                .diags
                .iter()
                .any(|d| d.msg.contains("takes 3 arguments"))
        );
        assert!(
            go("tree.unit\n  a \"A\n")
                .diags
                .iter()
                .any(|d| d.msg.contains("unterminated"))
        );
    }

    #[test]
    fn loops_and_interpolation() {
        let l = run(
            "for i in 1..3\n  node s${i} \"Step ${i * 10}\"\n  if i < 3\n    s${i} -> s${i + 1}\n  end\nend\ngroup ${names(\"s\", 1, 3)}\n",
        );
        assert!(
            l.diags.is_empty(),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(
            visible(&l),
            [
                "node s1 \"Step 10\"",
                "s1 -> s2",
                "node s2 \"Step 20\"",
                "s2 -> s3",
                "node s3 \"Step 30\"",
                "group s1, s2, s3"
            ]
        );
        let k = l
            .src
            .text
            .lines()
            .position(|t| t.trim() == "s2 -> s3")
            .unwrap();
        assert_eq!(l.src.file_line(k + 1), ("t.fitd", 4));
    }

    #[test]
    fn comments_are_not_interpolated() {
        let l = run("node a \"$${x} ${1 + 1}\"\n| $${y}\n");
        assert_eq!(visible(&l), ["node a \"${x} 2\"", "| ${y}"]);
        let l = run("for i in 1..1\n  node a${i} \"#${i}\" color=#123  # ${nope}\nend\n");
        assert!(
            l.diags.is_empty(),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(visible(&l), ["node a1 \"#1\" color=#123  # ${nope}"]);
    }

    #[test]
    fn list_loops_and_else() {
        let l = run("for i, s in a, b\n  node ${s}${i}\nend\n");
        assert_eq!(visible(&l), ["node a1", "node b2"]);
        let l = run(
            "for s in auth, front-end, \"Big DB\"\n  if s == \"auth\"\n    node a\n  else if s == \"front-end\"\n    node c\n  else\n    node b\n  end\nend\n",
        );
        assert!(l.diags.is_empty());
        assert_eq!(visible(&l), ["node a", "node c", "node b"]);
    }

    #[test]
    fn recursive_templates() {
        let l = run(
            "fib f 3\ndef fib(id, n)\n  node ${id} \"${n}\"\n  if n >= 2\n    fib ${id + \"a\"} ${n - 1}\n    fib ${id + \"b\"} ${n - 2}\n    ${id} -> ${id}a, ${id}b\n  end\nend\n",
        );
        assert!(
            l.diags.is_empty(),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        let nodes = visible(&l).iter().filter(|s| s.starts_with("node")).count();
        assert_eq!(nodes, 5);
    }

    #[test]
    fn collatz_runs_and_budget_stops_runaway() {
        let collatz = "def c(k)\n  node n${k}\n  if k > 1 and k % 2 == 0\n    n${k} -> n${k / 2}\n    c ${k / 2}\n  else if k > 1\n    n${k} -> n${3 * k + 1}\n    c ${3 * k + 1}\n  end\nend\nc 27\n";
        let l = run(collatz);
        assert!(
            l.diags.is_empty(),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        assert_eq!(
            visible(&l).iter().filter(|s| s.starts_with("node")).count(),
            112
        );
        let l = run("def f(n)\n  f ${n + 1}\nend\nf 0\n");
        assert!(
            l.diags.iter().any(|d| d.msg.contains("nest more than")),
            "{:?}",
            l.diags.iter().map(|d| &d.msg).collect::<Vec<_>>()
        );
        let l = run("for i in 1..1000000000\n  node a${i}\nend\n");
        assert!(l.diags.iter().any(|d| d.code == "E0013"));
    }

    #[test]
    fn errors_point_at_raw_lines() {
        let l = run("node a\nfor i in 1..2\n  node b${j}\nend\nif 1\nnode c\n");
        let msgs: Vec<_> = l
            .diags
            .iter()
            .map(|d| {
                (
                    d.code,
                    d.msg.clone(),
                    l.src.file_line(l.src.line_col(d.pos()).0).1,
                )
            })
            .collect();
        assert!(
            msgs.iter().any(|(c, m, line)| *c == "E0012"
                && m.contains("unknown variable `j`")
                && *line == 3),
            "{msgs:?}"
        );
        assert!(
            msgs.iter()
                .any(|(c, m, line)| *c == "E0002" && m.contains("no matching `end`") && *line == 5),
            "{msgs:?}"
        );
        assert_eq!(l.parse.len(), l.src.text.len());
    }
}
